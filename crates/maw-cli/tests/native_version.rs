#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Command;

fn temp_dir(label: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-version-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("temp bin");
    root
}

#[test]
fn top_level_version_is_native_and_never_falls_back_to_path_maw() {
    let root = temp_dir("path-fallback-guard");
    let fake_maw = root.join("bin/maw");
    fs::write(
        &fake_maw,
        "#!/bin/sh\necho 'maw v999.999.999-from-path'\nexit 0\n",
    )
    .expect("fake maw");
    fs::set_permissions(&fake_maw, fs::Permissions::from_mode(0o700)).expect("chmod");

    for arg in ["--version", "-v", "version"] {
        let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
            .arg(arg)
            .env_clear()
            .env("PATH", root.join("bin"))
            .env("MAW_JS_REF_DIR", "/nonexistent")
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .expect("run maw-rs version");
        assert!(
            output.status.success(),
            "{arg} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.starts_with("maw-rs v"), "{arg} stdout={stdout:?}");
        assert!(!stdout.starts_with("maw-rs vv"), "{arg} stdout={stdout:?}");
        assert!(
            stdout.contains(" (") && stdout.contains(") built "),
            "{arg} stdout={stdout:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).is_empty(),
            "{arg} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let _ = fs::remove_dir_all(root);
}
