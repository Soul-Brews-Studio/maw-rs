//! mawx WI-3 — install-root == discovery-root by construction.
//!
//! Before this fix the installer resolved its default root via
//! `maw_data_path(["plugins"])` (honors `MAW_DATA_DIR`/`MAW_XDG`) while
//! discovery's `scan_dirs()` only looked at `MAW_PLUGINS_DIR` →
//! `MAW_HOME/plugins` → `~/.maw/plugins`, so a plugin installed under
//! `MAW_DATA_DIR=X` (no `MAW_PLUGINS_DIR`) was never found. The reverse
//! divergence also existed: with `MAW_PLUGINS_DIR=Y` set, discovery scanned
//! only `Y` but the installer still wrote to the data dir.
//!
//! Subprocess-based (`CARGO_BIN_EXE_maw-rs`) so each case controls its whole
//! environment without racing other in-process env-mutating tests.

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn maw_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-wi3-dual-root-{label}-{}-{nonce}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("create temp dir");
    dir
}

/// A dev-tier TS plugin whose verb dispatch goes through the `bun` shim.
fn write_ts_plugin(source: &Path, name: &str, command: &str) {
    fs::create_dir_all(source).expect("plugin source dir");
    fs::write(
        source.join("index.ts"),
        b"export default async function plugin() {}\n",
    )
    .expect("entry");
    fs::write(
        source.join("plugin.json"),
        format!(
            r#"{{
  "name": "{name}",
  "version": "1.0.0",
  "sdk": "*",
  "target": "js",
  "entry": "index.ts",
  "cli": {{ "command": "{command}", "help": "maw {command}" }}
}}
"#
        ),
    )
    .expect("manifest");
}

fn write_bun_shim(bin_dir: &Path) {
    fs::create_dir_all(bin_dir).expect("bin dir");
    let shim = bin_dir.join("bun");
    fs::write(
        &shim,
        "#!/bin/sh\nif [ -n \"$BUN_SHIM_ARGS\" ]; then printf 'entry=%s\\n' \"$1\" > \"$BUN_SHIM_ARGS\"; fi\nprintf 'bun stdout\\n'\n",
    )
    .expect("write bun shim");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&shim).expect("shim metadata").permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&shim, permissions).expect("chmod bun shim");
    }
}

/// Run `maw` with a fully controlled env: only the vars in `set` are present
/// from the plugins/data family.
fn run_maw(root: &Path, args: &[&str], set: &[(&str, &Path)]) -> Output {
    let mut command = Command::new(maw_bin());
    command
        .args(args)
        .env("HOME", root.join("home"))
        .env_remove("MAW_PLUGINS_DIR")
        .env_remove("MAW_HOME")
        .env_remove("MAW_DATA_DIR")
        .env_remove("MAW_XDG")
        .env_remove("XDG_DATA_HOME")
        .env_remove("MAW_PLUGINS_LOCK")
        .env_remove("MAW_PLUGIN_DEV")
        .env_remove("BUN_SHIM_ARGS");
    for (key, value) in set {
        command.env(key, value);
    }
    command.output().expect("run maw")
}

