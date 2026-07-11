use std::{
    fs,
    io::{Read as _, Write as _},
    net::{Ipv4Addr, TcpListener},
    path::{Path, PathBuf},
    process::{Command, Output},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-avengers-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn install_plugin(root: &Path) -> PathBuf {
    let plugin = root.join("plugins/avengers");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(
        plugin.join("plugin.json"),
        include_str!("fixtures/native-avengers/avengers-plugin/plugin.json"),
    )
    .expect("plugin json");
    fs::write(
        plugin.join("plugin.wasm"),
        include_bytes!("fixtures/native-avengers/avengers-plugin/plugin.wasm"),
    )
    .expect("plugin wasm");
    root.join("plugins")
}

fn serve_json(path: &str, body: &'static str) -> String {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind server");
    let port = listener.local_addr().expect("server address").port();
    let expected = format!("GET {path} ");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept request");
        let mut request = [0_u8; 4096];
        let read = stream.read(&mut request).expect("read request");
        let request = String::from_utf8_lossy(&request[..read]).into_owned();
        assert!(request.starts_with(&expected), "{request}");
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });
    format!("http://127.0.0.1:{port}{path}")
}

fn run(root: &Path, base: Option<&str>, args: &[&str]) -> Output {
    let home = root.join("home");
    let config = home.join("config");
    let cwd = root.join("repo");
    fs::create_dir_all(&config).expect("config dir");
    fs::create_dir_all(&cwd).expect("cwd");
    if let Some(base) = base {
        fs::write(
            config.join("maw.config.json"),
            serde_json::json!({"avengers": base}).to_string(),
        )
        .expect("config");
    }
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("HOME", &home)
        .env("MAW_HOME", &home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", install_plugin(root))
        .output()
        .expect("run maw-rs")
}

fn assert_success(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

fn normalize_health_latency(mut output: String) -> String {
    let marker = "\x1b[90m";
    let start = output.find(marker).expect("latency color") + marker.len();
    let end = output[start..].find("ms ·").expect("latency suffix") + start;
    output.replace_range(start..end, "<latency>");
    output
}

#[test]
fn avengers_plugin_fallthrough_matches_native_status_output() {
    let root = temp_dir("status");
    let url = serve_json(
        "/all",
        r#"[{"name":"alpha","remaining":80,"limit":100},{"email":"beta@example.test","requests_remaining":10,"requests_limit":100}]"#,
    );
    let base = url.trim_end_matches("/all");
    let output = run(&root, Some(base), &["avengers", "status"]);
    let expected = format!(
        "\n\x1b[36;1mAvengers Status\x1b[0m  \x1b[90m{base}\x1b[0m\n\n  \x1b[32m●\x1b[0m  {:<30}  \x1b[32m80/100 (80%)\x1b[0m\n  \x1b[31m●\x1b[0m  {:<30}  \x1b[31m10/100 (10%)\x1b[0m\n\n",
        "alpha", "beta@example.test"
    );

    assert_success(output, &expected);
}

#[test]
fn avengers_plugin_fallthrough_matches_native_json_and_health_outputs() {
    for (name, path, body, title) in [
        (
            "best",
            "/best",
            r#"{"account":"alpha","remaining":80}"#,
            "Best Account",
        ),
        (
            "traffic",
            "/traffic-stats",
            r#"{"rolling":[1,2],"total":123}"#,
            "Traffic Stats",
        ),
    ] {
        let root = temp_dir(name);
        let url = serve_json(path, body);
        let base = url.trim_end_matches(path);
        let value: serde_json::Value = serde_json::from_str(body).expect("json body");
        let expected = format!(
            "\n\x1b[36;1m{title}\x1b[0m\n\n  {}\n\n",
            serde_json::to_string_pretty(&value).expect("pretty json")
        );

        assert_success(run(&root, Some(base), &["avengers", name]), &expected);
    }

    let root = temp_dir("health");
    let url = serve_json("/all", r#"[{"name":"alpha"},{"name":"beta"}]"#);
    let base = url.trim_end_matches("/all");
    let expected = format!(
        "\n\x1b[32m●\x1b[0m  Avengers \x1b[32monline\x1b[0m  \x1b[90m<latency>ms · 2 accounts\x1b[0m\n   \x1b[90m{base}\x1b[0m\n\n"
    );
    let output = run(&root, Some(base), &["avengers", "health"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize_health_latency(String::from_utf8(output.stdout).expect("stdout")),
        expected
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn avengers_plugin_keeps_missing_config_contract() {
    let output = run(&temp_dir("missing"), None, &["avengers"]);
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr"),
        "Avengers not configured. Add to maw.config.json:\n  \"avengers\": \"http://white.local:8090\"\n"
    );
}

#[test]
fn avengers_native_dispatcher_registration_is_removed() {
    assert_eq!(
        maw_cli::dispatcher_status("avengers"),
        maw_cli::DispatchKind::NativeError
    );
}
