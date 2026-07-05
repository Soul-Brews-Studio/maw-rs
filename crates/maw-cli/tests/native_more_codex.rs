use maw_cli::{dispatcher_status, DispatchKind};
use std::path::PathBuf;
use std::process::{Command, Output};

fn more_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn more_run(args: &[&str]) -> Output {
    Command::new(more_bin())
        .args(args)
        .env_clear()
        .env("CARGO_TERM_COLOR", "never")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw more")
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

#[test]
fn more_is_native_and_status_is_discoverable() {
    assert_eq!(dispatcher_status("more"), DispatchKind::Native);

    let stdout = assert_more_ok(&["more", "status"]);
    assert!(stdout.contains("more status"), "{stdout}");
    assert!(stdout.contains("live coders:"), "{stdout}");
}

#[test]
fn more_codex_arg_parsing_defaults_count_and_engine() {
    let stdout = assert_more_ok(&["more", "codex"]);
    assert_eq!(
        stdout,
        "would spawn 1 coders in session current with engine codex\n"
    );
}

#[test]
fn more_codex_arg_parsing_accepts_count_dry_run_session_and_engine() {
    let three = assert_more_ok(&["more", "codex", "3"]);
    assert_eq!(
        three,
        "would spawn 3 coders in session current with engine codex\n"
    );

    let dry_run = assert_more_ok(&["more", "codex", "--dry-run"]);
    assert_eq!(
        dry_run,
        "would spawn 1 coders in session current with engine codex\n"
    );

    let custom = assert_more_ok(&["more", "codex", "2", "--session", "team-a", "-e", "omx"]);
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
