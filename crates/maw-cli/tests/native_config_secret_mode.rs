#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic

//! #838 — `maw.config.json` carries `federationToken` and
//! `env.CLAUDE_CODE_OAUTH_TOKEN`, so every command that writes it must leave
//! the file owner-only (0600). Three live writers reach that same path:
//!
//! * `maw config set`  -> `config_atomic_write`     (also used by `maw agents gc`)
//! * `maw init`        -> `init_write_json_atomic`
//! * `maw on`          -> `write_json_atomic`
//!
//! Each is exercised as a real subprocess so the process umask applies, which
//! is exactly how the bug shipped: `std::fs::write` creates 0666 & ~umask.

use std::{
    fs,
    os::unix::fs::PermissionsExt as _,
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
    let path = std::env::temp_dir().join(format!("maw-rs-secret-mode-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", "/nonexistent-bin")
        .env("HOME", root.join("home"))
        .env("MAW_CONFIG_DIR", root.join("config"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn assert_ok(output: &Output) {
    assert!(
        output.status.success(),
        "command failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn mode_of(path: &Path) -> u32 {
    fs::metadata(path)
        .expect("config metadata")
        .permissions()
        .mode()
        & 0o777
}

fn assert_owner_only(path: &Path, writer: &str) {
    let mode = mode_of(path);
    assert_eq!(
        mode,
        0o600,
        "{writer} left {} at {mode:o}; group/other must not be able to read the federation token",
        path.display()
    );
}

fn seed_config(root: &Path, body: &str) -> PathBuf {
    let dir = root.join("config");
    fs::create_dir_all(&dir).expect("config dir");
    let path = dir.join("maw.config.json");
    fs::write(&path, body).expect("seed config");
    // Seeded 0644 on purpose: the write path must *tighten* an already-loose
    // file, not merely avoid loosening a fresh one.
    fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("seed mode");
    path
}

#[test]
fn config_set_leaves_the_credential_file_owner_only() {
    let root = temp_dir("config-set");
    let path = seed_config(
        &root,
        "{\n  \"node\": \"old-node\",\n  \"federationToken\": \"super-secret-1234\"\n}\n",
    );

    assert_ok(&run(&root, &["config", "set", "node", "new-node"]));

    let body = fs::read_to_string(&path).expect("config body");
    assert!(
        body.contains("\"node\": \"new-node\""),
        "write did not land: {body}"
    );
    assert!(
        body.contains("super-secret-1234"),
        "token was dropped: {body}"
    );
    assert_owner_only(&path, "maw config set");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn init_writes_the_credential_file_owner_only() {
    let root = temp_dir("init");
    fs::create_dir_all(root.join("config")).expect("config dir");

    assert_ok(&run(
        &root,
        &[
            "init",
            "--non-interactive",
            "--node",
            "testnode",
            "--federate",
            "--federation-token",
            "super-secret-1234",
            "--token",
            "oauth-secret-5678",
        ],
    ));

    let path = root.join("config").join("maw.config.json");
    let body = fs::read_to_string(&path).expect("config body");
    assert!(
        body.contains("super-secret-1234"),
        "federation token missing: {body}"
    );
    assert!(
        body.contains("oauth-secret-5678"),
        "oauth token missing: {body}"
    );
    assert_owner_only(&path, "maw init");

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn on_leaves_the_credential_file_owner_only() {
    let root = temp_dir("on");
    let path = seed_config(
        &root,
        "{\n  \"node\": \"local\",\n  \"federationToken\": \"super-secret-1234\"\n}\n",
    );

    assert_ok(&run(
        &root,
        &["on", "neo", "idle", "maw", "hey", "peer", "done"],
    ));

    let body = fs::read_to_string(&path).expect("config body");
    assert!(
        body.contains("\"triggers\""),
        "trigger not appended: {body}"
    );
    assert!(
        body.contains("super-secret-1234"),
        "token was dropped: {body}"
    );
    assert_owner_only(&path, "maw on");

    let _ = fs::remove_dir_all(&root);
}

/// `maw init --backup` copies the old config aside before overwriting it. The
/// copy holds the same tokens, and `fs::copy` inherits the source's mode — so
/// backing up a legacy 0644 config minted a *fresh* world-readable credential
/// file today. Same bug as #838, one step removed.
#[test]
fn init_backup_copy_is_owner_only_even_from_a_loose_source() {
    let root = temp_dir("init-backup");
    seed_config(
        &root,
        "{\n  \"node\": \"old-node\",\n  \"federationToken\": \"super-secret-1234\"\n}\n",
    );

    assert_ok(&run(
        &root,
        &[
            "init",
            "--non-interactive",
            "--node",
            "testnode",
            "--backup",
        ],
    ));

    let backups = fs::read_dir(root.join("config"))
        .expect("config dir")
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("maw.config.json.bak."))
        })
        .collect::<Vec<_>>();
    assert_eq!(
        backups.len(),
        1,
        "expected exactly one backup, got {backups:?}"
    );
    let backup = &backups[0];
    let body = fs::read_to_string(backup).expect("backup body");
    assert!(
        body.contains("super-secret-1234"),
        "backup should carry the old token, else this test proves nothing: {body}"
    );
    assert_owner_only(backup, "maw init --backup");

    let _ = fs::remove_dir_all(&root);
}
