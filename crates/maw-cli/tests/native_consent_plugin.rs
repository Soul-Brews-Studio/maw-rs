use maw_cli::{dispatcher_status, run_cli, DispatchKind};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvRestore {
    values: Vec<(&'static str, Option<OsString>)>,
}

impl EnvRestore {
    fn capture(keys: &[&'static str]) -> Self {
        Self {
            values: keys
                .iter()
                .map(|key| (*key, std::env::var_os(key)))
                .collect(),
        }
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.values.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "maw-rs-native-consent-{label}-{}-{nonce}",
        std::process::id()
    ));
    std::fs::create_dir_all(root.join("bin")).expect("bin");
    root
}

fn write_file(path: &Path, body: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, body).expect("write");
}

fn write_fake_maw(bin: &Path) {
    write_file(
        &bin.join("maw"),
        "#!/bin/sh\nprintf 'DELEGATED-MAW\\n'\nprintf 'args=%s\\n' \"$*\"\nexit 37\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(bin.join("maw"))
            .expect("shim metadata")
            .permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(bin.join("maw"), permissions).expect("chmod shim");
    }
}

fn setup_env(root: &Path) -> EnvRestore {
    let keys = [
        "HOME",
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_DATA_DIR",
        "MAW_STATE_DIR",
        "MAW_CACHE_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
        "CONSENT_TRUST_FILE",
        "CONSENT_PENDING_DIR",
        "MAW_FROM_RS",
        "MAW_PLUGINS_DIR",
        "PATH",
    ];
    let restore = EnvRestore::capture(&keys);
    write_fake_maw(&root.join("bin"));
    std::env::set_var("HOME", root.join("home"));
    std::env::remove_var("MAW_HOME");
    std::env::set_var("MAW_STATE_DIR", root.join("state"));
    std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
    std::env::set_var("MAW_PLUGINS_DIR", root.join("plugins"));
    std::env::set_var("PATH", root.join("bin"));
    std::env::remove_var("CONSENT_TRUST_FILE");
    std::env::remove_var("CONSENT_PENDING_DIR");
    std::env::remove_var("MAW_FROM_RS");
    std::fs::create_dir_all(root.join("plugins")).expect("plugins");
    restore
}

fn binary_command(root: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_maw-rs"));
    command
        .current_dir(root)
        .env_clear()
        .env("HOME", root.join("home"))
        .env("MAW_STATE_DIR", root.join("state"))
        .env("MAW_CONFIG_DIR", root.join("config"))
        .env("MAW_PLUGINS_DIR", root.join("plugins"))
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("PATH", root.join("bin"));
    command
}

