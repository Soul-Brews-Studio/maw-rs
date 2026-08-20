#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::collections::BTreeMap;

use maw_auth::{build_from_sign_payload, hash_body, sign, verify_hmac_sig};
use maw_transport::{PeerSendRequest, PeerWakeRequest, ReqwestHttpTransportIo};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

#[tokio::test]
async fn reqwest_http_transport_posts_api_send_with_verifiable_v3_signature() {
    let peer_key = "known-peer-key";
    let federation_token = "known-federation-token";
    let timestamp = 1_700_000_123_i64;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let captured = read_one_http_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 53\r\n\r\n{\"ok\":true,\"target\":\"remote-oracle\",\"state\":\"queued\"}",
            )
            .await
            .expect("response");
        tx.send(captured).expect("send capture");
    });

    let client = ReqwestHttpTransportIo::new(2_000).expect("client");
    let response = client
        .send_peer(&PeerSendRequest {
            peer_url: format!("http://{addr}"),
            target: "remote-oracle".to_owned(),
            text: "E1 signed capture".to_owned(),
            inbox: Some(true),
            from: "sender-oracle:sender-node".to_owned(),
            federation_token: federation_token.to_owned(),
            peer_key: peer_key.to_owned(),
            timestamp,
        })
        .await
        .expect("send peer");

    assert!(response.delivered_or_queued());
    assert_eq!(response.state.as_deref(), Some("queued"));

    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/send");
    assert_eq!(
        captured.body,
        r#"{"target":"remote-oracle","text":"E1 signed capture","inbox":true}"#
    );
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        captured.headers.get("x-maw-from").map(String::as_str),
        Some("sender-oracle:sender-node")
    );
    assert_eq!(
        captured.headers.get("x-maw-timestamp").map(String::as_str),
        Some("1700000123")
    );
    assert_eq!(
        captured
            .headers
            .get("x-maw-auth-version")
            .map(String::as_str),
        Some("v3")
    );
    let signature = captured
        .headers
        .get("x-maw-signature-v3")
        .expect("signature");
    assert_eq!(signature.len(), 64);
    assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(signature, &signature.to_ascii_lowercase());

    let legacy_signature = captured
        .headers
        .get("x-maw-signature")
        .expect("legacy signature");
    assert_eq!(
        legacy_signature,
        &sign(federation_token, "POST", "/api/send", timestamp, "")
    );

    let body_hash = hash_body(Some(captured.body.as_bytes()));
    let payload = build_from_sign_payload(
        "sender-oracle:sender-node",
        timestamp,
        "POST",
        "/api/send",
        &body_hash,
    );
    assert!(verify_hmac_sig(peer_key, &payload, signature));
}

/// #685: `verify_protected_request_outcome` on the receiving serve already
/// names the reason for a 401/403 in the response body
/// (`{"error":"unauthorized","decision":"<reason>"}`) -- `probe_peer_auth`
/// must carry that reason through, not reduce straight to a bare bool the
/// way it silently discarded the body before this fix.
#[tokio::test]
async fn reqwest_http_transport_probe_peer_auth_carries_the_named_401_reason() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let captured = read_one_http_request(&mut socket).await;
        let body = br#"{"error":"unauthorized","decision":"pubkey-mismatch"}"#;
        socket
            .write_all(
                format!(
                    "HTTP/1.1 401 Unauthorized\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            )
            .await
            .expect("status line");
        socket.write_all(body).await.expect("body");
        tx.send(captured).expect("send capture");
    });

    let client = ReqwestHttpTransportIo::new(2_000).expect("client");
    let result = client
        .probe_peer_auth(&PeerWakeRequest {
            peer_url: format!("http://{addr}"),
            target: String::new(),
            task: None,
            from: "sender-oracle:sender-node".to_owned(),
            federation_token: "known-federation-token".to_owned(),
            peer_key: "known-peer-key".to_owned(),
            timestamp: 1_700_000_123_i64,
        })
        .await
        .expect("probe peer auth");

    assert_eq!(result.ok, Some(false));
    assert_eq!(result.reason.as_deref(), Some("pubkey-mismatch"));

    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/probe");
}

