use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn more_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn more_run(args: &[&str]) -> Output {
    more_command(args).output().expect("run maw more")
}

fn more_run_with_fake_tmux(args: &[&str], fake_tmux: &Path) -> Output {
    more_command(args)
        .env("PATH", fake_tmux)
        .output()
        .expect("run maw more with fake tmux")
}

fn more_command(args: &[&str]) -> Command {
    let mut command = Command::new(more_bin());
    command
        .args(args)
        .env_clear()
        .env("CARGO_TERM_COLOR", "never")
        .env("MAW_JS_REF_DIR", "/nonexistent");
    command
}

fn more_stdout(output: &Output) -> String {
    String::from_utf8(output.stdout.clone()).expect("stdout utf8")
}

fn more_stderr(output: &Output) -> String {
    String::from_utf8(output.stderr.clone()).expect("stderr utf8")
}

fn assert_more_ok(args: &[&str]) -> String {
    let output = more_run(args);
    assert!(
        output.status.success(),
        "args={args:?}\nstderr={}",
        more_stderr(&output)
    );
    assert_eq!(more_stderr(&output), "");
    more_stdout(&output)
}

fn assert_more_ok_with_fake_tmux(args: &[&str]) -> String {
    let fake_tmux = more_fake_tmux_dir();
    let output = more_run_with_fake_tmux(args, &fake_tmux);
    assert!(
        output.status.success(),
        "args={args:?}\nstderr={}",
        more_stderr(&output)
    );
    assert_eq!(more_stderr(&output), "");
    more_stdout(&output)
}

fn assert_more_err(args: &[&str], expected: &str) {
    let output = more_run(args);
    assert!(!output.status.success(), "args={args:?}");
    assert_eq!(more_stdout(&output), "");
    let stderr = more_stderr(&output);
    assert!(
        stderr.contains(expected),
        "args={args:?}\nexpected={expected}\nstderr={stderr}"
    );
}

fn more_fake_tmux_dir() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("maw-more-tmux-{}-{unique}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir fake tmux dir");
    let tmux = dir.join("tmux");
    std::fs::write(
        &tmux,
        "#!/bin/sh\ncase \"$1\" in\n  display-message) printf 'team-a\\n' ;;\n  list-windows) printf 'team-a|||1|||team-a-codex-3|||0|||/repo/agents/team-a-codex-3\\n' ;;\n  list-panes) exit 0 ;;\n  *) echo unexpected tmux command: $1 >&2; exit 1 ;;\nesac\n",
    )
    .expect("write fake tmux");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&tmux).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&tmux, permissions).expect("chmod fake tmux");
    }
    dir
}

#[test]
fn more_is_native_and_status_is_discoverable() {
    assert_eq!(dispatcher_status("more"), DispatchKind::Native);

    let stdout = assert_more_ok(&["more", "status"]);
    assert!(stdout.contains("more status"), "{stdout}");
    assert!(stdout.contains("live coders:"), "{stdout}");
}

#[test]
fn more_codex_dry_run_defaults_count_and_engine() {
    let stdout = assert_more_ok_with_fake_tmux(&["more", "codex", "--dry-run"]);
    assert_eq!(
        stdout,
        "would spawn 1 coders in session team-a with engine codex\n"
    );
}

#[test]
fn more_codex_dry_run_accepts_count_session_and_engine() {
    let three = assert_more_ok_with_fake_tmux(&["more", "codex", "3", "--dry-run"]);
    assert_eq!(
        three,
        "would spawn 3 coders in session team-a with engine codex\n"
    );

    let custom = assert_more_ok_with_fake_tmux(&[
        "more",
        "codex",
        "2",
        "--session",
        "team-a",
        "--dry-run",
        "-e",
        "omx",
    ]);
    assert_eq!(
        custom,
        "would spawn 2 coders in session team-a with engine omx\n"
    );
}

#[test]
fn more_codex_rejects_invalid_counts() {
    assert_more_err(&["more", "codex", "0"], "N must be a positive integer");
    assert_more_err(&["more", "codex", "-1"], "N must be a positive integer");
    assert_more_err(&["more", "codex", "abc"], "N must be a positive integer");
}
