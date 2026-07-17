#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
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
    let path = std::env::temp_dir().join(format!("maw-rs-fleet-gc-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_exe(path: &Path, body: &str) {
    fs::write(path, body).expect("write exe");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn write_json(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(path, text).expect("write json");
}

fn seed_root(root: &Path) -> PathBuf {
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_exe(
        &bin_dir.join("tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  list-sessions) printf '%s' "$MAW_FAKE_TMUX_SESSIONS" ;;
  *) printf 'unexpected tmux %s\n' "$1" >&2; exit 9 ;;
esac
"#,
    );

    fs::create_dir_all(root.join("ghq/github.com/acme/keep")).expect("existing repo");
    write_json(
        &root.join("config/maw.config.json"),
        r#"{"node":"fleet-test"}"#,
    );
    write_json(
        &root.join("state/fleet/01-live.json"),
        r#"{"name":"01-live","windows":[{"name":"live","repo":"acme/live-missing"}]}"#,
    );
    write_json(
        &root.join("state/fleet/02-keep.json"),
        r#"{"name":"02-keep","windows":[{"name":"keep","repo":"acme/keep"}]}"#,
    );
    write_json(
        &root.join("state/fleet/03-ghost.json"),
        r#"{"name":"03-ghost","windows":[{"name":"ghost","repo":"acme/ghost"}]}"#,
    );
    write_json(
        &root.join("home/.maw/fleet/04-legacy-ghost.json"),
        r#"{"name":"04-legacy-ghost","windows":[{"name":"legacy","repo":"acme/legacy-ghost"}]}"#,
    );
    bin_dir
}

fn run(root: &Path, bin_dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", bin_dir)
        .env("HOME", root.join("home"))
        .env("MAW_CONFIG_DIR", root.join("config"))
        .env("MAW_STATE_DIR", root.join("state"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("MAW_TEST_MODE", "1")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("MAW_FAKE_TMUX_SESSIONS", "01-live\n")
        .output()
        .expect("run maw-rs")
}

#[test]
fn native_fleet_gc_renames_state_and_legacy_ghosts_without_real_tmux() {
    assert_eq!(dispatcher_status("fleet"), DispatchKind::Native);
    let root = temp_dir("rename");
    let bin_dir = seed_root(&root);

    let dry_run = run(&root, &bin_dir, &["fleet", "gc", "--dry-run"]);
    assert!(
        dry_run.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&dry_run.stdout),
        String::from_utf8_lossy(&dry_run.stderr)
    );
    let stdout = String::from_utf8(dry_run.stdout).expect("dry stdout");
    assert!(stdout.contains("[dry-run] would disable"));
    assert!(stdout.contains("03-ghost.json"));
    assert!(stdout.contains("04-legacy-ghost.json"));
    assert!(!stdout.contains("01-live.json"));
    assert!(!stdout.contains("02-keep.json"));
    assert!(root.join("state/fleet/03-ghost.json").exists());
    assert!(!root.join("state/fleet/03-ghost.json.disabled").exists());

    let real = run(&root, &bin_dir, &["fleet", "gc"]);
    assert!(
        real.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&real.stdout),
        String::from_utf8_lossy(&real.stderr)
    );
    assert!(root.join("state/fleet/01-live.json").exists());
    assert!(root.join("state/fleet/02-keep.json").exists());
    assert!(!root.join("state/fleet/03-ghost.json").exists());
    assert!(root.join("state/fleet/03-ghost.json.disabled").exists());
    assert!(!root.join("home/.maw/fleet/04-legacy-ghost.json").exists());
    assert!(root
        .join("home/.maw/fleet/04-legacy-ghost.json.disabled")
        .exists());
}
