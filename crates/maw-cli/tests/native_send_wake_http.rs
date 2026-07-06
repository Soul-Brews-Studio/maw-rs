use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_auth::{build_from_sign_payload, hash_body, sign, verify_hmac_sig};
use maw_cli::run_cli_async;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;

#[derive(Debug)]
struct CapturedRequest {
    method: String,
    path: String,
    headers: BTreeMap<String, String>,
    body: String,
}

static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
const FEDERATION_TOKEN: &str = "known-federation-token";

fn env_lock() -> &'static Mutex<()> {
    ENV_LOCK.get_or_init(|| Mutex::new(()))
}

#[tokio::test]
async fn native_send_posts_signed_api_send_to_configured_peer() {
    let _guard = env_lock().lock().await;
    let env = TestEnv::new("native-send");
    let peer_key = "known-peer-key";
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    env.write_config(&format!("http://{addr}"));
    std::env::set_var("MAW_HOME", &env.root);
    std::env::set_var("MAW_PEER_KEY", peer_key);

    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let captured = read_one_http_request(&mut socket).await;
        write_json_response(
            &mut socket,
            r#"{"ok":true,"target":"agent","state":"queued"}"#,
        )
        .await;
        tx.send(captured).expect("send capture");
    });

    let output = run_cli_async(&args(&[
        "send",
        "remote:agent",
        "hello",
        "there",
        "--inbox",
        "--from",
        "sender-oracle:sender-node",
    ]))
    .await;

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(output.stdout, "queued agent\n");

    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/send");
    assert_eq!(
        captured.body,
        r#"{"target":"agent","text":"hello there","inbox":true}"#
    );
    assert_common_v3_headers_and_signature(
        &captured,
        FEDERATION_TOKEN,
        peer_key,
        "sender-oracle:sender-node",
        "/api/send",
    );
}

#[tokio::test]
async fn native_wake_posts_signed_api_wake_to_configured_peer() {
    let _guard = env_lock().lock().await;
    let env = TestEnv::new("native-wake");
    let peer_key = "known-peer-key";
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let addr = listener.local_addr().expect("addr");
    env.write_config(&format!("http://{addr}"));
    std::env::set_var("MAW_HOME", &env.root);
    std::env::set_var("MAW_PEER_KEY", peer_key);

    let (tx, rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.expect("accept");
        let captured = read_one_http_request(&mut socket).await;
        write_json_response(&mut socket, r#"{"ok":true,"target":"agent"}"#).await;
        tx.send(captured).expect("send capture");
    });

    let output = run_cli_async(&args(&[
        "wake",
        "remote:agent",
        "--task",
        "fix issue",
        "--from",
        "sender-oracle:sender-node",
    ]))
    .await;

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(output.stdout, "woke agent\n");

    let captured = rx.await.expect("capture");
    assert_eq!(captured.method, "POST");
    assert_eq!(captured.path, "/api/wake");
    assert_eq!(captured.body, r#"{"target":"agent","task":"fix issue"}"#);
    assert_common_v3_headers_and_signature(
        &captured,
        FEDERATION_TOKEN,
        peer_key,
        "sender-oracle:sender-node",
        "/api/wake",
    );
}

fn assert_common_v3_headers_and_signature(
    captured: &CapturedRequest,
    federation_token: &str,
    peer_key: &str,
    from: &str,
    path: &str,
) {
    assert_eq!(
        captured.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        captured.headers.get("x-maw-from").map(String::as_str),
        Some(from)
    );
    assert_eq!(
        captured
            .headers
            .get("x-maw-auth-version")
            .map(String::as_str),
        Some("v3")
    );
    let timestamp = captured
        .headers
        .get("x-maw-timestamp")
        .expect("timestamp")
        .parse::<i64>()
        .expect("timestamp i64");
    let signature = captured
        .headers
        .get("x-maw-signature-v3")
        .expect("signature");
    assert_eq!(signature.len(), 64);
    assert!(signature.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(signature, &signature.to_ascii_lowercase());
    assert_eq!(
        captured.headers.get("x-maw-signature").map(String::as_str),
        Some(sign(federation_token, "POST", path, timestamp, "").as_str())
    );

    let body_hash = hash_body(Some(captured.body.as_bytes()));
    let payload = build_from_sign_payload(from, timestamp, "POST", path, &body_hash);
    assert!(verify_hmac_sig(peer_key, &payload, signature));
}

async fn write_json_response(socket: &mut tokio::net::TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    socket
        .write_all(response.as_bytes())
        .await
        .expect("response");
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

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

struct TestEnv {
    root: PathBuf,
}

impl TestEnv {
    fn new(name: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time")
            .as_nanos();
        let root = std::env::temp_dir().join(format!("maw-rs-{name}-{nonce}"));
        std::fs::create_dir_all(root.join("config")).expect("config dir");
        Self { root }
    }

    fn write_config(&self, peer_url: &str) {
        std::fs::write(
            self.root.join("config").join("maw.config.json"),
            format!(
                r#"{{"node":"sender-node","oracle":"sender-oracle","federationToken":"{FEDERATION_TOKEN}","namedPeers":[{{"name":"remote","url":"{peer_url}"}}]}}"#
            ),
        )
        .expect("write config");
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
        std::env::remove_var("MAW_HOME");
        std::env::remove_var("MAW_PEER_KEY");
    }
}
