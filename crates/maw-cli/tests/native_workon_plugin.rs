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
    let path = std::env::temp_dir().join(format!("maw-rs-workon-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_exe(path: &Path, body: &str) {
    fs::write(path, body).expect("write exe");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn seed_hermetic_root(root: &Path, existing_windows: &str) -> PathBuf {
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_exe(
        &bin_dir.join("tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  display-message) printf '50-mawjs\n' ;;
  list-windows) printf '%s' "$MAW_FAKE_TMUX_WINDOWS" ;;
  has-session) exit 1 ;;
  new-session|new-window|send-keys|select-window) exit 0 ;;
  *) printf 'unexpected tmux %s\n' "$1" >&2; exit 9 ;;
esac
"#,
    );
    write_exe(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_GIT_LOG"
if [ "$3" = "branch" ]; then exit 1; fi
if [ "$3" = "for-each-ref" ]; then exit 0; fi
if [ "$3" = "rev-parse" ] && [ "$4" = "--show-toplevel" ]; then
  cd "$2" 2>/dev/null || exit 128
  pwd
  exit 0
fi
if [ "$3" = "worktree" ] && [ "$4" = "add" ]; then
  mkdir -p "$5"
  printf 'gitdir: fake\n' > "$5/.git"
  exit 0
fi
printf 'unexpected git args: %s\n' "$*" >&2
exit 9
"#,
    );

    let xdg_config = root.join("xdg-config");
    let ghq = root.join("ghq");
    let repo = ghq.join("github.com/acme/demo");
    fs::create_dir_all(&repo).expect("repo");
    fs::write(repo.join(".git"), "gitdir: main\n").expect("git marker");
    let config_dir = xdg_config.join("maw");
    fs::create_dir_all(&config_dir).expect("config dir");
    fs::write(
        config_dir.join("maw.config.json"),
        serde_json::json!({"commands":{"default":"echo launch"}}).to_string(),
    )
    .expect("config");
    fs::write(root.join("windows.txt"), existing_windows).expect("windows");
    bin_dir
}

fn run(root: &Path, bin_dir: &Path, args: &[&str]) -> std::process::Output {
    run_with_tmux_env(root, bin_dir, args, Some("/tmp/tmux-1000/default,123,0"))
}

fn run_with_tmux_env(
    root: &Path,
    bin_dir: &Path,
    args: &[&str],
    tmux_env: Option<&str>,
) -> std::process::Output {
    run_from(root, root, bin_dir, args, tmux_env)
}

fn run_from(
    cwd: &Path,
    root: &Path,
    bin_dir: &Path,
    args: &[&str],
    tmux_env: Option<&str>,
) -> std::process::Output {
    let mut command = Command::new(bin());
    command
        .args(args)
        .current_dir(cwd)
        .env_clear()
        .env("PATH", bin_dir)
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("MAW_TEST_MODE", "1")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env(
            "MAW_FAKE_TMUX_WINDOWS",
            fs::read_to_string(root.join("windows.txt")).expect("windows"),
        )
        .env("MAW_FAKE_GIT_LOG", root.join("git.log"));
    if let Some(value) = tmux_env {
        command.env("TMUX", value);
    }
    command.output().expect("run maw-rs")
}

fn normalize_root(text: &str, root: &Path) -> String {
    text.replace(&root.display().to_string(), "<ROOT>")
}

#[test]
fn native_workon_create_nested_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("create");
    let bin_dir = seed_hermetic_root(&root, "shell\n");

    let output = run(
        &root,
        &bin_dir,
        &["workon", "demo", "feat", "--layout", "nested"],
    );

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize_root(&String::from_utf8(output.stdout).expect("stdout"), &root),
        include_str!("fixtures/native-workon/create-nested.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        tmux_log.contains("new-window -P -F #{window_id} -t 50-mawjs: -n demo-feat -c"),
        "{tmux_log}"
    );
    assert!(
        tmux_log.contains("send-keys -t 50-mawjs:demo-feat -l echo launch"),
        "{tmux_log}"
    );
    assert!(
        tmux_log.contains("send-keys -t 50-mawjs:demo-feat Enter"),
        "{tmux_log}"
    );
    let git_log = fs::read_to_string(root.join("git.log")).expect("git log");
    assert!(git_log.contains("worktree add"), "{git_log}");
}

#[test]
fn native_workon_reuse_window_is_hermetic_and_does_not_spawn() {
    let root = temp_dir("reuse");
    let bin_dir = seed_hermetic_root(&root, "demo\n");

    let output = run(&root, &bin_dir, &["workon", "demo"]);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-workon/reuse-window.stdout")
    );
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        tmux_log.contains("select-window -t 50-mawjs:demo"),
        "{tmux_log}"
    );
    assert!(!tmux_log.contains("new-window"), "{tmux_log}");
}

#[test]
fn native_workon_outside_tmux_creates_session_and_prints_attach_plan() {
    let root = temp_dir("outside");
    let bin_dir = seed_hermetic_root(&root, "");

    let output = run_with_tmux_env(&root, &bin_dir, &["workon", "demo"], None);

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(
        stdout.contains("workon 'demo' in new session demo"),
        "{stdout}"
    );
    assert!(stdout.contains("run: tmux attach -t demo"), "{stdout}");
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(tmux_log.contains("has-session -t =demo"), "{tmux_log}");
    assert!(
        tmux_log.contains("new-session -d -s demo -c") && tmux_log.contains("-n demo"),
        "{tmux_log}"
    );
    assert!(
        tmux_log.contains("send-keys -t demo:demo -l echo launch"),
        "{tmux_log}"
    );
    assert!(
        tmux_log.contains("display-message -t demo:demo -p #{pane_in_mode}"),
        "{tmux_log}"
    );
    assert!(
        tmux_log.contains("capture-pane -t demo:demo -e -p -S -5"),
        "{tmux_log}"
    );
}

#[test]
fn native_workon_dot_resolves_current_repo() {
    let root = temp_dir("dot");
    let bin_dir = seed_hermetic_root(&root, "shell\n");
    let repo = root.join("ghq/github.com/acme/demo");

    let output = run_from(
        &repo,
        &root,
        &bin_dir,
        &["workon", "."],
        Some("/tmp/tmux-1000/default,123,0"),
    );

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("workon 'demo' in 50-mawjs"), "{stdout}");
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(
        tmux_log.contains("new-window -P -F #{window_id} -t 50-mawjs: -n demo -c"),
        "{tmux_log}"
    );
    let git_log = fs::read_to_string(root.join("git.log")).expect("git log");
    assert!(git_log.contains("rev-parse --show-toplevel"), "{git_log}");
}

#[test]
fn native_workon_registers_dispatcher_and_guards_layout() {
    assert_eq!(dispatcher_status("workon"), DispatchKind::Native);
    let root = temp_dir("layout");
    let bin_dir = seed_hermetic_root(&root, "");

    let output = run(&root, &bin_dir, &["workon", "demo", "--layout", "bad"]);

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("workon: --layout must be nested or legacy"));
}