#[tokio::test]
async fn reqwest_http_transport_probe_peer_auth_ok_carries_no_reason() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let _captured = read_one_http_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 11\r\n\r\n{\"ok\":true}",
            )
            .await
            .expect("response");
    });

    let client = ReqwestHttpTransportIo::new(2_000).expect("client");
    let result = client
        .probe_peer_auth(&PeerWakeRequest {
            peer_url: format!("http://{addr}"),
            target: String::new(),
            task: None,
            from: "sender-oracle:sender-node".to_owned(),
            federation_token: "known-federation-token".to_owned(),
            peer_key: "known-peer-key".to_owned(),
            timestamp: 1_700_000_123_i64,
        })
        .await
        .expect("probe peer auth");

    assert_eq!(result.ok, Some(true));
    assert!(result.reason.is_none());
}

#[tokio::test]
async fn reqwest_http_transport_posts_api_wake_with_verifiable_v3_signature() {
    let peer_key = "known-peer-key";
    let federation_token = "known-federation-token";
    let timestamp = 1_700_000_456_i64;
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    let (tx, rx) = tokio::sync::oneshot::channel();

    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let captured = read_one_http_request(&mut socket).await;
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 36\r\n\r\n{\"ok\":true,\"target\":\"remote-oracle\"}",
            )
            .await
            .expect("response");
        tx.send(captured).expect("send capture");
    });

    let client = ReqwestHttpTransportIo::new(2_000).expect("client");
    let response = client
        .wake_peer(&PeerWakeRequest {
            peer_url: format!("http://{addr}"),
            target: "remote-oracle".to_owned(),
            task: Some("fix issue".to_owned()),
            from: "sender-oracle:sender-node".to_owned(),
            federation_token: federation_token.to_owned(),
            peer_key: peer_key.to_owned(),
            timestamp,
        })
        .await
        .expect("wake peer");

    assert!(response.ok);
    assert_eq!(response.target.as_deref(), Some("remote-oracle"));

    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/wake");
    assert_eq!(
        captured.body,
        r#"{"target":"remote-oracle","task":"fix issue"}"#
    );
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        captured.headers.get("x-maw-from").map(String::as_str),
        Some("sender-oracle:sender-node")
    );
    assert_eq!(
        captured.headers.get("x-maw-timestamp").map(String::as_str),
        Some("1700000456")
    );
    assert_eq!(
        captured
            .headers
            .get("x-maw-auth-version")
            .map(String::as_str),
        Some("v3")
    );
    let signature = captured
        .headers
        .get("x-maw-signature-v3")
        .expect("signature");
    assert_eq!(signature.len(), 64);
    assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(signature, &signature.to_ascii_lowercase());

    let legacy_signature = captured
        .headers
        .get("x-maw-signature")
        .expect("legacy signature");
    assert_eq!(
        legacy_signature,
        &sign(federation_token, "POST", "/api/wake", timestamp, "")
    );

    let body_hash = hash_body(Some(captured.body.as_bytes()));
    let payload = build_from_sign_payload(
        "sender-oracle:sender-node",
        timestamp,
        "POST",
        "/api/wake",
        &body_hash,
    );
    assert!(verify_hmac_sig(peer_key, &payload, signature));
}

