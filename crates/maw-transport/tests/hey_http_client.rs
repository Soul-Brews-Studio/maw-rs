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
