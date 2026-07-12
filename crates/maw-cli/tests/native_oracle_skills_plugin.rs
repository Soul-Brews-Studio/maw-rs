use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "maw-rs-native-oracle-skills-{name}-{}-{stamp}-{}",
        std::process::id(),
        NEXT_DIR.fetch_add(1, Ordering::Relaxed)
    ));
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

fn write_fake_oracle_skills(bin_dir: &Path) {
    let exe = bin_dir.join("arra-oracle-skills");
    fs::write(
        &exe,
        r#"#!/bin/sh
: > "$MAW_ORACLE_SKILLS_ARGV_LOG"
for arg in "$@"; do
  printf '<%s>\n' "$arg" >> "$MAW_ORACLE_SKILLS_ARGV_LOG"
done
if [ "${MAW_ORACLE_SKILLS_EXIT:-0}" -ne 0 ]; then
  printf 'oracle-skills child stderr\n' >&2
  printf 'oracle-skills child stdout before failure\n'
  exit "$MAW_ORACLE_SKILLS_EXIT"
fi
printf 'oracle-skills child stdout\n'
printf 'oracle-skills child stderr\n' >&2
"#,
    )
    .expect("write fake arra-oracle-skills");
    chmod_exec(&exe);
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let bin_dir = root.join("bin");
    let home = root.join("home");
    let xdg_config = root.join("xdg-config");
    let xdg_data = root.join("xdg-data");
    let xdg_state = root.join("xdg-state");
    fs::create_dir_all(&home).expect("home");
    fs::create_dir_all(&xdg_config).expect("xdg config");
    fs::create_dir_all(&xdg_data).expect("xdg data");
    fs::create_dir_all(&xdg_state).expect("xdg state");

    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env_clear()
        .env("PATH", &bin_dir)
        .env("HOME", &home)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_ORACLE_SKILLS_ARGV_LOG", root.join("argv.log"))
        .output()
        .expect("run maw-rs")
}

#[test]
fn native_oracle_skills_passes_args_to_arra_binary_and_matches_golden() {
    let root = temp_dir("pass-through");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_oracle_skills(&bin_dir);

    let output = run(
        &root,
        &[
            "oracle-skills",
            "list",
            "--agent=codex",
            "--literal=-not-a-maw-flag",
        ],
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "oracle-skills child stdout\n"
    );
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr"),
        "oracle-skills child stderr\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("argv.log")).expect("argv log"),
        "<list>\n<--agent=codex>\n<--literal=-not-a-maw-flag>\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_oracle_skills_is_native_and_option_injection_safe() {
    let root = temp_dir("guard");
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_oracle_skills(&bin_dir);

    assert_eq!(dispatcher_status("oracle-skills"), DispatchKind::Native);

    let output = run(
        &root,
        &[
            "oracle-skills",
            "--",
            "$(touch should-not-exist)",
            ";touch also-should-not-exist",
        ],
    );

    assert!(output.status.success());
    assert_eq!(
        fs::read_to_string(root.join("argv.log")).expect("argv log"),
        "<-->\n<$(touch should-not-exist)>\n<;touch also-should-not-exist>\n"
    );
    assert!(!root.join("should-not-exist").exists());
    assert!(!root.join("also-should-not-exist").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn native_oracle_skills_missing_binary_surfaces_install_hint_without_maw_js_ref() {
    let root = temp_dir("missing");
    fs::create_dir_all(root.join("bin")).expect("bin dir");

    let output = run(&root, &["oracle-skills", "list"]);

    assert!(!output.status.success());
    assert_eq!(String::from_utf8(output.stdout).expect("stdout"), "");
    assert_eq!(
        String::from_utf8(output.stderr).expect("stderr"),
        "arra-oracle-skills not found on $PATH. Install with: bun add -g arra-oracle-skills\n"
    );
    fs::remove_dir_all(root).expect("cleanup");
}
