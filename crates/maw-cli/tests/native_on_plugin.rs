#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
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
    let path = std::env::temp_dir().join(format!("maw-rs-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], cwd: &Path, maw_home: &Path, home: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_HOME", maw_home)
        .env("HOME", home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

#[test]
fn native_on_once_timeout_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("on-once");
    let cwd = root.join("repo");
    let maw_home = root.join("maw-home");
    let home = root.join("home");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(maw_home.join("config")).expect("config dir");
    fs::create_dir_all(&home).expect("home");
    fs::write(
        maw_home.join("config/maw.config.json"),
        r#"{
  "node": "local",
  "triggers": [
    {
      "name": "old"
    }
  ]
}
"#,
    )
    .expect("seed config");

    let output = run(
        &[
            "on",
            "neo",
            "idle",
            "--once",
            "maw",
            "hey",
            "homekeeper",
            "done",
            "--timeout",
            "12ms",
        ],
        &cwd,
        &maw_home,
        &home,
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-on/on-once.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");

    let config: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(maw_home.join("config/maw.config.json")).expect("config"),
    )
    .expect("config json");
    assert_eq!(config["node"], "local");
    assert_eq!(config["triggers"].as_array().expect("triggers").len(), 2);
    assert_eq!(
        config["triggers"][1],
        serde_json::json!({
            "action": "maw hey homekeeper done",
            "name": "on-neo-idle",
            "on": "agent-idle",
            "once": true,
            "repo": "neo",
            "timeout": 12
        })
    );
}

#[test]
fn native_on_is_registered_and_uses_temp_home_offline() {
    assert_eq!(
        maw_cli::dispatcher_status("on"),
        maw_cli::DispatchKind::Native
    );
    assert!(maw_cli::native_dispatch_commands().contains(&"on"));

    let root = temp_dir("on-usage");
    let cwd = root.join("repo");
    let maw_home = root.join("maw-home");
    let home = root.join("home");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&home).expect("home");

    let output = run(&["on"], &cwd, &maw_home, &home);

    assert!(output.status.success());
    assert!(String::from_utf8(output.stdout)
        .expect("stdout")
        .contains("Usage:"));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert!(
        !maw_home.join("config/maw.config.json").exists(),
        "usage must not create config"
    );
}
