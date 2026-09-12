#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
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
    let path = std::env::temp_dir().join(format!("maw-rs-reindex-gpu-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_fake_maw(root: &Path) -> PathBuf {
    let bin = root.join("fake-bin");
    fs::create_dir_all(&bin).expect("fake bin");
    let maw = bin.join("maw");
    fs::write(&maw, "#!/bin/sh\necho DELEGATED-MAW >&2\nexit 73\n").expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mut perms = fs::metadata(&maw).expect("meta").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&maw, perms).expect("chmod");
    }
    bin
}

fn run(root: &Path, args: &[&str]) -> std::process::Output {
    let fake_bin = write_fake_maw(root);
    Command::new(bin())
        .args(args)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env_remove("MAW_REINDEX_GPU_ENDPOINT")
        .env_remove("MAW_REINDEX_GPU_ALT_ENDPOINT")
        .env_remove("MAW_REINDEX_GPU_INDEX_PATH")
        .env(
            "PATH",
            format!(
                "{}:{}",
                fake_bin.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .expect("run maw-rs")
}

fn assert_no_delegate(stdout: &str, stderr: &str) {
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "stdout delegated: {stdout}"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "stderr delegated: {stderr}"
    );
}

#[test]
fn reindex_gpu_rejects_argv_endpoint_without_maw_delegate() {
    let root = temp_dir("argv-endpoint");
    let output = run(
        &root,
        &["reindex-gpu", "--endpoint", "http://127.0.0.1:1/api/embed"],
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(!output.status.success());
    assert_no_delegate(&stdout, &stderr);
    assert!(
        stderr.contains("gateway endpoint must come from config/env, not argv"),
        "{stderr}"
    );
}

#[test]
fn reindex_gpu_requires_configured_gateway_without_maw_delegate() {
    let root = temp_dir("missing-endpoint");
    let output = run(&root, &["reindex-gpu"]);
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(!output.status.success());
    assert_no_delegate(&stdout, &stderr);
    assert!(stderr.contains("gateway endpoint required"), "{stderr}");
}
