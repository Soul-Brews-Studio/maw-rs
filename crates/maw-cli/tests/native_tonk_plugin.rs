#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
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
    let path = std::env::temp_dir().join(format!("maw-rs-tonk-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], maw_home: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .env("MAW_HOME", maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TONK_FAKE_GH", "1")
        .output()
        .expect("run maw-rs")
}

fn assert_stdout_golden(name: &str, args: &[&str], expected: &str) {
    let root = temp_dir(name);
    let output = run(args, &root);
    assert!(
        output.status.success(),
        "stderr={} stdout={}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn tonk_committed_golden_without_js_ref_and_fake_gh() {
    assert_stdout_golden(
        "help",
        &["tonk", "help"],
        include_str!("fixtures/native-tonk/help.stdout"),
    );
    assert_stdout_golden(
        "say",
        &["tonk", "say", "TK"],
        include_str!("fixtures/native-tonk/say.stdout"),
    );
    assert_stdout_golden(
        "status",
        &["tonk", "status"],
        include_str!("fixtures/native-tonk/status.stdout"),
    );
    assert_stdout_golden(
        "gh-whoami",
        &["tonk", "gh", "whoami"],
        include_str!("fixtures/native-tonk/gh-whoami.stdout"),
    );
    assert_stdout_golden(
        "gh-read",
        &["tonk", "gh", "discuss", "read", "tonkmac/maw-rs", "7"],
        include_str!("fixtures/native-tonk/gh-read.stdout"),
    );
    assert_stdout_golden(
        "gh-create",
        &[
            "tonk",
            "gh",
            "discuss",
            "create",
            "tonkmac/maw-rs",
            "--title",
            "Hello",
            "--category",
            "Workshop",
            "--text",
            "Body",
        ],
        include_str!("fixtures/native-tonk/gh-create.stdout"),
    );
    assert_stdout_golden(
        "gh-post",
        &[
            "tonk",
            "gh",
            "discuss",
            "post",
            "tonkmac/maw-rs",
            "7",
            "--text",
            "Body",
        ],
        include_str!("fixtures/native-tonk/gh-post.stdout"),
    );
    assert_stdout_golden(
        "gh-reply",
        &[
            "tonk",
            "gh",
            "discuss",
            "reply",
            "tonkmac/maw-rs",
            "7",
            "COMMENT_1",
            "--text",
            "Body",
        ],
        include_str!("fixtures/native-tonk/gh-reply.stdout"),
    );
}
