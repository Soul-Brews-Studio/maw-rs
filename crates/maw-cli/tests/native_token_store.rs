use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

const SECRET_VALUE: &str = "super-secret-token-value";

fn epic55_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn epic55_temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-epic55-token-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn epic55_seed_fake_pass(root: &Path) {
    let pass = root.join("fake/pass");
    fs::create_dir_all(pass.join("claude")).expect("claude pass");
    fs::create_dir_all(pass.join("envrc")).expect("envrc pass");
    fs::write(pass.join("claude/token-nova"), "nova-token-value\n").expect("nova token");
    fs::write(pass.join("claude/token-pym"), format!("{SECRET_VALUE}\n")).expect("pym token");
    fs::write(
        pass.join("envrc/demo"),
        "export CLAUDE_TOKEN_NAME=\"nova\"\nexport CLAUDE_CODE_OAUTH_TOKEN=\"$(pass show claude/token-nova)\"\n",
    )
    .expect("demo envrc");
}

fn epic55_run(root: &Path, cwd: &Path, args: &[&str]) -> Output {
    Command::new(epic55_bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TOKEN_FAKE_ROOT", root.join("fake"))
        .env("HOME", root.join("home"))
        .output()
        .expect("run maw-rs")
}

#[test]
fn epic55_token_list_matches_committed_golden_names_only() {
    let root = epic55_temp_dir("list");
    epic55_seed_fake_pass(&root);
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        cwd.join(".envrc"),
        "export CLAUDE_TOKEN_NAME=\"nova\"\nexport CLAUDE_CODE_OAUTH_TOKEN=\"$(pass show claude/token-nova)\"\n",
    )
    .expect("envrc");

    let output = epic55_run(&root, &cwd, &["token", "list"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert_eq!(stdout, include_str!("fixtures/epic55/token-list.stdout"));
    assert!(!stdout.contains(SECRET_VALUE));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn epic55_token_use_writes_reference_atomically_without_value() {
    let root = epic55_temp_dir("use");
    epic55_seed_fake_pass(&root);
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        cwd.join(".envrc"),
        "export KEEP_ME=1\nexport CLAUDE_CODE_OAUTH_TOKEN=old-literal\n",
    )
    .expect("old envrc");

    let output = epic55_run(&root, &cwd, &["token", "use", "pym", "--no-team"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/epic55/token-use.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    let envrc = fs::read_to_string(cwd.join(".envrc")).expect("envrc");
    assert!(envrc.contains("export KEEP_ME=1"));
    assert!(envrc.contains("export CLAUDE_TOKEN_NAME=\"pym\""));
    assert!(envrc.contains("export CLAUDE_CODE_OAUTH_TOKEN=\"$(pass show claude/token-pym)\""));
    assert!(!envrc.contains(SECRET_VALUE));
    assert!(!envrc.contains("CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS"));
}

#[test]
fn epic55_token_save_transits_envrc_via_pass_stdin_and_hides_pass_failures() {
    let root = epic55_temp_dir("save");
    epic55_seed_fake_pass(&root);
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        cwd.join(".envrc"),
        format!("export CLAUDE_CODE_OAUTH_TOKEN={SECRET_VALUE}\n"),
    )
    .expect("envrc");

    let saved = epic55_run(&root, &cwd, &["token", "save", "repo", "--force"]);
    assert!(
        saved.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&saved.stderr)
    );
    let stored = fs::read_to_string(root.join("fake/pass/envrc/repo")).expect("saved envrc");
    assert!(
        stored.contains(SECRET_VALUE),
        "fake pass receives stdin payload"
    );

    let failed = Command::new(epic55_bin())
        .args(["token", "save", "repo", "--force"])
        .current_dir(&cwd)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TOKEN_FAKE_ROOT", root.join("fake"))
        .env("MAW_RS_TOKEN_FAKE_FAIL", "insert")
        .output()
        .expect("run failed save");
    assert!(!failed.status.success());
    let stderr = String::from_utf8(failed.stderr).expect("stderr");
    assert!(stderr.contains("pass insert failed"));
    assert!(!stderr.contains(SECRET_VALUE));
}

#[test]
fn epic55_token_load_requires_force_for_non_tty_overwrite_and_rejects_literal_secret_envrc() {
    let root = epic55_temp_dir("load");
    epic55_seed_fake_pass(&root);
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(cwd.join(".envrc"), "export KEEP=1\n").expect("existing envrc");

    let skipped = epic55_run(&root, &cwd, &["token", "load", "demo"]);
    assert!(
        skipped.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&skipped.stderr)
    );
    assert_eq!(
        String::from_utf8(skipped.stdout).expect("stdout"),
        "Skipped (would overwrite .envrc; envrc/demo)\n"
    );
    assert_eq!(
        fs::read_to_string(cwd.join(".envrc")).expect("envrc"),
        "export KEEP=1\n"
    );

    let loaded = epic55_run(&root, &cwd, &["token", "load", "demo", "--force"]);
    assert!(
        loaded.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&loaded.stderr)
    );
    let envrc = fs::read_to_string(cwd.join(".envrc")).expect("envrc");
    assert!(envrc.contains("$(pass show claude/token-nova)"));
    assert!(!envrc.contains(SECRET_VALUE));

    fs::write(
        root.join("fake/pass/envrc/bad"),
        format!("export CLAUDE_CODE_OAUTH_TOKEN={SECRET_VALUE}\n"),
    )
    .expect("bad envrc");
    let rejected = epic55_run(&root, &cwd, &["token", "load", "bad", "--force"]);
    assert!(!rejected.status.success());
    let stderr = String::from_utf8(rejected.stderr).expect("stderr");
    assert!(stderr.contains("refusing to write"));
    assert!(!stderr.contains(SECRET_VALUE));
}

#[test]
fn epic55_token_scan_outputs_names_only_and_guards_bad_names_before_io() {
    let root = epic55_temp_dir("scan");
    epic55_seed_fake_pass(&root);
    let cwd = root.join("repo");
    let ghq_repo = root.join("ghq/github.com/acme/demo");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::create_dir_all(&ghq_repo).expect("ghq repo");
    fs::write(
        ghq_repo.join(".envrc"),
        format!("export CLAUDE_CODE_OAUTH_TOKEN={SECRET_VALUE}\n"),
    )
    .expect("repo envrc");

    let output = Command::new(epic55_bin())
        .args(["token", "scan"])
        .current_dir(&cwd)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_TOKEN_FAKE_ROOT", root.join("fake"))
        .env("GHQ_ROOT", root.join("ghq"))
        .env("HOME", root.join("home"))
        .output()
        .expect("scan");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("pym (1 repos)"));
    assert!(stdout.contains("acme/demo"));
    assert!(!stdout.contains(SECRET_VALUE));

    let guarded = epic55_run(&root, &cwd, &["token", "use", "../bad"]);
    assert!(!guarded.status.success());
    assert!(String::from_utf8(guarded.stderr)
        .expect("stderr")
        .contains("invalid argument value"));
}

#[test]
fn epic55_token_dispatch_is_final_native_slice() {
    assert_eq!(
        maw_cli::dispatcher_status("token"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("tokens"),
        maw_cli::DispatchKind::Native
    );
}
