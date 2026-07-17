#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-project-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn chmod_exec(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn write_fake_maw(bin_dir: &Path) {
    let maw = bin_dir.join("maw");
    fs::write(
        &maw,
        "#!/bin/sh\nprintf 'DELEGATED-MAW %s\\n' \"$*\"\nprintf 'bun\\n'\nexit 42\n",
    )
    .expect("write fake maw");
    chmod_exec(&maw);
}

fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    fs::write(
        &tmux,
        "#!/bin/sh\nprintf 'DELEGATED-MAW-TMUX %s\\n' \"$*\"\nexit 1\n",
    )
    .expect("write fake tmux");
    chmod_exec(&tmux);
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_data = root.join("xdg-data");
    let xdg_state = root.join("xdg-state");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&xdg_data).expect("xdg data");
    fs::create_dir_all(&xdg_state).expect("xdg state");

    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

/// Golden strings from the wasm-parity `project` fixture goldens (extracted in
/// the repo split to Soul-Brews-Studio/maw-fixtures @aecf20b6, under
/// crates/maw-plugin-manifest/tests/fixtures/wasm-parity/project/)
const GOLDEN_NO_ARGS_STDOUT: &str = "usage: maw project <learn|incubate|find|list> [args...]\n  \
     learn    <url>   \u{2014} clone repo for study (symlink in \u{03c8}/learn/)\n  \
     incubate <url>   \u{2014} clone repo for development (symlink in \u{03c8}/incubate/)\n  \
     find     <query> \u{2014} search tracked repos (alias: search)\n  \
     list             \u{2014} list all tracked repos\n\n\
     see Oracle skill /project for the full implementation \
     (scaffold tracks https://github.com/Soul-Brews-Studio/maw-js/issues/523).";

const GOLDEN_LIST_STDOUT: &str =
    "project list: would list all tracked repos from \u{03c8}/learn and \u{03c8}/incubate \
     \u{2014} not yet implemented in core plugin; use Oracle skill /project for full behavior.\n  \
     track: https://github.com/Soul-Brews-Studio/maw-js/issues/523";

const GOLDEN_LEARN_STDOUT: &str =
    "project learn: would clone \"https://github.com/Soul-Brews-Studio/maw-js\" \
     and symlink into \u{03c8}/learn/<owner>/<repo> \u{2014} not yet implemented in core plugin; \
     use Oracle skill /project for full behavior.\n  \
     track: https://github.com/Soul-Brews-Studio/maw-js/issues/523";

const GOLDEN_INCUBATE_STDOUT: &str =
    "project incubate: would clone \"https://github.com/Soul-Brews-Studio/maw-rs\" \
     and symlink into \u{03c8}/incubate/<owner>/<repo> \u{2014} not yet implemented in core plugin; \
     use Oracle skill /project for full behavior.\n  \
     track: https://github.com/Soul-Brews-Studio/maw-js/issues/523";

const GOLDEN_FIND_STDOUT: &str =
    "project find: would search tracked repos for \"oracle\" across \u{03c8}/learn and \
     \u{03c8}/incubate \u{2014} not yet implemented in core plugin; use Oracle skill /project for full behavior.\n  \
     track: https://github.com/Soul-Brews-Studio/maw-js/issues/523";

const GOLDEN_LEARN_MISSING_STDERR: &str = "usage: maw project learn <url>";

const GOLDEN_BOGUS_STDERR: &str =
    "maw project: unknown subcommand \"bogus\" (expected learn|incubate|find|list)";

#[test]
fn native_project_no_args_exit0_matches_golden() {
    let root = temp_dir("no-args");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(&root, &["project"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, GOLDEN_NO_ARGS_STDOUT, "stdout mismatch");
    assert_eq!(stderr, "", "stderr mismatch");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_list_exit0_matches_golden() {
    let root = temp_dir("list");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(&root, &["project", "list"]);
    assert!(
        out.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, GOLDEN_LIST_STDOUT);
    assert_eq!(stderr, "");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_learn_with_url_matches_golden() {
    let root = temp_dir("learn-url");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(
        &root,
        &[
            "project",
            "learn",
            "https://github.com/Soul-Brews-Studio/maw-js",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, GOLDEN_LEARN_STDOUT);
    assert_eq!(stderr, "");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_incubate_with_url_matches_golden() {
    let root = temp_dir("incubate-url");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(
        &root,
        &[
            "project",
            "incubate",
            "https://github.com/Soul-Brews-Studio/maw-rs",
        ],
    );
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, GOLDEN_INCUBATE_STDOUT);
    assert_eq!(stderr, "");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_find_with_query_matches_golden() {
    let root = temp_dir("find-oracle");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(&root, &["project", "find", "oracle"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, GOLDEN_FIND_STDOUT);
    assert_eq!(stderr, "");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_learn_no_url_exit1_matches_golden() {
    let root = temp_dir("learn-no-url");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(&root, &["project", "learn"]);
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, "", "stdout should be empty");
    assert_eq!(stderr, GOLDEN_LEARN_MISSING_STDERR);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_bogus_exit1_both_streams_match_golden() {
    let root = temp_dir("bogus");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(&root, &["project", "bogus"]);
    assert!(!out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    let stderr = String::from_utf8(out.stderr).expect("stderr");
    assert_eq!(stdout, GOLDEN_NO_ARGS_STDOUT, "stdout should be help text");
    assert_eq!(stderr, GOLDEN_BOGUS_STDERR);
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_dash_flag_filter_proves_dashes_dropped() {
    // --foo list → --foo is filtered → effective sub = list
    let root = temp_dir("dash-filter");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    let out = run(&root, &["project", "--foo", "list"]);
    assert!(out.status.success());
    let stdout = String::from_utf8(out.stdout).expect("stdout");
    assert!(stdout.contains("project list:"), "stdout={stdout}");
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_no_delegated_maw_no_bun() {
    let root = temp_dir("zero-bun");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_maw(&bin_dir);
    write_fake_tmux(&bin_dir);

    for args in [
        vec!["project"],
        vec!["project", "list"],
        vec!["project", "bogus"],
    ] {
        let out = run(&root, &args);
        let combined = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(
            !combined.contains("DELEGATED-MAW"),
            "args={args:?} combined={combined}"
        );
        assert!(
            !combined.contains("bun"),
            "args={args:?} combined={combined}"
        );
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_project_dispatcher_status_is_native() {
    assert_eq!(dispatcher_status("project"), DispatchKind::Native);
}
