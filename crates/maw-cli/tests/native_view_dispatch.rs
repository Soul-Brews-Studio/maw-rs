use std::{
    fs,
    os::unix::fs::PermissionsExt,
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
    let path = std::env::temp_dir().join(format!("maw-rs-view346-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn fake_maw_dir(root: &Path) -> PathBuf {
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).expect("fake bin");
    let fake = bin_dir.join("maw");
    fs::write(
        &fake,
        "#!/bin/sh\necho DELEGATED-MAW >&2\necho DELEGATED-MAW\nexit 42\n",
    )
    .expect("fake maw");
    let mut perms = fs::metadata(&fake)
        .expect("fake maw metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&fake, perms).expect("chmod fake maw");
    bin_dir
}

#[test]
fn view_346_golden_parity_and_fake_maw_no_delegate() {
    let root = temp_dir("golden");
    let fake_bin = fake_maw_dir(&root);
    let path = format!(
        "{}:{}",
        fake_bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(bin())
        .args([
            "view",
            "mawjs",
            "--alive",
            "50-mawjs",
            "--readonly",
            "--no-wake",
            "--print",
        ])
        .env("PATH", path)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_HOME", root.join("maw-home"))
        .output()
        .expect("run maw-rs view");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(
        stdout,
        include_str!("fixtures/native-view/view-readonly-no-wake.stdout")
    );
    assert_eq!(stderr, "");
    assert!(!stdout.contains("DELEGATED-MAW"));
    assert!(!stderr.contains("DELEGATED-MAW"));
    assert_eq!(
        maw_cli::dispatcher_status("view"),
        maw_cli::DispatchKind::Native
    );
}
