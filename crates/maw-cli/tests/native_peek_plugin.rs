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
    let path = std::env::temp_dir().join(format!("maw-rs-peek-{name}-{stamp}"));
    fs::create_dir_all(path.join("bin")).expect("temp bin");
    path
}

fn chmod(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }
}

fn write_fake_maw(bin_dir: &Path) {
    let maw = bin_dir.join("maw");
    fs::write(&maw, "#!/bin/sh\necho DELEGATED-MAW \"$@\"\nexit 99\n").expect("fake maw");
    chmod(&maw);
}

fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    fs::write(
        &tmux,
        r#"#!/bin/sh
cmd="$1"
shift
case "$cmd" in
  list-sessions)
    printf 'sess\n'
    ;;
  list-windows)
    case "$*" in
      *'|||'*)
        printf 'sess|||0|||active|||1|||/tmp\n'
        printf 'sess|||1|||blank|||0|||/tmp\n'
        ;;
      *)
        printf 'sess\t0\tactive\t1\n'
        printf 'sess\t1\tblank\t0\n'
        ;;
    esac
    ;;
  capture-pane)
    target=""
    while [ "$#" -gt 0 ]; do
      if [ "$1" = "-t" ]; then shift; target="$1"; fi
      shift || true
    done
    case "$target" in
      sess:1.0) printf 'native peek body\n' ;;
      sess:0) printf 'older\nlast line\n' ;;
      sess:1) printf '\n  \n' ;;
      *) printf 'missing target %s\n' "$target" >&2; exit 1 ;;
    esac
    ;;
  *) printf 'unexpected tmux %s\n' "$cmd" >&2; exit 2 ;;
esac
"#,
    )
    .expect("fake tmux");
    chmod(&tmux);
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let bin_dir = root.join("bin");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);
    Command::new(bin())
        .args(args)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

#[test]
fn peek_native_fake_maw_proof_single_and_overview_goldens() {
    assert_eq!(dispatcher_status("peek"), DispatchKind::Native);
    let root = temp_dir("proof");
    for (name, args, expected) in [
        (
            "single",
            ["peek", "sess:1.0"].as_slice(),
            include_str!("fixtures/native-peek/single.stdout"),
        ),
        (
            "overview",
            ["peek"].as_slice(),
            include_str!("fixtures/native-peek/overview.stdout"),
        ),
    ] {
        let output = run(&root, args);
        assert!(
            output.status.success(),
            "{name}: stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout");
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        assert_eq!(stdout, expected, "{name}");
        assert!(
            !stdout.contains("DELEGATED-MAW"),
            "{name}: stdout delegated"
        );
        assert!(
            !stderr.contains("DELEGATED-MAW"),
            "{name}: stderr delegated"
        );
        assert_eq!(stderr, "", "{name}");
    }
}

#[test]
fn peek_native_rejects_injection_before_fake_tmux_or_maw() {
    assert_eq!(dispatcher_status("peek"), DispatchKind::Native);
    let root = temp_dir("reject");
    let output = run(&root, &["peek", "--", "-bad"]);
    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(stderr, include_str!("fixtures/native-peek/reject.stderr"));
    assert!(!stderr.contains("DELEGATED-MAW"));
}
