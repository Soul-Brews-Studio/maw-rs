use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_root(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-messages-serve-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp root");
    path
}

fn seed_fake_maw(root: &Path) -> PathBuf {
    let bin_dir = root.join("fake-bin");
    fs::create_dir_all(&bin_dir).expect("fake bin");
    let maw = bin_dir.join("maw");
    fs::write(&maw, "#!/bin/sh\necho DELEGATED-MAW >&2\nexit 73\n").expect("fake maw");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&maw).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&maw, perms).expect("chmod");
    }
    bin_dir
}

fn run(args: &[&str], fake_path: &Path) -> Output {
    let old_path = std::env::var("PATH").unwrap_or_default();
    Command::new(bin())
        .args(args)
        .env("MAW_HOME", "/tmp/maw-rs-native-ms-fixed")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_ENGINE_URL", "http://127.0.0.1:3456")
        // Force a deterministic port probe so `serve status` doesn't report
        // "running" on a dev box that has a real daemon on :3456 (see #152).
        .env("MAW_SERVE_FAKE_PROBE", "nolistener")
        .env("PATH", format!("{}:{old_path}", fake_path.display()))
        .output()
        .expect("run maw-rs")
}

fn assert_native_output(output: Output, expected: &str) {
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert_eq!(stdout, expected);
    assert_eq!(stderr, "");
    assert!(!stdout.contains("DELEGATED-MAW"));
    assert!(!stderr.contains("DELEGATED-MAW"));
}

#[test]
fn serve_status_stop_and_messages_lifecycle_are_native_no_delegate() {
    let root = temp_root("golden");
    let fake_path = seed_fake_maw(&root);

    assert_native_output(
        run(&["serve", "status"], &fake_path),
        include_str!("fixtures/native-messages-serve/serve-status.stdout"),
    );
    assert_native_output(
        run(&["serve", "stop"], &fake_path),
        include_str!("fixtures/native-messages-serve/serve-stop.stdout"),
    );
    assert_native_output(
        run(
            &["messages", "status", "--engine", "http://127.0.0.1:3456"],
            &fake_path,
        ),
        include_str!("fixtures/native-messages-serve/messages-status.stdout"),
    );
    assert_native_output(
        run(
            &[
                "messages",
                "serve",
                "--detach",
                "--engine",
                "http://127.0.0.1:3456",
                "--port",
                "0",
            ],
            &fake_path,
        ),
        include_str!("fixtures/native-messages-serve/messages-serve-detach.stdout"),
    );
}

#[test]
fn serve_messages_lifecycle_rejects_untrusted_args_before_io() {
    let root = temp_root("guards");
    let fake_path = seed_fake_maw(&root);
    let bad_engine = run(&["messages", "status", "--engine", "--bad"], &fake_path);
    assert!(!bad_engine.status.success());
    assert!(String::from_utf8_lossy(&bad_engine.stderr).contains("rejected --engine"));
    assert!(!String::from_utf8_lossy(&bad_engine.stderr).contains("DELEGATED-MAW"));

    let bad_serve = run(&["serve", "status", "--bad"], &fake_path);
    assert!(!bad_serve.status.success());
    assert!(String::from_utf8_lossy(&bad_serve.stderr).contains("unexpected argument"));
    assert!(!String::from_utf8_lossy(&bad_serve.stderr).contains("DELEGATED-MAW"));
}