#[test]
fn consent_empty_list_is_native_and_never_invokes_path_maw() {
    let _guard = env_lock().lock().expect("env lock");
    let root = temp_dir("empty");
    let _restore = setup_env(&root);

    let output = run_cli(&args(&["consent"]));

    assert_eq!(dispatcher_status("consent"), DispatchKind::Native);
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
    assert_eq!(
        output.stdout,
        include_str!("fixtures/zero-bun/consent-list-empty.stdout")
    );
    assert!(
        !output.stdout.contains("DELEGATED-MAW"),
        "{}",
        output.stdout
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn consent_list_and_list_trust_read_state_without_pin_hash_or_delegation() {
    let _guard = env_lock().lock().expect("env lock");
    let root = temp_dir("state");
    let _restore = setup_env(&root);
    write_file(
        &root.join("state/consent-pending/req-new.json"),
        r#"{
  "id": "req-new",
  "from": "alpha",
  "to": "local-node",
  "action": "hey",
  "summary": "please send a hello across the fleet with a deliberately long summary that should be truncated in list output",
  "pinHash": "SECRET_PIN_HASH_SHOULD_NOT_PRINT",
  "createdAt": "2026-05-18T00:00:00.000Z",
  "expiresAt": "2999-01-01T00:00:00.000Z",
  "status": "pending"
}"#,
    );
    write_file(
        &root.join("config/consent-pending/req-old.json"),
        r#"{
  "id": "req-old",
  "from": "beta",
  "to": "local-node",
  "action": "team-invite",
  "summary": "short summary",
  "pinHash": "LEGACY_SECRET_PIN_HASH_SHOULD_NOT_PRINT",
  "createdAt": "2026-05-17T00:00:00.000Z",
  "expiresAt": "2999-01-01T00:00:00.000Z",
  "status": "pending"
}"#,
    );
    write_file(&root.join("state/consent-pending/junk.json"), "{not json");
    write_file(
        &root.join("state/trust.json"),
        r#"{
  "version": 1,
  "trust": {
    "alpha→local-node:hey": {
      "from": "alpha",
      "to": "local-node",
      "action": "hey",
      "approvedAt": "2026-05-18T00:03:00.000Z",
      "approvedBy": "human",
      "requestId": "req-new"
    }
  }
}"#,
    );

    let pending = run_cli(&args(&["consent", "list"]));
    assert_eq!(pending.code, 0, "{}", pending.stderr);
    assert_eq!(
        pending.stdout,
        include_str!("fixtures/zero-bun/consent-list-populated.stdout")
    );
    assert!(
        !pending.stdout.contains("SECRET_PIN_HASH"),
        "{}",
        pending.stdout
    );
    assert!(
        !pending.stderr.contains("SECRET_PIN_HASH"),
        "{}",
        pending.stderr
    );
    assert!(
        !pending.stdout.contains("DELEGATED-MAW"),
        "{}",
        pending.stdout
    );

    let trust = run_cli(&args(&["consent", "list-trust"]));
    assert_eq!(trust.code, 0, "{}", trust.stderr);
    assert_eq!(
        trust.stdout,
        include_str!("fixtures/zero-bun/consent-list-trust-populated.stdout")
    );
    assert!(
        !trust.stdout.contains("SECRET_PIN_HASH"),
        "{}",
        trust.stdout
    );
    assert!(!trust.stdout.contains("DELEGATED-MAW"), "{}", trust.stdout);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn consent_runtime_fake_maw_proof_for_list_and_list_trust() {
    let root = temp_dir("runtime");
    write_fake_maw(&root.join("bin"));
    std::fs::create_dir_all(root.join("plugins")).expect("plugins");
    write_file(
        &root.join("state/trust.json"),
        r#"{"version":1,"trust":{"alpha→local-node:hey":{"from":"alpha","to":"local-node","action":"hey","approvedAt":"2026-05-18T00:03:00.000Z","approvedBy":"human","requestId":"req-new"}}}"#,
    );

    for args in [&["consent", "list"][..], &["consent", "list-trust"][..]] {
        let output = binary_command(&root)
            .args(args)
            .output()
            .expect("run maw-rs");
        assert!(
            output.status.success(),
            "stderr={}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).expect("stdout");
        let stderr = String::from_utf8(output.stderr).expect("stderr");
        assert!(!stdout.contains("DELEGATED-MAW"), "{stdout}");
        assert!(!stderr.contains("DELEGATED-MAW"), "{stderr}");
        assert!(!stderr.contains("failed to run maw fallback"), "{stderr}");
    }
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn consent_mutating_subcommands_are_refused_without_delegation() {
    let _guard = env_lock().lock().expect("env lock");
    let root = temp_dir("mutating");
    let _restore = setup_env(&root);

    let output = run_cli(&args(&["consent", "approve", "req-1", "123456"]));

    assert_eq!(dispatcher_status("consent"), DispatchKind::Native);
    assert_ne!(output.code, 0);
    assert!(output.stdout.is_empty());
    assert!(
        output.stderr.contains("request not found: req-1"),
        "{}",
        output.stderr
    );
    assert!(
        !output.stderr.contains("DELEGATED-MAW"),
        "{}",
        output.stderr
    );
    assert!(!root.join("state/trust.json").exists());

    let trust = run_cli(&args(&["consent", "trust", "peer"]));
    assert_ne!(trust.code, 0);
    assert!(trust.stderr.contains("not native in maw-rs ZERO-BUN B2"), "{}", trust.stderr);
    let _ = std::fs::remove_dir_all(root);
}
