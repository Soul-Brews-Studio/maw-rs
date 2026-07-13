use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-incubate")
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-native-incubate-{name}-{stamp}"));
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

fn write_shell(path: &Path, body: &str) {
    fs::write(path, body).expect("write fake executable");
    chmod_exec(path);
}

fn write_fake_bud_tools(bin_dir: &Path) {
    for program in ["gh", "ghq", "git", "direnv"] {
        write_shell(
            &bin_dir.join(program),
            &format!(
                r#"#!/bin/sh
printf '{program} %s\n' "$*" >> "$MAW_INCUBATE_BUD_LOG"
exit 0
"#
            ),
        );
    }
}

fn write_fake_self(bin_dir: &Path) -> PathBuf {
    let fake_self = bin_dir.join("fake-self");
    write_shell(
        &fake_self,
        r#"#!/bin/sh
printf 'self MAW_FROM_RS=%s args=%s\n' "$MAW_FROM_RS" "$*" >> "$MAW_INCUBATE_SELF_LOG"
exit 0
"#,
    );
    fake_self
}

fn write_fake_tmux(bin_dir: &Path) {
    let tmux = bin_dir.join("tmux");
    write_shell(
        &tmux,
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_INCUBATE_TMUX_LOG"
case "$1" in
  list-windows)
    printf 'widgets|||0|||main|||1|||/tmp\n'
    ;;
  send-keys)
    exit 0
    ;;
  capture-pane)
    printf '\n'
    ;;
  *)
    exit 0
    ;;
esac
"#,
    );
}

fn seed_config(root: &Path) {
    let config = root.join("xdg-config").join("maw");
    fs::create_dir_all(&config).expect("config dir");
    fs::write(
        config.join("maw.config.json"),
        r#"{"node":"ci","oracle":"incubate-test"}"#,
    )
    .expect("seed config");
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_data = root.join("xdg-data");
    let xdg_state = root.join("xdg-state");
    let ghq_root = root.join("ghq");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&xdg_data).expect("xdg data");
    fs::create_dir_all(&xdg_state).expect("xdg state");
    fs::create_dir_all(&ghq_root).expect("ghq root");

    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("GHQ_ROOT", &ghq_root)
        .env("MAW_BUD_OWNER", "org")
        .env("CLAUDE_TOKEN_NAME", "duo")
        .env("TMUX", root.join("tmux-socket"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_SELF_BIN", bin_dir.join("fake-self"))
        .env("MAW_PLUGINS_DIR", plugin_dir())
        .env("MAW_INCUBATE_BUD_LOG", root.join("bud.log"))
        .env("MAW_INCUBATE_SELF_LOG", root.join("self.log"))
        .env("MAW_INCUBATE_TMUX_LOG", root.join("tmux.log"))
        .output()
        .expect("run maw-rs")
}

fn normalize_stdout(root: &Path, stdout: Vec<u8>) -> String {
    String::from_utf8(stdout)
        .expect("stdout")
        .replace(&root.display().to_string(), "{ROOT}")
}

#[test]
fn incubate_plugin_dry_run_matches_native_golden_and_is_hermetic() {
    let root = temp_dir("dry-run");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_bud_tools(&bin_dir);
    write_fake_self(&bin_dir);
    seed_config(&root);

    let output = run(&root, &["incubate", "org/foo", "--dry-run", "--flash"]);

    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize_stdout(&root, output.stdout),
        include_str!("fixtures/native-incubate/dry-run.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert!(
        !root.join("bud.log").exists(),
        "dry-run bud must not invoke gh/ghq/git"
    );
    assert!(
        !root.join("self.log").exists(),
        "dry-run bud must not wake via self-bin"
    );
    assert!(
        !root.join("tmux.log").exists(),
        "dry-run must not touch tmux"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn incubate_plugin_preserves_bud_subdispatch_tmux_send_and_guards() {
    let root = temp_dir("send");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_bud_tools(&bin_dir);
    write_fake_self(&bin_dir);
    write_fake_tmux(&bin_dir);
    seed_config(&root);

    assert_eq!(dispatcher_status("incubate"), DispatchKind::NativeError);

    let output = run(&root, &["incubate", "org/widgets", "--contribute"]);
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        normalize_stdout(&root, output.stdout),
        include_str!("fixtures/native-incubate/send.stdout")
    );
    let bud_log = fs::read_to_string(root.join("bud.log")).expect("bud log");
    assert!(bud_log.contains("gh repo view org/widgets-oracle --json name"));
    assert!(bud_log.contains("ghq get github.com/org/widgets-oracle"));
    assert!(bud_log.contains("git -C "));
    assert!(bud_log.contains("direnv allow "));
    assert!(
        !bud_log.contains(" maw ") && !bud_log.starts_with("maw "),
        "incubate must not shell PATH maw; bud_log={bud_log}"
    );
    let self_log = fs::read_to_string(root.join("self.log")).expect("self log");
    assert!(self_log.contains("self MAW_FROM_RS=1 args=wake widgets --no-attach --repo-path"));
    let tmux_log = fs::read_to_string(root.join("tmux.log")).expect("tmux log");
    assert!(tmux_log.contains("list-windows -a -F #{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}"));
    assert!(tmux_log.contains("send-keys -t widgets:0 -l /incubate org/widgets --contribute"));

    let bad = run(&root, &["incubate", "org/widgets", "--stem", "-bad"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8(bad.stderr)
        .expect("stderr")
        .contains("--stem requires a value"));
    fs::remove_dir_all(root).expect("cleanup");
}
