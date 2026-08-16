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
    let root = std::env::temp_dir().join(format!("maw-rs-tmux-close-{name}-{stamp}"));
    fs::create_dir_all(root.join("fakebin")).expect("fakebin");
    root
}

fn seed_fake_bins(root: &Path) {
    fs::write(
        root.join("fakebin/tmux"),
        r#"#!/usr/bin/env bash
printf '%s\n' "$*" >> "$TMUX_CLOSE_CALLS"
case "$1" in
  break-pane)
    test "$2" = "-d" || exit 21
    test "$3" = "-s" || exit 22
    test "$4" = "%42" || exit 23
    exit 0
    ;;
  list-panes)
    printf '%%1\n%%42\n'
    exit 0
    ;;
  list-sessions)
    printf 'demo\n'
    exit 0
    ;;
  *)
    echo "unexpected tmux $*" >&2
    exit 24
    ;;
esac
"#,
    )
    .expect("fake tmux");
    fs::write(
        root.join("fakebin/maw"),
        "#!/usr/bin/env bash\necho DELEGATED-MAW\n",
    )
    .expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(root.join("fakebin/tmux"), fs::Permissions::from_mode(0o755))
            .expect("chmod fake tmux");
        fs::set_permissions(root.join("fakebin/maw"), fs::Permissions::from_mode(0o755))
            .expect("chmod fake maw");
    }
}

#[test]
fn tmux_close_golden_parity_and_fake_maw_no_delegate() {
    let root = temp_dir("golden");
    seed_fake_bins(&root);
    let calls = root.join("tmux.calls");
    let path = format!(
        "{}:{}",
        root.join("fakebin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(bin())
        .args(["tmux", "close", "%42"])
        .current_dir(&root)
        .env("PATH", path)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("TMUX", "/tmp/tmux-1000/default,1,0")
        .env("TMUX_PANE", "%1")
        .env("TMUX_CLOSE_CALLS", &calls)
        .output()
        .expect("run maw-rs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert_eq!(
        stdout,
        include_str!("fixtures/native-tmux-close/tmux-close.stdout")
    );
    assert!(!stdout.contains("DELEGATED-MAW"), "fake maw was delegated");
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(dispatcher_status("tmux"), DispatchKind::Native);
    let tmux_calls = fs::read_to_string(calls).expect("tmux calls");
    assert!(
        tmux_calls.contains("break-pane -d -s %42\n"),
        "tmux calls: {tmux_calls}"
    );
    assert!(
        !tmux_calls.contains("DELEGATED-MAW"),
        "fake maw delegated through tmux calls"
    );
}

#[test]
fn tmux_close_input_guard_before_tmux_spawn() {
    let root = temp_dir("guard");
    seed_fake_bins(&root);
    let calls = root.join("tmux.calls");
    let path = format!(
        "{}:{}",
        root.join("fakebin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(bin())
        .args(["tmux", "close", "-bad"])
        .current_dir(&root)
        .env("PATH", path)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("TMUX", "/tmp/tmux-1000/default,1,0")
        .env("TMUX_PANE", "%1")
        .env("TMUX_CLOSE_CALLS", &calls)
        .output()
        .expect("run maw-rs");

    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("unknown argument -bad"));
    let tmux_calls = fs::read_to_string(calls).unwrap_or_default();
    assert!(
        !tmux_calls.contains("break-pane"),
        "guarded input reached mutating tmux call: {tmux_calls}"
    );
}
