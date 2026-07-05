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
  new-window|send-keys|select-window) exit 0 ;;
  *) printf 'unexpected tmux %s\n' "$1" >&2; exit 9 ;;
esac
"#,
    );
    write_exe(
        &bin_dir.join("git"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_GIT_LOG"
if [ "$3" = "branch" ]; then exit 1; fi
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
    Command::new(bin())
        .args(args)
        .current_dir(root)
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
        .env("TMUX", "/tmp/tmux-1000/default,123,0")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env(
            "MAW_FAKE_TMUX_WINDOWS",
            fs::read_to_string(root.join("windows.txt")).expect("windows"),
        )
        .env("MAW_FAKE_GIT_LOG", root.join("git.log"))
        .output()
        .expect("run maw-rs")
}

fn normalize_root(text: &str, root: &Path) -> String {
    let mut candidates = vec![root.display().to_string()];
    if let Ok(canonical) = root.canonicalize() {
        candidates.push(canonical.display().to_string());
    }
    if let Some(path) = root.to_str() {
        if path.starts_with("/var/") {
            candidates.push(format!("/private{path}"));
        } else if let Some(stripped) = path.strip_prefix("/private") {
            candidates.push(stripped.to_owned());
        }
    }
    candidates.sort_by_key(|path| std::cmp::Reverse(path.len()));
    candidates.dedup();
    let mut output = text.to_owned();
    for candidate in candidates {
        output = output.replace(&candidate, "<ROOT>");
    }
    output
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
        tmux_log.contains("new-window -t 50-mawjs -n demo-feat -c"),
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
