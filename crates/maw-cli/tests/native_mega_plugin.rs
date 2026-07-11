use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn maw_rs_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn plugin_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-mega")
}

fn write_file(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn seed_mega_env(name: &str) -> (PathBuf, PathBuf, PathBuf) {
    let root =
        std::env::temp_dir().join(format!("maw-rs-native-mega-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let fake_bin = root.join("bin");
    std::fs::create_dir_all(config.join("fleet")).expect("fleet dir");
    std::fs::create_dir_all(&home).expect("home dir");
    std::fs::create_dir_all(&fake_bin).expect("fake bin dir");
    write_file(
        &config.join("fleet/01-alpha.json"),
        r#"{"name":"01-alpha","windows":[{"name":"alpha-main","repo":"tonkmac/alpha"},{"name":"alpha-team-lead","repo":"tonkmac/alpha"}]}"#,
    );
    write_file(
        &config.join("fleet/02-beta.json"),
        r#"{"name":"02-beta","windows":[{"name":"beta-main","repo":"tonkmac/beta"}]}"#,
    );
    (root, home, config)
}

fn command_with_env(root: &Path, home: &Path, config: &Path) -> Command {
    let mut command = Command::new(maw_rs_bin());
    command
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("cache"))
        .env("TMUX", "hermetic-tmux")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", plugin_dir())
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn mega_ls_team_lead_is_hermetic_and_does_not_need_tmux() {
    assert_eq!(dispatcher_status("mega"), DispatchKind::NativeError);
    let (root, home, config) = seed_mega_env("ls");

    let output = command_with_env(&root, &home, &config)
        .args(["mega", "ls", "--team-lead"])
        .output()
        .expect("run maw mega ls");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout utf8"),
        "\u{1b}[36mmega fleet\u{1b}[0m\n  01-alpha lead  2 windows\n"
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr utf8"), "");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn mega_status_tree_and_kill_use_fake_tmux_and_guard_targets() {
    let (root, home, config) = seed_mega_env("tmux");
    let log = root.join("tmux.log");
    write_file(
        &root.join("bin/tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MEGA_TMUX_LOG"
case "$1" in
  list-windows)
    case "$3" in
      01-alpha) printf '0\talpha-main\t1\t1\n1\talpha-team-lead\t0\t2\n' ;;
      *) exit 1 ;;
    esac
    ;;
  kill-session)
    exit 0
    ;;
  *) exit 64 ;;
esac
"#,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tmux = root.join("bin/tmux");
        let mut permissions = std::fs::metadata(&tmux)
            .expect("tmux metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&tmux, permissions).expect("chmod tmux");
    }

    let status = command_with_env(&root, &home, &config)
        .env("MEGA_TMUX_LOG", &log)
        .args(["mega", "status", "alpha"])
        .output()
        .expect("run maw mega status");
    assert!(
        status.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_eq!(
        String::from_utf8(status.stdout).expect("stdout utf8"),
        "\u{1b}[36mmega status\u{1b}[0m\n  01-alpha  live  2 live / 2 configured windows\n"
    );

    let tree = command_with_env(&root, &home, &config)
        .env("MEGA_TMUX_LOG", &log)
        .args(["mega", "tree", "alpha"])
        .output()
        .expect("run maw mega tree");
    assert!(
        tree.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&tree.stderr)
    );
    assert!(String::from_utf8(tree.stdout)
        .expect("stdout utf8")
        .contains("01-alpha\n  ├─ 0:alpha-main *  1 pane\n"));

    let kill = command_with_env(&root, &home, &config)
        .env("MEGA_TMUX_LOG", &log)
        .args(["mega", "kill", "alpha", "--yes"])
        .output()
        .expect("run maw mega kill");
    assert!(
        kill.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&kill.stderr)
    );
    assert_eq!(
        String::from_utf8(kill.stdout).expect("stdout utf8"),
        "\u{1b}[36mmega kill\u{1b}[0m\n  \u{1b}[32m✓\u{1b}[0m 01-alpha\n"
    );

    let guarded = command_with_env(&root, &home, &config)
        .env("MEGA_TMUX_LOG", &log)
        .args(["mega", "status", "-Sbad"])
        .output()
        .expect("run maw mega guard");
    assert!(!guarded.status.success());
    assert_eq!(
        String::from_utf8(guarded.stderr).expect("stderr utf8"),
        "mega: unknown argument -Sbad\n"
    );

    let log_text = std::fs::read_to_string(&log).expect("tmux log");
    assert!(
        log_text.contains("list-windows -t 01-alpha -F"),
        "{log_text}"
    );
    assert!(log_text.contains("kill-session -t 01-alpha"), "{log_text}");
    assert!(
        !log_text.contains("-Sbad"),
        "guarded target reached tmux: {log_text}"
    );
    let _ = std::fs::remove_dir_all(root);
}
