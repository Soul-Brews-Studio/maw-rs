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

#[test]
fn kill_peer_native_forward_matches_committed_golden_without_real_maw_or_network() {
    let root = temp_dir("kill-peer-native");
    let fake_bin = root.join("bin");
    let home = root.join("home");
    let xdg_state = root.join("state");
    let xdg_config = root.join("config");
    let xdg_cache = root.join("cache");
    let xdg_data = root.join("data");
    for dir in [
        &fake_bin,
        &home,
        &xdg_state,
        &xdg_config,
        &xdg_cache,
        &xdg_data,
    ] {
        fs::create_dir_all(dir).expect("mkdir");
    }

    write_script(
        &fake_bin.join("maw"),
        "#!/bin/sh\necho 'DELEGATED-MAW'\nexit 77\n",
    );
    write_script(
        &fake_bin.join("bun"),
        "#!/bin/sh\necho 'DELEGATED-BUN'\nexit 78\n",
    );
    write_script(
        &fake_bin.join("curl"),
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > \"$FAKE_CURL_ARGS\"\nprintf '{\"ok\":true,\"output\":\"remote fake\"}__MAW_HTTP_STATUS__:200'\n",
    );
    let peers = root.join("peers.json");
    fs::write(
        &peers,
        r#"{"version":1,"peers":{"fakepeer":{"url":"http://fake-peer.invalid:3456","node":"fakepeer"}}}"#,
    )
    .expect("peers");
    let curl_args = root.join("curl.args");

    let output = Command::new(bin())
        .args(["kill", "faketarget", "--peer", "fakepeer", "--pane", "1"])
        .env_clear()
        .env("HOME", &home)
        .env("XDG_STATE_HOME", &xdg_state)
        .env("XDG_CONFIG_HOME", &xdg_config)
        .env("XDG_CACHE_HOME", &xdg_cache)
        .env("XDG_DATA_HOME", &xdg_data)
        .env("PEERS_FILE", &peers)
        .env("MAW_SENDER", "local:test-oracle")
        .env(
            "MAW_PEER_KEY",
            "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        )
        .env("MAW_FEDERATION_TOKEN", "test-federation-token")
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("PATH", &fake_bin)
        .env("FAKE_CURL_ARGS", &curl_args)
        .output()
        .expect("run maw-rs");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert_eq!(
        stdout,
        include_str!("fixtures/native-kill/kill-peer-forward.stdout")
    );
    assert!(!stdout.contains("DELEGATED-MAW"));
    assert!(!stdout.contains("DELEGATED-BUN"));
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");

    let curl = fs::read_to_string(curl_args).expect("curl args");
    assert!(curl.contains("http://fake-peer.invalid:3456/api/kill"));
    assert!(curl.contains("--data-binary\n{\"pane\":1,\"target\":\"faketarget\"}"));
    assert!(!curl.contains("sh\n-c"));
}