fn failover_request(peer_url: String) -> PeerSendRequest {
    PeerSendRequest {
        peer_url,
        target: "oracle".to_owned(),
        text: "hello".to_owned(),
        inbox: None,
        from: "sender:node".to_owned(),
        federation_token: "token".to_owned(),
        peer_key: "key".to_owned(),
        timestamp: 1_700_000_123,
    }
}
const SEND_OK: &str = "HTTP/1.1 200 OK\r\ncontent-length: 11\r\n\r\n{\"ok\":true}";
const SEND_401: &str = "HTTP/1.1 401 Unauthorized\r\ncontent-length: 55\r\n\r\n{\"error\":\"connect failed posting timeout ECONNREFUSED\"}";
const SEND_307: &str = "HTTP/1.1 307 X\r\nlocation:LOCATION\r\n\r\n{\"ok\":false}";
async fn failover_server(response: Option<String>) -> (String, tokio::sync::oneshot::Receiver<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("address");
    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let _ = tx.send(());
        let _ = read_one_http_request(&mut socket).await;
        if let Some(response) = response {
            socket
                .write_all(response.as_bytes())
                .await
                .expect("respond");
        } else {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    (format!("http://{address}"), rx)
}
async fn failover_send(
    addresses: &[String],
    timeout_ms: u64,
) -> Result<maw_transport::PeerSendResponse, String> {
    ReqwestHttpTransportIo::new(timeout_ms)
        .expect("client")
        .send_peer_addresses(&failover_request(addresses[0].clone()), addresses)
        .await
}
async fn closed_url() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    format!("http://{}/timeout", listener.local_addr().expect("address"))
}
async fn assert_untouched(receiver: tokio::sync::oneshot::Receiver<()>) {
    let wait = std::time::Duration::from_millis(50);
    assert!(tokio::time::timeout(wait, receiver).await.is_err());
}
#[tokio::test]
async fn send_peer_failover_contract() {
    let (first, attempted) = failover_server(None).await;
    let (second, _) = failover_server(Some(SEND_OK.to_owned())).await;
    let error = failover_send(&[first, second.clone()], 500)
        .await
        .expect_err("post-request timeout stops");
    attempted.await.expect("first attempted");
    assert!(error.starts_with("timed out posting "), "{error}");
    let response = SEND_307.replace("LOCATION", &closed_url().await);
    let (first, _) = failover_server(Some(response)).await;
    let attempts = [first, second.clone()];
    let error = failover_send(&attempts, 500).await.expect_err("307 stops");
    assert!(error.contains("HTTP 307"), "{error}");
    let response = failover_send(&[closed_url().await, second], 500)
        .await
        .expect("connect fallback succeeds");
    assert!(response.delivered_or_queued() && response.warning.is_none());
    let (first, _) = failover_server(Some(SEND_401.to_owned())).await;
    let (second, untouched) = failover_server(Some(SEND_OK.to_owned())).await;
    let error = failover_send(&[first, second], 1_000)
        .await
        .expect_err("401 stops");
    assert!(error.contains("HTTP 401"), "{error}");
    assert_untouched(untouched).await;
    let attempts = vec![closed_url().await, closed_url().await];
    let error = failover_send(&attempts, 1_000)
        .await
        .expect_err("exhausted");
    for address in &attempts {
        assert!(error.contains(address), "{error}");
    }
    assert_eq!(error.matches("REFUSED").count(), 2, "{error}");
}
async fn read_one_http_request(socket: &mut tokio::net::TcpStream) -> CapturedRequest {
    let mut buffer = Vec::new();
    let mut temp = [0_u8; 1024];
    let header_end = loop {
        let read = socket.read(&mut temp).await.expect("read");
        assert_ne!(read, 0, "client closed before headers");
        buffer.extend_from_slice(&temp[..read]);
        if let Some(index) = find_header_end(&buffer) {
            break index;
        }
    };
    let header_text = String::from_utf8(buffer[..header_end].to_vec()).expect("utf8 headers");
    let mut lines = header_text.split("\r\n");
    let request_line = lines.next().expect("request line");
    let mut request_parts = request_line.split_whitespace();
    let method = request_parts.next().expect("method").to_owned();
    let path = request_parts.next().expect("path").to_owned();
    let headers = lines
        .filter_map(|line| {
            let (name, value) = line.split_once(':')?;
            Some((name.to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect::<BTreeMap<_, _>>();
    let content_length = headers
        .get("content-length")
        .expect("content-length")
        .parse::<usize>()
        .expect("content-length usize");
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let read = socket.read(&mut temp).await.expect("read body");
        assert_ne!(read, 0, "client closed before body");
        buffer.extend_from_slice(&temp[..read]);
    }
    let body = String::from_utf8(buffer[body_start..body_start + content_length].to_vec())
        .expect("utf8 body");
    CapturedRequest {
        method,
        path,
        headers,
        body,
    }
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}
