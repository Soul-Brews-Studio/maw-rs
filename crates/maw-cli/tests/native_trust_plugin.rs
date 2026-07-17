#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn trust_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn trust_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn trust_temp(name: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-native-trust-{name}-{nonce}"));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn trust_command(root: &Path) -> Command {
    let mut command = Command::new(trust_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-115,1,0")
        .env("TMUX_PANE", "%1")
        .env("MAW_SENDER", "bigboy-vps:08-gm-bo")
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn trust_native_lists_seeded_fake_store_only() {
    let root = trust_temp("list");
    let fake_store = root.join("fake-trust.json");
    trust_write(
        &fake_store,
        r#"[
          {"sender":"beta","target":"alpha","addedAt":"2026-06-10T00:00:00.000Z"},
          {"sender":"gamma","target":"delta","addedAt":"2026-06-09T00:00:00.000Z"}
        ]"#,
    );
    let output = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args(["trust", "list"])
        .output()
        .expect("run trust list");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let gamma = stdout.find("gamma ↔ delta").expect("gamma row");
    let beta = stdout.find("beta ↔ alpha").expect("beta row");
    assert!(gamma < beta, "{stdout}");
    assert_eq!(dispatcher_status("trust"), DispatchKind::Native);
    assert!(root.join("xdg-state").read_dir().is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trust_native_add_list_and_revoke_mutate_fake_store_live_without_key_echo() {
    let root = trust_temp("live");
    let fake_store = root.join("fake-trust.json");
    let peer_key = "ed25519:integration-secret-peer-key";
    trust_write(
        &fake_store,
        r#"[{"sender":"alpha","target":"beta","addedAt":"2026-06-09T00:00:00.000Z"}]"#,
    );

    let missing_key = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args(["trust", "add", "gamma", "delta"])
        .output()
        .expect("run trust add without key");
    assert!(!missing_key.status.success());
    assert!(String::from_utf8(missing_key.stderr)
        .expect("missing key stderr")
        .contains("expected --peer-key"));

    let add = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args(["trust", "add", "gamma", "delta", "--peer-key", peer_key])
        .output()
        .expect("run trust add");
    assert!(
        add.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&add.stderr)
    );
    let add_stdout = String::from_utf8(add.stdout).expect("add stdout");
    let add_stderr = String::from_utf8(add.stderr).expect("add stderr");
    assert!(add_stdout.contains("trusted:"), "{add_stdout}");
    assert!(add_stdout.contains("redacted"), "{add_stdout}");
    assert!(!add_stdout.contains(peer_key), "{add_stdout}");
    assert!(!add_stderr.contains(peer_key), "{add_stderr}");
    let body = std::fs::read_to_string(&fake_store).expect("fake store after add");
    assert!(body.contains(peer_key), "{body}");

    let list = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args(["trust", "list"])
        .output()
        .expect("run trust list");
    assert!(list.status.success());
    let list_stdout = String::from_utf8(list.stdout).expect("list stdout");
    assert!(list_stdout.contains("gamma ↔ delta"), "{list_stdout}");
    assert!(list_stdout.contains("redacted"), "{list_stdout}");
    assert!(!list_stdout.contains(peer_key), "{list_stdout}");

    let mismatch = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args([
            "trust",
            "pin",
            "delta",
            "gamma",
            "--peer-key",
            "ed25519:different-secret-peer-key",
        ])
        .output()
        .expect("run trust mismatch");
    assert!(!mismatch.status.success());
    let mismatch_stderr = String::from_utf8(mismatch.stderr).expect("mismatch stderr");
    assert!(
        mismatch_stderr.contains("peer-key mismatch"),
        "{mismatch_stderr}"
    );
    assert!(
        !mismatch_stderr.contains("different-secret-peer-key"),
        "{mismatch_stderr}"
    );

    let no_yes = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args(["trust", "remove", "alpha", "beta"])
        .output()
        .expect("run trust remove no yes");
    assert!(!no_yes.status.success());
    assert!(String::from_utf8(no_yes.stderr)
        .expect("no yes stderr")
        .contains("without --yes"));

    let remove = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &fake_store)
        .args(["trust", "remove", "beta", "alpha", "--yes"])
        .output()
        .expect("run trust remove yes");
    assert!(
        remove.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&remove.stderr)
    );
    assert!(String::from_utf8(remove.stdout)
        .expect("remove stdout")
        .contains("removed trust relationship"));
    let body = std::fs::read_to_string(&fake_store).expect("fake store after remove");
    assert!(!body.contains("alpha"), "{body}");
    assert!(body.contains(peer_key), "{body}");
    assert!(root.join("home/.maw/state/trust.json").read_dir().is_err());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trust_native_refuses_auto_trust_before_store_or_secret_echo() {
    let root = trust_temp("auto");
    let missing_store = root.join("missing-parent/fake-trust.json");
    let output = trust_command(&root)
        .env("MAW_RS_TRUST_FAKE_STORE", &missing_store)
        .args([
            "trust",
            "add",
            "--auto-trust=fake-secret-token",
            "alpha",
            "beta",
        ])
        .output()
        .expect("run trust auto");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("no auto-trust"), "{stderr}");
    assert!(!stderr.contains("fake-secret-token"), "{stderr}");
    assert!(!root.join("missing-parent").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn trust_native_guard_rejects_unsafe_sender_without_secret_echo() {
    let root = trust_temp("guard");
    let output = trust_command(&root)
        .args(["trust", "add", "-secret-token", "beta"])
        .output()
        .expect("run trust guard");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("sender must not start"), "{stderr}");
    assert!(!stderr.contains("secret-token"), "{stderr}");
    let _ = std::fs::remove_dir_all(root);
}
