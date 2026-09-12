#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let release = manifest.join("../..").join("target/release/maw-rs");
    if release.exists() {
        release
    } else {
        PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
    }
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).expect("write script");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).expect("chmod");
    }
}

fn write_fake_maw(fake_bin: &Path) {
    write_script(
        &fake_bin.join("maw"),
        "#!/bin/sh\necho 'DELEGATED-MAW'\nexit 77\n",
    );
}

fn write_fake_bun(fake_bin: &Path) {
    write_script(
        &fake_bin.join("bun"),
        "#!/bin/sh\necho 'DELEGATED-BUN'\nexit 78\n",
    );
}

fn write_fake_curl(fake_bin: &Path) {
    write_script(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$@\" >> \"$FAKE_CURL_ARGS\"\nprintf '{\"enabled\":true,\"tokenPreview\":\"tokn****\"}__MAW_HTTP_STATUS__:200'\n",
    );
}

fn assert_no_fake_maw_runtime_delegation(stdout: &str, stderr: &str) {
    assert!(
        !stdout.contains("DELEGATED-MAW"),
        "stdout must prove native runtime path, got fake maw marker: {stdout}"
    );
    assert!(
        !stderr.contains("DELEGATED-MAW"),
        "stderr must prove native runtime path, got fake maw marker: {stderr}"
    );
}

fn assert_no_fake_bun_runtime_delegation(stdout: &str, stderr: &str) {
    assert!(
        !stdout.contains("DELEGATED-BUN"),
        "stdout must prove native runtime path, got fake bun marker: {stdout}"
    );
    assert!(
        !stderr.contains("DELEGATED-BUN"),
        "stderr must prove native runtime path, got fake bun marker: {stderr}"
    );
}

fn run_ping(
    root: &Path,
    fake_bin: &Path,
    maw_home: &Path,
    curl_args: &Path,
    args: &[&str],
) -> Output {
    let mut command = Command::new(bin());
    command
        .args(args)
        .env_clear()
        .env("MAW_HOME", maw_home)
        .env("PEERS_FILE", root.join("missing-peers.json"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_PING_NOW_MS", "1000")
        .env("PATH", fake_bin)
        .env("FAKE_CURL_ARGS", curl_args);
    command.output().expect("run maw-rs")
}

#[test]
fn ping_runtime_fake_maw_proof_covers_ping_all_and_ping_peer() {
    let root = temp_dir("ping-native");
    let fake_bin = root.join("bin");
    let maw_home = root.join("maw-home");
    let config_dir = maw_home.join("config");
    fs::create_dir_all(&fake_bin).expect("fake bin");
    fs::create_dir_all(&config_dir).expect("config dir");

    write_fake_maw(&fake_bin);
    write_fake_bun(&fake_bin);
    write_fake_curl(&fake_bin);
    fs::write(
        config_dir.join("maw.config.json"),
        r#"{"node":"local","oracle":"tester","namedPeers":[{"name":"fakepeer","url":"http://fake-peer.invalid:3456"}]}"#,
    )
    .expect("config");
    let curl_args = root.join("curl.args");

    for args in [&["ping"][..], &["ping", "fakepeer"][..]] {
        let output = run_ping(&root, &fake_bin, &maw_home, &curl_args, args);
        assert!(
            output.status.success(),
            "args={args:?} stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout");
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        assert_eq!(
            stdout,
            include_str!("fixtures/native-ping/ping-peer-success.stdout"),
            "args={args:?}"
        );
        assert_eq!(stderr, "", "args={args:?}");
        assert_no_fake_maw_runtime_delegation(&stdout, &stderr);
        assert_no_fake_bun_runtime_delegation(&stdout, &stderr);
    }

    let curl = fs::read_to_string(curl_args).expect("curl args");
    assert_eq!(
        curl.matches("http://fake-peer.invalid:3456/api/auth/status")
            .count(),
        2
    );
    assert_eq!(
        curl.matches("--\nhttp://fake-peer.invalid:3456/api/auth/status")
            .count(),
        2
    );
    assert!(!curl.contains("sh\n-c"));
}
