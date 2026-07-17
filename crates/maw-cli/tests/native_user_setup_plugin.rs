#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn usersetup_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn usersetup_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn usersetup_seed(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-user-setup-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let cache = root.join("cache");
    let projects = home.join(".claude/projects");
    std::fs::create_dir_all(projects.join("-tmp-missing-agents-01")).expect("candidate dir");
    usersetup_write(
        &projects.join("-tmp-existing-project/session-1.jsonl"),
        "{}\n",
    );
    usersetup_write(&projects.join("-tmp-existing-project/note.txt"), "note\n");
    usersetup_write(&projects.join("orphan-root.jsonl"), "{}\n");
    (root, home, config, cache)
}

fn usersetup_command(root: &Path, home: &Path, config: &Path, cache: &Path) -> Command {
    let mut command = Command::new(usersetup_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("MAW_CACHE_DIR", cache)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("MAW_CLAUDE_PROJECTS_DIR", home.join(".claude/projects"))
        .env("MAW_USERSETUP_NOW", "2026-06-25T00:00:00.000Z")
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    command
}

#[test]
fn usersetup_native_porcelain_golden_is_hermetic() {
    let (root, home, config, cache) = usersetup_seed("porcelain");
    let output = usersetup_command(&root, &home, &config, &cache)
        .args(["user-setup", "--porcelain"])
        .output()
        .expect("run user-setup");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = include_str!("fixtures/native-user-setup/porcelain.stdout")
        .replace("{ROOT}", &root.to_string_lossy());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(dispatcher_status("user-setup"), DispatchKind::Native);
    assert!(
        root.join("config").read_dir().is_err(),
        "user-setup must not write config"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn usersetup_native_audit_json_and_guards_are_hermetic() {
    let (root, home, config, cache) = usersetup_seed("audit");
    let output = usersetup_command(&root, &home, &config, &cache)
        .args(["user-setup", "projects", "audit", "--json"])
        .output()
        .expect("run audit");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert_eq!(value["generatedAt"], "2026-06-25T00:00:00.000Z");
    assert_eq!(value["projectCount"], 2);
    assert_eq!(value["entries"][0]["encoded"], "-tmp-existing-project");
    assert_eq!(
        value["entries"][1]["warnings"][0],
        "path confidence is ambiguous"
    );

    let guarded = usersetup_command(&root, &home, &config, &cache)
        .args(["user-setup", "projects", "-audit"])
        .output()
        .expect("guard");
    assert_eq!(guarded.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(guarded.stderr).expect("stderr"),
        "user-setup: unknown argument -audit\n"
    );
    assert!(
        !root.join(".claude/projects").exists(),
        "real cwd/root was not used as HOME"
    );
    let _ = std::fs::remove_dir_all(root);
}
