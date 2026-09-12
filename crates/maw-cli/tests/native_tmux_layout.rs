#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-{name}-{}-{stamp}", std::process::id()));
    fs::create_dir_all(&root).expect("temp root");
    root
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).expect("write script");
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).expect("chmod");
}

#[test]
fn tmux_layout_runtime_fake_maw_no_delegate_and_golden() {
    let temp = temp_root("tmux-layout");
    let bin = temp.join("bin");
    let home = temp.join("home");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(&home).expect("home");
    write_script(&bin.join("maw"), "#!/bin/sh\necho DELEGATED-MAW\nexit 77\n");
    write_script(&bin.join("bun"), "#!/bin/sh\necho DELEGATED-BUN\nexit 77\n");
    write_script(
        &bin.join("tmux"),
        "#!/bin/sh\nif [ \"$1\" = select-layout ] && [ \"$2\" = -t ] && [ \"$3\" = session:1 ] && [ \"$4\" = tiled ]; then exit 0; fi\necho BAD-TMUX:$* >&2\nexit 66\n",
    );

    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["tmux", "layout", "session:1.2", "tiled"])
        .env("PATH", &bin)
        .env("HOME", &home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(
        output.status.success(),
        "status={:?}\nstdout={stdout}\nstderr={stderr}",
        output.status
    );
    assert_eq!(
        stdout,
        include_str!("fixtures/native-tmux-layout/default.stdout")
    );
    assert!(
        !stdout.contains("DELEGATED-MAW") && !stderr.contains("DELEGATED-MAW"),
        "delegated maw\nstdout={stdout}\nstderr={stderr}"
    );
    assert!(
        !stdout.contains("DELEGATED-BUN") && !stderr.contains("DELEGATED-BUN"),
        "delegated bun\nstdout={stdout}\nstderr={stderr}"
    );
}

#[test]
fn tmux_layout_runtime_rejects_invalid_preset_before_tmux() {
    let temp = temp_root("tmux-layout-invalid");
    let bin = temp.join("bin");
    fs::create_dir_all(&bin).expect("bin");
    write_script(
        &bin.join("tmux"),
        "#!/bin/sh\necho SHOULD-NOT-RUN >&2\nexit 77\n",
    );
    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["tmux", "layout", "session:1.2", "bad"])
        .env("PATH", &bin)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs");
    let stdout = String::from_utf8(output.stdout).expect("stdout utf8");
    let stderr = String::from_utf8(output.stderr).expect("stderr utf8");
    assert!(!output.status.success(), "invalid preset should fail");
    assert!(stdout.is_empty(), "stdout={stdout}");
    assert!(stderr.contains("invalid layout"), "stderr={stderr}");
    assert!(
        !stderr.contains("SHOULD-NOT-RUN"),
        "tmux runner was invoked: {stderr}"
    );
}