fn stdio(output: &Output) -> String {
    format!(
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Spec done-criteria: install under a scratch `MAW_DATA_DIR=X` with no
/// `MAW_PLUGINS_DIR` set → discovery finds it → its verb dispatches.
#[test]
fn install_under_maw_data_dir_is_discovered_and_verb_dispatches() {
    let root = temp_dir("data-dir");
    let data_dir = root.join("data");
    let source = root.join("src-pkg");
    let bin_dir = root.join("bin");
    write_ts_plugin(&source, "wi3-demo", "wi3demo");
    write_bun_shim(&bin_dir);
    let data_env: &[(&str, &Path)] = &[("MAW_DATA_DIR", &data_dir)];

    // Install with only MAW_DATA_DIR set — lands in <data>/plugins.
    let install = run_maw(
        &root,
        &["plugin", "install", source.to_str().expect("source utf8")],
        data_env,
    );
    assert_eq!(install.status.code(), Some(0), "{}", stdio(&install));
    assert!(
        data_dir.join("plugins/wi3-demo/plugin.json").is_file(),
        "install must write under MAW_DATA_DIR/plugins"
    );
    assert!(
        !root.join("home/.maw/plugins/wi3-demo").exists(),
        "install must not write to the legacy home root"
    );

    // Discovery (same env, still no MAW_PLUGINS_DIR) finds it.
    let listed = run_maw(&root, &["plugins", "--json"], data_env);
    assert_eq!(listed.status.code(), Some(0), "{}", stdio(&listed));
    assert!(
        String::from_utf8_lossy(&listed.stdout).contains("wi3-demo"),
        "discovery must find the installed plugin: {}",
        stdio(&listed)
    );

    // Its verb dispatches, loaded from the install root.
    let bun_args = root.join("bun-args");
    let dispatch_env: &[(&str, &Path)] = &[
        ("MAW_DATA_DIR", &data_dir),
        ("PATH", &bin_dir),
        ("BUN_SHIM_ARGS", &bun_args),
    ];
    let dispatched = run_maw(&root, &["wi3demo"], dispatch_env);
    assert_eq!(dispatched.status.code(), Some(0), "{}", stdio(&dispatched));
    assert_eq!(
        String::from_utf8_lossy(&dispatched.stdout),
        "bun stdout\n",
        "{}",
        stdio(&dispatched)
    );
    let captured = fs::read_to_string(&bun_args).expect("bun shim args");
    assert_eq!(
        captured,
        format!(
            "entry={}\n",
            data_dir.join("plugins/wi3-demo/index.ts").display()
        ),
        "dispatch must load the entry from the MAW_DATA_DIR install root"
    );

    let _ = fs::remove_dir_all(root);
}

/// Reverse direction: with `MAW_PLUGINS_DIR` set (discovery's exclusive scan
/// root), a default `maw plugin install` must land there too — not in the
/// data dir where discovery would never look.
#[test]
fn install_honors_maw_plugins_dir_so_discovery_root_matches() {
    let root = temp_dir("plugins-dir");
    let plugins_dir = root.join("explicit-plugins");
    let source = root.join("src-pkg");
    let bin_dir = root.join("bin");
    write_ts_plugin(&source, "wi3-explicit", "wi3explicit");
    write_bun_shim(&bin_dir);
    let plugins_env: &[(&str, &Path)] = &[("MAW_PLUGINS_DIR", &plugins_dir)];

    let install = run_maw(
        &root,
        &["plugin", "install", source.to_str().expect("source utf8")],
        plugins_env,
    );
    assert_eq!(install.status.code(), Some(0), "{}", stdio(&install));
    assert!(
        plugins_dir.join("wi3-explicit/plugin.json").is_file(),
        "install must write into MAW_PLUGINS_DIR"
    );
    assert!(
        !root.join("home/.maw/plugins/wi3-explicit").exists(),
        "install must not fall back to the data root when MAW_PLUGINS_DIR is set"
    );

    let bun_args = root.join("bun-args");
    let dispatch_env: &[(&str, &Path)] = &[
        ("MAW_PLUGINS_DIR", &plugins_dir),
        ("PATH", &bin_dir),
        ("BUN_SHIM_ARGS", &bun_args),
    ];
    let dispatched = run_maw(&root, &["wi3explicit"], dispatch_env);
    assert_eq!(dispatched.status.code(), Some(0), "{}", stdio(&dispatched));
    let captured = fs::read_to_string(&bun_args).expect("bun shim args");
    assert_eq!(
        captured,
        format!(
            "entry={}\n",
            plugins_dir.join("wi3-explicit/index.ts").display()
        ),
        "dispatch must load the entry from MAW_PLUGINS_DIR"
    );

    let _ = fs::remove_dir_all(root);
}

/// An explicit `--root` still beats every env default.
#[test]
fn install_explicit_root_flag_still_wins() {
    let root = temp_dir("root-flag");
    let data_dir = root.join("data");
    let explicit_root = root.join("explicit-root");
    let source = root.join("src-pkg");
    write_ts_plugin(&source, "wi3-flag", "wi3flag");

    let install = run_maw(
        &root,
        &[
            "plugin",
            "install",
            source.to_str().expect("source utf8"),
            "--root",
            explicit_root.to_str().expect("root utf8"),
        ],
        &[("MAW_DATA_DIR", &data_dir)],
    );
    assert_eq!(install.status.code(), Some(0), "{}", stdio(&install));
    assert!(explicit_root.join("wi3-flag/plugin.json").is_file());
    assert!(!data_dir.join("plugins/wi3-flag").exists());

    let _ = fs::remove_dir_all(root);
}
