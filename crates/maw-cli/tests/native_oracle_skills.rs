#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn oracle_skills_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-oracle-skills-{label}-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join("config/maw")).expect("config");
    fs::create_dir_all(root.join("state")).expect("state");
    root
}

fn oracle_skills_chmod_exec(path: &Path) {
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms).expect("chmod");
}

fn oracle_skills_write_fake_marker(bin_dir: &Path, name: &str, marker: &str) {
    let path = bin_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho '{marker} $*'\nexit 0\n")).expect("marker");
    oracle_skills_chmod_exec(&path);
}

fn oracle_skills_write_fake_arra(bin_dir: &Path) {
    let path = bin_dir.join("arra-oracle-skills");
    fs::write(&path, "#!/bin/sh\necho \"ARRA-ORACLE-SKILLS $*\"\nexit 0\n").expect("arra");
    oracle_skills_chmod_exec(&path);
}

fn oracle_skills_run(root: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(args)
        .env_clear()
        .env("PATH", root.join("bin"))
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("MAW_CONFIG_DIR", root.join("config/maw"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run oracle-skills")
}

fn oracle_skills_assert_no_delegation(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("DELEGATED-MAW"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "stderr={stderr}");
    assert!(!stdout.contains("DELEGATED-BUN"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-BUN"), "stderr={stderr}");
}

#[test]
fn oracle_skills_runtime_fake_maw_no_delegate_proof() {
    assert_eq!(dispatcher_status("oracle-skills"), DispatchKind::Native);
    let root = oracle_skills_temp_dir("runtime-proof");
    let bin_dir = root.join("bin");
    oracle_skills_write_fake_marker(&bin_dir, "maw", "DELEGATED-MAW");
    oracle_skills_write_fake_marker(&bin_dir, "bun", "DELEGATED-BUN");
    oracle_skills_write_fake_arra(&bin_dir);

    let output = oracle_skills_run(&root, &["oracle-skills", "list", "--json"]);
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    oracle_skills_assert_no_delegation(&output);
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        include_str!("fixtures/native-oracle-skills/list-json.stdout")
    );
    assert!(
        output.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );

    let help = oracle_skills_run(&root, &["oracle-skills", "--help"]);
    assert!(
        help.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&help.stderr)
    );
    oracle_skills_assert_no_delegation(&help);
    assert_eq!(
        String::from_utf8_lossy(&help.stdout),
        include_str!("fixtures/native-oracle-skills/help.stdout")
    );
    assert!(
        help.stderr.is_empty(),
        "stderr={}",
        String::from_utf8_lossy(&help.stderr)
    );
    let _ = fs::remove_dir_all(root);
}
