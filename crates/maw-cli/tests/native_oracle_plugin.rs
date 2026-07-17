#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::path::{Path, PathBuf};
use std::process::Command;

fn oracle_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn oracle_write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent dir");
    }
    std::fs::write(path, text).expect("write file");
}

fn oracle_chmod(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}

fn oracle_seed(name: &str) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-oracle-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    let home = root.join("home");
    let config = root.join("config");
    let cache = root.join("cache");
    let bin = root.join("bin");
    std::fs::create_dir_all(&bin).expect("bin dir");
    std::fs::create_dir_all(root.join("ghq/github.com/org/neo-oracle/ψ")).expect("neo repo");
    std::fs::create_dir_all(root.join("ghq/github.com/org/morpheus-oracle/ψ"))
        .expect("morpheus repo");
    oracle_write(
        &config.join("fleet/01-neo.json"),
        r#"{"windows":[{"name":"neo-oracle","repo":"org/neo-oracle"}]}"#,
    );
    oracle_write(
        &cache.join("oracles.json"),
        &format!(
            r#"{{"schema":1,"local_scanned_at":"1000","ghq_root":"{}","oracles":[{{"org":"org","repo":"neo-oracle","name":"neo","local_path":"{}","has_psi":true,"has_fleet_config":true,"budded_from":null,"budded_at":null,"federation_node":null,"detected_at":"1000","nickname":"Neo Prime"}},{{"org":"org","repo":"trinity-oracle","name":"trinity","local_path":"","has_psi":false,"has_fleet_config":false,"budded_from":null,"budded_at":null,"federation_node":null,"detected_at":"900"}}]}}"#,
            root.join("ghq").display(),
            root.join("ghq/github.com/org/neo-oracle").display()
        ),
    );
    oracle_write(
        &bin.join("tmux"),
        r#"#!/bin/sh
printf '%s\n' "$*" >> "$ORACLE_TMUX_LOG"
case "$1" in
  list-sessions) printf '01-neo\n';;
  list-windows) printf '01-neo|||neo-oracle\n';;
  *) exit 64 ;;
esac
"#,
    );
    oracle_chmod(&bin.join("tmux"));
    (root, home, config, cache)
}

fn oracle_command(root: &Path, home: &Path, config: &Path, cache: &Path) -> Command {
    let mut command = Command::new(oracle_bin());
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", home)
        .env("MAW_CONFIG_DIR", config)
        .env("MAW_CACHE_DIR", cache)
        .env("XDG_CONFIG_HOME", root.join("xdg-config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CACHE_HOME", root.join("xdg-cache"))
        .env("GHQ_ROOT", root.join("ghq"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("ORACLE_TMUX_LOG", root.join("tmux.log"))
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn oracle_native_list_golden_is_hermetic_without_real_registry() {
    let (root, home, config, cache) = oracle_seed("list");
    let output = oracle_command(&root, &home, &config, &cache)
        .args(["oracle", "ls", "--path"])
        .output()
        .expect("run oracle ls");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected = format!(
        "{}\n",
        include_str!("fixtures/native-oracle/list.stdout")
            .replace("{ROOT}", &root.to_string_lossy())
    );
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), expected);
    assert_eq!(dispatcher_status("oracle"), DispatchKind::Native);
    assert_eq!(dispatcher_status("oracles"), DispatchKind::Native);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn oracle_native_register_search_nickname_and_guard_are_hermetic() {
    let (root, home, config, cache) = oracle_seed("ops");
    let register = oracle_command(&root, &home, &config, &cache)
        .args(["oracles", "register", "morpheus", "--json"])
        .output()
        .expect("register");
    assert!(
        register.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&register.stderr)
    );
    assert!(String::from_utf8(register.stdout)
        .expect("stdout")
        .contains("morpheus-oracle"));

    let set = oracle_command(&root, &home, &config, &cache)
        .args(["oracle", "set-nickname", "morpheus", "Moe"])
        .output()
        .expect("set nickname");
    assert!(
        set.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&set.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("ghq/github.com/org/morpheus-oracle/ψ/nickname"))
            .expect("nickname"),
        "Moe\n"
    );

    let search = oracle_command(&root, &home, &config, &cache)
        .args(["oracle", "search", "Moe", "--json"])
        .output()
        .expect("search");
    assert!(
        search.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&search.stderr)
    );
    assert!(String::from_utf8(search.stdout)
        .expect("stdout")
        .contains("morpheus"));

    let guarded = oracle_command(&root, &home, &config, &cache)
        .args(["oracle", "ls", "--org", "-bad"])
        .output()
        .expect("guard");
    assert!(!guarded.status.success());
    assert_eq!(
        String::from_utf8(guarded.stderr).expect("stderr"),
        "oracle: --org requires a value\n"
    );
    let log = std::fs::read_to_string(root.join("tmux.log")).unwrap_or_default();
    assert!(!log.contains("-bad"), "guarded arg reached tmux: {log}");
    let _ = std::fs::remove_dir_all(root);
}
