use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn absorb_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn absorb_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn absorb_chmod(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn absorb_seed(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-absorb-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    absorb_write(
        &config.join("fleet/01-donor.json"),
        r#"{"name":"01-donor-oracle","squadName":"donor-team","windows":[{"name":"donor-window","repo":"org/donor-oracle"}]}"#,
    );
    absorb_write(
        &config.join("fleet/02-receiver.json"),
        r#"{"name":"02-receiver-oracle","squadName":"receiver-team","windows":[{"name":"receiver-window","repo":"org/receiver-oracle"}]}"#,
    );
    std::fs::create_dir_all(root.join("ghq/github.com/org/donor-oracle/ψ/memory/learnings"))
        .expect("donor repo");
    std::fs::create_dir_all(root.join("ghq/github.com/org/receiver-oracle/ψ/memory/learnings"))
        .expect("receiver repo");
    absorb_write(
        &bin.join("tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$ABSORB_TMUX_LOG"
exit 0
"#,
    );
    absorb_chmod(&bin.join("tmux"));
    (root, home, config)
}

fn absorb_command(root: &Path, home: &Path, config: &Path) -> Command {
    let mut command = Command::new(absorb_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("ABSORB_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn absorb_native_dry_run_golden_is_hermetic_without_js_ref() {
    let (root, home, config) = absorb_seed("dry-run");
    let output = absorb_command(&root, &home, &config)
        .args(["absorb", "donor", "--into", "receiver", "--dry-run"])
        .output()
        .expect("run absorb");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = format!(
        "{}\n",
        include_str!("fixtures/native-absorb/dry-run.stdout")
            .replace("{ROOT}", &root.to_string_lossy())
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(dispatcher_status("absorb"), DispatchKind::Native);
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !log.contains("switch-client"),
        "dry-run switched tmux: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn absorb_native_blocks_leading_dash_targets_before_io() {
    let (root, home, config) = absorb_seed("guard");
    let guarded = absorb_command(&root, &home, &config)
        .args(["absorb", "-Sbad", "--into", "receiver", "--dry-run"])
        .output()
        .expect("run guard");
    assert!(!guarded.status.success());
    assert_eq!(
        String::from_utf8(guarded.stderr).expect("stderr"),
        "absorb: unknown argument -Sbad\n"
    );
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(!log.contains("-Sbad"), "guarded arg reached tmux: {log}");
    assert!(
        !log.contains("switch-client"),
        "guarded arg switched tmux: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}
