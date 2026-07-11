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
    let path = std::env::temp_dir().join(format!("maw-rs-soul-sync-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write(path: &Path, text: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
    fs::write(path, text).expect("write");
}

fn install_plugin(root: &Path) -> PathBuf {
    let plugin = root.join("plugins/soul-sync");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(
        plugin.join("plugin.json"),
        include_str!("fixtures/native-soul-sync/soul-sync-plugin/plugin.json"),
    )
    .expect("plugin json");
    fs::write(
        plugin.join("plugin.wasm"),
        include_bytes!("fixtures/native-soul-sync/soul-sync-plugin/plugin.wasm"),
    )
    .expect("plugin wasm");
    root.join("plugins")
}

fn seed(root: &Path) -> (PathBuf, PathBuf, PathBuf, PathBuf) {
    let home = root.join("home");
    let maw_home = root.join("maw-home");
    let ghq = root.join("ghq");
    let neo = ghq.join("github.com/org/neo-oracle");
    let trinity = ghq.join("github.com/org/trinity-oracle");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&trinity).expect("peer repo");
    write(&neo.join("ψ/memory/learnings/a.md"), "new");
    write(
        &maw_home.join("fleet/01-neo.json"),
        r#"{"name":"01-neo","windows":[{"name":"neo-oracle","repo":"org/neo-oracle"}],"sync_peers":["trinity"]}"#,
    );
    write(
        &maw_home.join("fleet/02-trinity.json"),
        r#"{"name":"02-trinity","windows":[{"name":"trinity-oracle","repo":"org/trinity-oracle"}]}"#,
    );
    (home, maw_home, ghq, neo)
}

fn run(command: &str, root: &Path) -> std::process::Output {
    let (home, maw_home, ghq, cwd) = seed(root);
    let plugins = install_plugin(root);
    Command::new(bin())
        .arg(command)
        .current_dir(cwd)
        .env("HOME", home)
        .env("MAW_HOME", maw_home)
        .env("GHQ_ROOT", ghq)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", plugins)
        .env_remove("TMUX")
        .output()
        .expect("run maw-rs")
}

fn assert_push_parity(command: &str) {
    let root = temp_dir(command);
    let output = run(command, &root);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-soul-sync/push.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
    assert_eq!(
        fs::read_to_string(root.join("ghq/github.com/org/trinity-oracle/ψ/memory/learnings/a.md"))
            .expect("copied"),
        "new"
    );
    assert!(
        fs::read_to_string(root.join("ghq/github.com/org/trinity-oracle/ψ/.soul-sync/sync.log"))
            .expect("sync log")
            .contains("neo → trinity | 1 files | 1 learnings")
    );
}

#[test]
fn soul_sync_plugin_matches_native_push_golden() {
    assert_push_parity("soul-sync");
}

#[test]
fn soul_sync_plugin_aliases_match_native_push_golden() {
    assert_push_parity("soulsync");
    assert_push_parity("ss");
}

#[test]
fn soul_sync_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    for command in ["soul-sync", "soulsync", "ss"] {
        assert_eq!(
            maw_cli::dispatcher_status(command),
            maw_cli::DispatchKind::NativeError
        );
    }
}
