#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_cli::{dispatcher_status, DispatchKind};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-federation-{label}-{}-{nonce}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("bin")).expect("bin");
    fs::create_dir_all(root.join("config/maw")).expect("config");
    fs::create_dir_all(root.join("state")).expect("state");
    root
}

fn chmod_exec(path: &Path) {
    let mut perms = fs::metadata(path).expect("metadata").permissions();
    perms.set_mode(0o700);
    fs::set_permissions(path, perms).expect("chmod");
}

fn write_fake_marker(bin_dir: &Path, name: &str, marker: &str) {
    let path = bin_dir.join(name);
    fs::write(&path, format!("#!/bin/sh\necho '{marker} $*'\nexit 0\n")).expect("marker");
    chmod_exec(&path);
}

fn write_fake_curl(bin_dir: &Path, log: &Path) {
    let path = bin_dir.join("curl");
    fs::write(
        &path,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> {}\ncase \"$*\" in\n  */api/federation/status*) printf '%s\\n' '{{\"node\":\"peer-node\",\"agents\":[\"remote\"]}}' ; exit 0 ;;\n  */api/identity*) printf '%s\\n' '{{\"node\":\"peer-node\",\"agents\":[\"remote\"]}}' ; exit 0 ;;\n  *) echo unexpected-url >&2; exit 55 ;;\nesac\n",
            shell_quote(&log.display().to_string())
        ),
    )
    .expect("curl");
    chmod_exec(&path);
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn seed_config(root: &Path) {
    fs::write(
        root.join("config/maw/maw.config.json"),
        r#"{"node":"local-node","agents":{"local-agent":"local"}}"#,
    )
    .expect("config");
    fs::write(
        root.join("state/peers.json"),
        r#"{"version":1,"peers":{"fakepeer":{"url":"http://peer.example:3456","node":"peer-node","addedAt":"1"}}}"#,
    )
    .expect("peers");
}

fn run_federation(root: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_maw-rs"));
    cmd.args(args)
        .env_clear()
        .env("PATH", root.join("bin"))
        .env("HOME", root)
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_STATE_HOME", root.join("state"))
        .env("MAW_CONFIG_DIR", root.join("config/maw"))
        .env("PEERS_FILE", root.join("state/peers.json"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run federation")
}

fn assert_no_delegation(output: &std::process::Output) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stdout.contains("DELEGATED-MAW"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "stderr={stderr}");
    assert!(!stdout.contains("DELEGATED-BUN"), "stdout={stdout}");
    assert!(!stderr.contains("DELEGATED-BUN"), "stderr={stderr}");
}

#[test]
fn federation_runtime_fake_maw_proof_status_and_sync() {
    assert_eq!(dispatcher_status("federation"), DispatchKind::Native);
    let root = temp_dir("runtime-proof");
    let bin_dir = root.join("bin");
    let curl_log = root.join("curl.log");
    write_fake_marker(&bin_dir, "maw", "DELEGATED-MAW");
    write_fake_marker(&bin_dir, "bun", "DELEGATED-BUN");
    write_fake_curl(&bin_dir, &curl_log);
    seed_config(&root);

    let status = run_federation(&root, &["federation", "status"]);
    assert!(
        status.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&status.stderr)
    );
    assert_no_delegation(&status);
    assert_eq!(
        String::from_utf8_lossy(&status.stdout),
        include_str!("fixtures/zerobun/federation-status.stdout")
    );

    let sync = run_federation(&root, &["federation", "sync", "--json"]);
    assert!(
        sync.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&sync.stderr)
    );
    assert_no_delegation(&sync);
    assert_eq!(
        String::from_utf8_lossy(&sync.stdout),
        include_str!("fixtures/zerobun/federation-sync-json.stdout")
    );

    let log = fs::read_to_string(&curl_log).expect("curl log");
    assert!(log.contains("/api/federation/status"), "{log}");
    assert!(log.contains("/api/identity"), "{log}");
    assert!(!log.contains("DELEGATED"), "{log}");
    let _ = fs::remove_dir_all(root);
}
