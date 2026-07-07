use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn tile_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn tile_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, text).expect("write");
}

fn tile_temp(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("maw-rs-native-tile-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("bin")).expect("temp");
    std::fs::create_dir_all(root.join("repo")).expect("repo");
    root
}

fn tile_install_fake_tmux(root: &Path) {
    tile_write(
        &root.join("bin/tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$MAW_FAKE_TMUX_LOG"
case "$1" in
  display-message)
    case "$*" in
      *'#{window_id}'*) printf '%s\n' '@7' ;;
      *'#{session_name}:#{window_index}.#{pane_index}'*) printf '%s\n' 'alpha-main:1.0' ;;
      *'#{session_name}:#{window_index}'*) printf '%s\n' 'alpha-main:1' ;;
      *) echo "unexpected display-message argv: $*" >&2; exit 12 ;;
    esac
    ;;
  list-panes)
    case "$*" in
      *'#{pane_id}|||#{pane_title}|||#{@maw_tile}'*)
        if [ "$MAW_TILE_MODE" = "clean" ]; then
          printf '%s\n' '%1|||lead|||' '%2|||alpha-main-tile-1|||1' '%3|||worker|||' '%4|||tile-2|||'
        else
          printf '%s\n' '%1|||lead|||'
        fi
        ;;
      *'#{pane_index}|||#{pane_id}|||#{pane_title}|||#{pane_top}'*)
        printf '%s\n' '0|||%1|||lead|||20' '1|||%2|||tile-1|||40' '2|||%3|||tile-2|||10'
        ;;
      *'#{pane_height}'*) printf '%s\n' '20' '20' '20' ;;
      *'#{pane_id}'*) printf '%s\n' '%1' '%2' '%3' ;;
      *) echo "unexpected list-panes argv: $*" >&2; exit 13 ;;
    esac
    ;;
  split-window)
    if [ "$MAW_FAKE_SPLITS" = "one" ]; then printf '%s\n' '%2'; else printf '%s\n' '%2'; fi
    ;;
  select-pane|set-option|select-layout|send-keys|swap-pane|kill-pane)
    :
    ;;
  *)
    echo "unexpected tmux command: $*" >&2
    exit 9
    ;;
esac
"#,
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(root.join("bin/tmux"))
            .expect("metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(root.join("bin/tmux"), permissions).expect("chmod");
    }
}

fn tile_command(root: &Path) -> Command {
    let mut command = Command::new(tile_bin());
    command
        .current_dir(root.join("repo"))
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("home/.maw"))
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("xdg-state"))
        .env("XDG_DATA_HOME", root.join("xdg-data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("TMUX", "/tmp/tmux-103,1,0")
        .env("TMUX_PANE", "%1")
        .env("MAW_FAKE_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn tile_native_spawn_cmd_uses_safe_tmux_argv_and_layouts() {
    let root = tile_temp("spawn");
    tile_install_fake_tmux(&root);
    let output = tile_command(&root)
        .args([
            "tile",
            "1",
            "--path",
            ".",
            "--cmd",
            "echo ok",
            "--session-id",
            "solo",
        ])
        .output()
        .expect("run tile");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("alpha-main-tile-1 → %2"), "{stdout}");
    assert!(
        stdout.contains("\u{1b}[32m✓\u{1b}[0m 1 panes tiled (path, cmd)"),
        "{stdout}"
    );
    assert_eq!(dispatcher_status("tile"), DispatchKind::Native);
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("log");
    assert!(
        log.contains("display-message -t %1 -p #{window_id}"),
        "{log}"
    );
    assert!(
        log.contains("split-window -t %1 -h -P -F #{pane_id}"),
        "{log}"
    );
    assert!(log.contains("select-layout -t @7 main-vertical"), "{log}");
    assert!(!log.contains("pane-border-status"), "{log}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tile_native_border_flag_opts_in_to_top_border_status() {
    let root = tile_temp("border");
    tile_install_fake_tmux(&root);
    let output = tile_command(&root)
        .args(["tile", "1", "--border"])
        .output()
        .expect("run tile border");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("log");
    assert!(
        log.contains("set-option -w -t @7 pane-border-status top"),
        "{log}"
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tile_native_swap_resolves_titles_and_uses_tmux_targets() {
    let root = tile_temp("swap");
    tile_install_fake_tmux(&root);
    let output = tile_command(&root)
        .args(["tile", "swap", "top", "bottom"])
        .output()
        .expect("run tile swap");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "\u{1b}[32m✓\u{1b}[0m swapped tile-2 ↔ tile-1\n"
    );
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("log");
    assert!(
        log.contains(
            "list-panes -t @7 -F #{pane_index}|||#{pane_id}|||#{pane_title}|||#{pane_top}"
        ),
        "{log}"
    );
    assert!(log.contains("swap-pane -s %3 -t %2"), "{log}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tile_native_clean_kills_only_marked_tile_panes() {
    let root = tile_temp("clean");
    tile_install_fake_tmux(&root);
    let output = tile_command(&root)
        .env("MAW_TILE_MODE", "clean")
        .args(["tile", "clean"])
        .output()
        .expect("run tile clean");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("alpha-main-tile-1 (%2)"), "{stdout}");
    assert!(stdout.contains("tile-2 (%4)"), "{stdout}");
    assert!(stdout.contains("cleaned 2 tiles"), "{stdout}");
    let log = std::fs::read_to_string(root.join("tmux.log")).expect("log");
    assert!(log.contains("kill-pane -t %2"), "{log}");
    assert!(log.contains("kill-pane -t %4"), "{log}");
    assert!(!log.contains("kill-pane -t %3"), "{log}");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tile_native_guard_rejects_separator_before_tmux_mutation() {
    let root = tile_temp("guard");
    tile_install_fake_tmux(&root);
    let output = tile_command(&root)
        .args(["tile", "--", "1"])
        .output()
        .expect("run tile guard");
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr)
        .expect("stderr")
        .contains("-- separator is not supported"));
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(
        !log.contains("split-window"),
        "guard must not mutate tmux: {log}"
    );
    assert!(
        !log.contains("select-layout"),
        "guard must not mutate tmux: {log}"
    );
    let _ = std::fs::remove_dir_all(root);
}
