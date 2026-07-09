use std::{
    process::Command,
    sync::{Mutex, OnceLock},
};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn fake_discord() -> &'static str {
    r#"{
  "bot": "nova-oracle",
  "gateway_events": ["heartbeat", "heartbeat-ack"],
  "guilds": [
    {
      "id": "123456789012345678",
      "name": "Fleet Lab",
      "channels": [
        { "id": "222222222222222222", "name": "ops", "type": 0, "enabled": true, "requireMention": true, "allowFrom": ["111111111111111111"] },
        { "id": "333333333333333333", "name": "general", "type": 0, "enabled": false, "requireMention": true, "allowFrom": [] },
        { "id": "444444444444444444", "name": "thread", "type": 11, "enabled": true, "requireMention": false, "allowFrom": [] }
      ]
    }
  ]
}"#
}

#[test]
fn discord_inv_default_committed_golden_without_ref_checkout() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["discord-inv", "nova-oracle"])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_ATLAS_FAKE_DISCORD", fake_discord())
        .env("DISCORD_BOT_TOKEN", "mock-token-never-printed")
        .output()
        .expect("run discord-inv");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-atlas/atlas-default.stdout")
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(!stderr.contains("mock-token-never-printed"), "{stderr}");
}

#[test]
fn discord_inv_json_redacts_token_and_validates_guild_id() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args([
            "discord-inv",
            "nova-oracle",
            "--guild",
            "123456789012345678",
            "--json",
        ])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_ATLAS_FAKE_DISCORD", fake_discord())
        .env("DISCORD_BOT_TOKEN", "mock-token-never-printed")
        .output()
        .expect("run discord-inv json");
    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("stdout");
    assert!(stdout.contains("\"gatewayEvents\": 2"), "{stdout}");
    assert!(!stdout.contains("mock-token-never-printed"), "{stdout}");

    let rejected = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["discord-inv", "nova-oracle", "--guild", "abc"])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_RS_ATLAS_FAKE_DISCORD", fake_discord())
        .env("DISCORD_BOT_TOKEN", "mock-token-never-printed")
        .output()
        .expect("run discord-inv bad guild");
    assert!(!rejected.status.success());
    let stderr = String::from_utf8(rejected.stderr).expect("stderr");
    assert!(stderr.contains("invalid guild id"), "{stderr}");
    assert!(!stderr.contains("mock-token-never-printed"), "{stderr}");
}

fn atlas_temp_dir(label: &str) -> std::path::PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-atlas-compat-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn write_bun_shim(dir: &std::path::Path) {
    let shim = dir.join("bun");
    std::fs::write(
        &shim,
        "#!/bin/sh\nprintf 'legacy atlas plugin stdout\\n'\nprintf 'entry=%s\\n' \"$1\" > \"$ATLAS_BUN_ARGS\"\nshift\ni=0\nfor arg in \"$@\"; do printf 'arg%s=%s\\n' \"$i\" \"$arg\" >> \"$ATLAS_BUN_ARGS\"; i=$((i + 1)); done\n",
    )
    .expect("write bun");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&shim)
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&shim, permissions).expect("chmod bun");
    }
}

fn write_legacy_atlas_plugin(plugins_dir: &std::path::Path) {
    let package_dir = plugins_dir.join("legacy-atlas");
    std::fs::create_dir_all(&package_dir).expect("plugin dir");
    std::fs::write(
        package_dir.join("index.ts"),
        "export default async function plugin() {}\n",
    )
    .expect("entry");
    std::fs::write(
        package_dir.join("plugin.json"),
        r#"{"name":"legacy-atlas","version":"1.0.0","sdk":"*","runtime":"bun-dev","target":"js","entry":"index.ts","cli":{"command":"atlas","help":"maw atlas"}}"#,
    )
    .expect("manifest");
}

#[test]
fn atlas_namespace_is_owned_by_plugin_not_native_discord_inv() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = atlas_temp_dir("legacy");
    let bin_dir = root.join("bin");
    let plugins_dir = root.join("plugins");
    let bun_args = root.join("bun-args");
    std::fs::create_dir_all(&bin_dir).expect("bin dir");
    std::fs::create_dir_all(&plugins_dir).expect("plugins dir");
    write_bun_shim(&bin_dir);
    write_legacy_atlas_plugin(&plugins_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["atlas", "ls", "--json"])
        .env("PATH", &bin_dir)
        .env("MAW_PLUGINS_DIR", &plugins_dir)
        .env("ATLAS_BUN_ARGS", &bun_args)
        .env_remove("MAW_RS_ATLAS_FAKE_DISCORD")
        .output()
        .expect("run atlas legacy");

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        "legacy atlas plugin stdout\n"
    );
    let captured = std::fs::read_to_string(&bun_args).expect("bun args");
    assert!(captured.contains("arg0=ls"), "{captured}");
    assert!(captured.contains("arg1=--json"), "{captured}");
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("legacy-atlas"), "{stderr}");

    std::fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn atlas_namespace_without_plugin_fails_fast_instead_of_hanging() {
    let _guard = env_lock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = atlas_temp_dir("missing-plugin");
    let plugins_dir = root.join("plugins");
    std::fs::create_dir_all(&plugins_dir).expect("plugins dir");

    let started = std::time::Instant::now();
    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["atlas", "backfill"])
        .env("MAW_PLUGINS_DIR", &plugins_dir)
        .env_remove("MAW_RS_ATLAS_FAKE_DISCORD")
        .output()
        .expect("run atlas missing plugin");
    let elapsed = started.elapsed();

    assert!(!output.status.success());
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "atlas dispatch miss hung for {elapsed:?}"
    );
    let stderr = String::from_utf8(output.stderr).expect("stderr");
    assert!(stderr.contains("unknown command 'atlas'"), "{stderr}");

    std::fs::remove_dir_all(root).expect("cleanup");
}
