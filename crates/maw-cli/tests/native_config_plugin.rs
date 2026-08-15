#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

use maw_cli::{dispatcher_status, DispatchKind};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-config-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn seed_config(root: &Path) -> PathBuf {
    let config = root.join("config");
    fs::create_dir_all(&config).expect("config dir");
    fs::write(
        config.join("maw.config.json"),
        "{\n  \"node\": \"old-node\",\n  \"port\": 3456,\n  \"env\": {\n    \"SECRET\": \"raw-secret-never-print\"\n  }\n}\n",
    )
    .expect("config");
    config
}

fn fake_maw_path(root: &Path) -> PathBuf {
    let bin_dir = root.join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("fake bin");
    let maw = bin_dir.join("maw");
    fs::write(&maw, "#!/bin/sh\necho DELEGATED-MAW \"$@\"\nexit 99\n").expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&maw, fs::Permissions::from_mode(0o755)).expect("chmod fake maw");
    }
    bin_dir
}

fn run_config(root: &Path, args: &[&str]) -> Output {
    let config = root.join("config");
    let fake_bin = fake_maw_path(root);
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", fake_bin)
        .env("HOME", root.join("home"))
        .env("MAW_CONFIG_DIR", config)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn assert_no_delegation(output: &Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "stdout delegated: {stdout}"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "stderr delegated: {stderr}"
    );
}

fn assert_success_golden(root: &Path, args: &[&str], expected: &str) {
    let output = run_config(root, args);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn config_dispatch_is_native() {
    assert_eq!(dispatcher_status("config"), DispatchKind::Native);
}

#[test]
fn config_set_node_and_port_goldens_without_maw_delegation() {
    let root = temp_dir("golden");
    let config = seed_config(&root);
    assert_success_golden(
        &root,
        &["config", "set", "node", "new-node"],
        include_str!("fixtures/native-config/set-node.stdout"),
    );
    assert_success_golden(
        &root,
        &["config", "set", "port", "4567", "--json"],
        include_str!("fixtures/native-config/set-port-json.stdout"),
    );
    let body = fs::read_to_string(config.join("maw.config.json")).expect("config body");
    assert!(body.contains("\"node\": \"new-node\""));
    assert!(body.contains("\"port\": 4567"));
    assert!(body.contains("raw-secret-never-print"));
    assert!(!config.join("maw.config.json.tmp").exists());
}

#[test]
fn config_rejects_bad_node_and_port_before_any_delegation() {
    let root = temp_dir("reject");
    let config = seed_config(&root);
    let output = run_config(&root, &["config", "set", "node", "--bad"]);
    assert_no_delegation(&output);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("invalid node"));
    let output = run_config(&root, &["config", "set", "port", "70000"]);
    assert_no_delegation(&output);
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("invalid port"));
    let body = fs::read_to_string(config.join("maw.config.json")).expect("config body");
    assert!(body.contains("\"node\": \"old-node\""));
    assert!(body.contains("\"port\": 3456"));
}

#[test]
fn config_sources_and_explain_match_maw_js_shape_and_merge_layers() {
    let root = temp_dir("sources");
    let config = root.join("config");
    fs::create_dir_all(&config).expect("config dir");
    fs::write(
        config.join("maw.config.10.json"),
        r#"{
  "node": "base-node",
  "env": { "A": "one", "B": "base" },
  "plugins": { "foo": { "enabled": true, "level": 1 } }
}
"#,
    )
    .expect("base config");
    fs::write(
        config.join("maw.config.90.local.json"),
        r#"{
  "node": "local-node",
  "env": { "B": "local", "C": "three" },
  "plugins": { "foo": { "level": 2 } }
}
"#,
    )
    .expect("local config");

    let output = run_config(&root, &["config", "sources", "--json"]);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sources: serde_json::Value = serde_json::from_slice(&output.stdout).expect("sources json");
    assert_eq!(sources["warnings"], serde_json::json!([]));
    assert_eq!(sources["sources"].as_array().expect("source rows").len(), 2);
    assert_eq!(sources["sources"][0]["weight"], 10);
    assert_eq!(sources["sources"][0]["scope"], "user");
    assert_eq!(sources["sources"][0]["local"], false);
    assert_eq!(sources["sources"][1]["weight"], 90);
    assert_eq!(sources["sources"][1]["local"], true);

    let output = run_config(&root, &["config", "explain", "env.B", "--json"]);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let explain: serde_json::Value = serde_json::from_slice(&output.stdout).expect("explain json");
    assert_eq!(explain["key"], "env.B");
    assert_eq!(explain["finalValue"], "local");
    assert_eq!(explain["entries"].as_array().expect("entries").len(), 2);
    assert_eq!(explain["entries"][0]["value"], "base");
    assert_eq!(explain["entries"][1]["value"], "local");

    let output = run_config(&root, &["config", "show"]);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let shown: serde_json::Value = serde_json::from_slice(&output.stdout).expect("show json");
    assert_eq!(shown["node"], "local-node");
    assert_eq!(
        shown["env"],
        serde_json::json!({ "A": "one", "B": "local", "C": "three" })
    );
    assert_eq!(shown["plugins"]["foo"]["enabled"], true);
    assert_eq!(shown["plugins"]["foo"]["level"], 2);
}

#[test]
fn config_reports_legacy_default_engine_as_a_live_alias() {
    // #682: the key used to be read by nothing and reported by nothing, so a
    // config that looked like it selected claude silently launched codex.
    let root = temp_dir("legacy-alias");
    let config = root.join("config");
    fs::create_dir_all(&config).expect("config dir");
    fs::write(
        config.join("maw.config.50.json"),
        "{\n  \"defaultEngine\": \"claude\"\n}\n",
    )
    .expect("config");

    let output = run_config(&root, &["config", "sources", "--json"]);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let sources: serde_json::Value = serde_json::from_slice(&output.stdout).expect("sources json");
    let warnings = sources["warnings"].as_array().expect("warnings");
    assert_eq!(warnings.len(), 1, "{warnings:?}");
    let note = warnings[0].as_str().expect("warning text");
    assert!(note.contains("defaultEngine"), "{note}");
    assert!(note.contains("wake.engine"), "{note}");
    assert!(note.contains("682"), "{note}");

    let output = run_config(&root, &["config", "explain", "defaultEngine", "--json"]);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let explain: serde_json::Value = serde_json::from_slice(&output.stdout).expect("explain json");
    assert_eq!(explain["finalValue"], "claude");
    let notes = explain["notes"].as_array().expect("notes");
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].as_str().expect("note").contains("wake.engine"));

    // A key with no alias entry stays note-free.
    let output = run_config(&root, &["config", "explain", "node", "--json"]);
    let explain: serde_json::Value = serde_json::from_slice(&output.stdout).expect("explain json");
    assert_eq!(explain["notes"], serde_json::json!([]));
}

#[test]
fn config_explain_masks_secret_values() {
    let root = temp_dir("secret-explain");
    let config = root.join("config");
    fs::create_dir_all(&config).expect("config dir");
    fs::write(
        config.join("maw.config.50.json"),
        "{\n  \"federationToken\": \"super-secret-token-1234\"\n}\n",
    )
    .expect("config");

    let output = run_config(&root, &["config", "explain", "federationToken", "--json"]);
    assert_no_delegation(&output);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(!stdout.contains("super-secret-token-1234"), "{stdout}");
    let explain: serde_json::Value = serde_json::from_str(&stdout).expect("json");
    assert_eq!(explain["finalValue"], "****...1234");
    assert_eq!(explain["entries"][0]["value"], "****...1234");
}
