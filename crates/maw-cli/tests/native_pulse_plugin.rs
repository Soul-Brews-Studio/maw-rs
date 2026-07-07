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
    let path = std::env::temp_dir().join(format!("maw-rs-native-pulse-{name}-{stamp}"));
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

fn write_fake_gh(bin_dir: &Path) {
    let gh = bin_dir.join("gh");
    fs::write(
        &gh,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_PULSE_GH_LOG"
if [ "$1 $2" = 'issue list' ]; then
  printf '%s\n' '[{"number":20,"title":"📅 2026-06-25 Daily Thread","labels":[{"name":"daily-thread"}]},{"number":21,"title":"P001 launch board","labels":[{"name":"oracle:nova"}]},{"number":19,"title":"registry cleanup","labels":[]},{"number":22,"title":"ship pulse native","labels":[{"name":"oracle:pulse"}]}]'
  exit 0
fi
printf 'unexpected gh: %s\n' "$*" >&2
exit 42
"#,
    )
    .expect("write fake gh");
    chmod_exec(&gh);
}

fn write_fake_git(bin_dir: &Path) {
    let git = bin_dir.join("git");
    fs::write(
        &git,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_PULSE_GIT_LOG"
case "$*" in
  *'rev-parse --abbrev-ref HEAD') printf 'agents/1-old\n'; exit 0 ;;
  *'worktree list --porcelain') exit 0 ;;
esac
printf 'unexpected git: %s\n' "$*" >&2
exit 42
"#,
    )
    .expect("write fake git");
    chmod_exec(&git);
}

fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    fs::write(
        &tmux,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_PULSE_TMUX_LOG"
if [ "$*" = 'list-windows -a -F #W' ]; then
  printf '1-active\n'
  exit 0
fi
printf 'unexpected tmux: %s\n' "$*" >&2
exit 42
"#,
    )
    .expect("write fake tmux");
    chmod_exec(&tmux);
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    run_with_pulse_repo(root, args, None)
}

fn run_with_pulse_repo(
    root: &Path,
    args: &[&str],
    pulse_repo: Option<&str>,
) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_data = root.join("xdg-data");
    let xdg_state = root.join("xdg-state");
    let ghq = root.join("ghq");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(xdg_config.join("maw")).expect("xdg config");
    fs::write(
        xdg_config.join("maw/maw.config.json"),
        r#"{"node":"ci","oracle":"pulse-test"}"#,
    )
    .expect("seed config");
    fs::create_dir_all(&xdg_data).expect("xdg data");
    fs::create_dir_all(&xdg_state).expect("xdg state");
    fs::create_dir_all(&ghq).expect("ghq");

    let mut command = Command::new(bin());
    command
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("GHQ_ROOT", &ghq)
        .env("TMUX", root.join("tmux-socket"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PULSE_GH_LOG", root.join("gh.log"))
        .env("MAW_PULSE_GIT_LOG", root.join("git.log"))
        .env("MAW_PULSE_TMUX_LOG", root.join("tmux.log"));
    if let Some(repo) = pulse_repo {
        command.env("MAW_PULSE_REPO", repo);
    }
    command.output().expect("run maw-rs")
}

#[test]
fn native_pulse_list_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("list");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_gh(&bin_dir);

    let output = run(&root, &["pulse", "list"]);

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!("{}\n", include_str!("fixtures/native-pulse/list.stdout"))
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("gh.log")).expect("gh log"),
        "issue list --repo laris-co/pulse-oracle --state open --json number,title,labels --limit 50\n"
    );
}

#[test]
fn native_pulse_list_honors_maw_pulse_repo_override() {
    let root = temp_dir("list-override");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_gh(&bin_dir);

    let output = run_with_pulse_repo(&root, &["pulse", "list"], Some("acme/pulse-board"));

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("gh.log")).expect("gh log"),
        "issue list --repo acme/pulse-board --state open --json number,title,labels --limit 50\n"
    );
}

#[test]
fn native_pulse_cleanup_dry_run_is_hermetic_and_matches_golden() {
    let root = temp_dir("cleanup");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_git(&bin_dir);
    write_fake_tmux(&bin_dir);
    let worktree = root.join("ghq/github.com/acme/widgets/agents/1-old");
    fs::create_dir_all(&worktree).expect("worktree");
    fs::write(
        worktree.join(".git"),
        "gitdir: ../../../.git/worktrees/1-old\n",
    )
    .expect("git marker");

    let output = run(&root, &["pulse", "cleanup", "--dry-run"]);

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        format!(
            "{}\n",
            include_str!("fixtures/native-pulse/cleanup-dry-run.stdout")
        )
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("tmux.log")).expect("tmux log"),
        "list-sessions -F #{session_name}\nlist-windows -a -F #W\n"
    );
    assert!(fs::read_to_string(root.join("git.log"))
        .expect("git log")
        .contains("rev-parse --abbrev-ref HEAD"));
}

#[test]
fn native_dispatcher_registers_pulse_plugin() {
    assert_eq!(dispatcher_status("pulse"), DispatchKind::Native);
}
