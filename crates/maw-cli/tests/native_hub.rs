use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
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
    let root = std::env::temp_dir().join(format!("maw-rs-hub-{name}-{stamp}"));
    fs::create_dir_all(&root).expect("temp dir");
    root
}
fn install_plugin(root: &Path) -> PathBuf {
    let plugin = root.join("plugins/hub");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(
        plugin.join("plugin.json"),
        include_str!("fixtures/native-hub/hub-plugin/plugin.json"),
    )
    .expect("manifest");
    fs::write(
        plugin.join("plugin.wasm"),
        include_bytes!("fixtures/native-hub/hub-plugin/plugin.wasm"),
    )
    .expect("wasm");
    root.join("plugins")
}
fn run(root: &Path, args: &[&str]) -> Output {
    let home = root.join("home");
    let config = root.join("config");
    fs::create_dir_all(&home).expect("home");
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", &home)
        .env("MAW_HOME", &home)
        .env("MAW_CONFIG_DIR", &config)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", install_plugin(root))
        .output()
        .expect("run hub")
}
fn success(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn hub_plugin_matches_native_validation_and_constants() {
    let root = temp_dir("validate");
    success(
        run(
            &root,
            &[
                "hub",
                "validate-workspace",
                "--id",
                "ws",
                "--hub-url",
                "wss://hub",
                "--token",
                "t",
            ],
        ),
        "ok\n",
    );
    success(
        run(
            &root,
            &[
                "hub",
                "validate-workspace",
                "--id",
                "ws",
                "--hub-url",
                "http://hub",
                "--token",
                "t",
            ],
        ),
        "invalid: hubUrl must be ws:|wss: (got http:)\n",
    );
    success(run(&root, &["hub", "validate-workspace", "--id", "ws", "--hub-url", "wss://hub", "--token", "t", "--shared-agent", "mawjs", "--plan-json"]), "{\"command\":\"hub\",\"kind\":\"validate-workspace\",\"input\":{\"hubUrl\":\"wss://hub\",\"id\":\"ws\",\"sharedAgents\":[\"mawjs\"],\"token\":\"t\"},\"ok\":true,\"reason\":null}\n");
    success(
        run(&root, &["hub", "constants"]),
        "hub constants heartbeat-ms=30000 reconnect-base-ms=1000 reconnect-max-ms=60000\n",
    );
}

#[test]
fn hub_plugin_matches_native_workspace_loader() {
    let root = temp_dir("load");
    let config = root.join("config");
    let config_arg = config.to_string_lossy();
    success(
        run(
            &root,
            &["hub", "load-workspaces", "--config-dir", &config_arg],
        ),
        "configs=0 warnings=0\n",
    );
    let workspaces = config.join("workspaces");
    fs::write(workspaces.join("valid.json"), r#"{"id":"alpha","hubUrl":"wss://hub.example.test","token":"secret","sharedAgents":["mawjs"]}"#).expect("valid");
    fs::write(workspaces.join("invalid.json"), r#"{"id":"bad","hubUrl":"https://not-websocket.example.test","token":"secret","sharedAgents":[]}"#).expect("invalid");
    fs::write(workspaces.join("broken.json"), "{not json").expect("broken");
    let output = run(
        &root,
        &[
            "hub",
            "load-workspaces",
            "--config-dir",
            &config_arg,
            "--plan-json",
        ],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(
        value["configs"][0],
        serde_json::json!({"id":"alpha","hubUrl":"wss://hub.example.test","token":"secret","sharedAgents":["mawjs"]})
    );
    let warnings = value["warnings"]
        .as_array()
        .expect("warnings")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        warnings.contains("invalid workspace config: invalid.json"),
        "{warnings}"
    );
    assert!(
        warnings.contains("failed to parse workspace config: broken.json"),
        "{warnings}"
    );
}

#[test]
fn hub_plugin_keeps_usage_bytes_and_removes_only_hub_dispatch() {
    let output = run(&temp_dir("usage"), &["hub"]);
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "hub: expected validate-workspace or load-workspaces\nusage: maw-rs hub validate-workspace [--id <id>] [--hub-url <ws-url>] [--token <token>] [--shared-agent <agent>]... [--plan-json]\n       maw-rs hub load-workspaces --config-dir <dir> [--plan-json]\n       maw-rs hub constants [--plan-json]\n");
    assert_eq!(
        maw_cli::dispatcher_status("hub"),
        maw_cli::DispatchKind::NativeError
    );
    assert_eq!(
        maw_cli::dispatcher_status("xdg"),
        maw_cli::DispatchKind::Native
    );
}
