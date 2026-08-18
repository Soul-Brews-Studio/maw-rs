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
    let path = std::env::temp_dir().join(format!("maw-rs-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn install_empty_tmux(root: &Path) -> PathBuf {
    let bin = root.join("fakebin");
    fs::create_dir_all(&bin).expect("fake bin");
    let tmux = bin.join("tmux");
    fs::write(
        &tmux,
        include_str!("fixtures/hermetic-tmux/empty-server.sh"),
    )
    .expect("fake tmux");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmux, fs::Permissions::from_mode(0o755)).expect("chmod fake tmux");
    }
    bin
}

fn path_with(bin: PathBuf) -> std::ffi::OsString {
    let mut entries = vec![bin];
    entries.extend(
        std::env::var_os("PATH")
            .into_iter()
            .flat_map(|value| std::env::split_paths(&value).collect::<Vec<_>>()),
    );
    std::env::join_paths(entries).expect("fake PATH")
}

fn run(args: &[&str], cwd: &Path, maw_home: &Path, ghq_root: &Path) -> std::process::Output {
    let fake_bin = install_empty_tmux(maw_home);
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("MAW_HOME", maw_home)
        .env("GHQ_ROOT", ghq_root)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("PATH", path_with(fake_bin))
        .output()
        .expect("run maw-rs")
}

#[test]
fn native_find_oracle_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("find-oracle");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let repo = ghq.join("github.com/acme/beta");
    fs::create_dir_all(&repo).expect("seed repo");

    let output = run(&["find", "beta"], &repo, &maw_home, &ghq);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/find-oracle.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_scope_list_empty_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("scope-list");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");

    let output = run(&["scope", "list"], &cwd, &maw_home, &ghq);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/scope-list-empty.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_token_current_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("token-current");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(cwd.join(".envrc"), "export CLAUDE_TOKEN_NAME=\"nova\"\n").expect("envrc");

    let output = run(&["token", "current"], &cwd, &maw_home, &ghq);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/token-current.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_about_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("about-beta");
    let cwd = root.join("cwd");
    fs::create_dir_all(&cwd).expect("cwd");
    let repo = cwd.join("ghq/github.com/acme/beta");
    let worktree = repo.join("agents/alpha-task");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::write(repo.join(".git"), "gitdir: .git/worktrees/main\n").expect("repo git marker");
    fs::write(
        worktree.join(".git"),
        "gitdir: ../../.git/worktrees/alpha-task\n",
    )
    .expect("worktree git marker");

    let fake_bin = install_empty_tmux(&root);
    let output = Command::new(bin())
        .args(["about", "beta"])
        .current_dir(&cwd)
        .env("MAW_HOME", "maw-home")
        .env("GHQ_ROOT", "ghq")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("PATH", path_with(fake_bin))
        .output()
        .expect("run maw-rs about");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/about-beta.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_overview_kill_matches_committed_maw_js_golden_without_ref_checkout() {
    let root = temp_dir("overview-kill");
    let maw_home = root.join("home");
    let ghq = root.join("ghq");
    let cwd = root.join("repo");
    fs::create_dir_all(&cwd).expect("cwd");

    let output = run(&["overview", "--color", "--kill"], &cwd, &maw_home, &ghq);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-orchestration/overview-kill.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_dispatcher_registers_orchestration_plugins() {
    assert_eq!(
        maw_cli::dispatcher_status("scope"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("find"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("token"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("about"),
        maw_cli::DispatchKind::Native
    );
    assert_eq!(
        maw_cli::dispatcher_status("overview"),
        maw_cli::DispatchKind::Native
    );
}
