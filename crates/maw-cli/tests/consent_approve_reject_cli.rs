use maw_cli::run_cli;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Seed one pending request (id `req-1`, pin `ABC234`) in a fresh tempdir and
/// point `CONSENT_PENDING_DIR` / `CONSENT_TRUST_FILE` at it (caller holds the lock).
fn seed(label: &str, action: &str, expires_at: &str) -> (PathBuf, PathBuf) {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).expect("time").as_nanos();
    let root = std::env::temp_dir().join(format!("maw-rs-consent-native-{label}-{}-{nonce}", std::process::id()));
    let pending_dir = root.join("consent-pending");
    std::fs::create_dir_all(&pending_dir).expect("pending dir");
    let pin_hash = maw_auth::hash_consent_pin("ABC234");
    let pending_file = pending_dir.join("req-1.json");
    std::fs::write(&pending_file, format!(
        r#"{{"id":"req-1","from":"alpha","to":"local-node","action":"{action}","summary":"recruit alpha","pinHash":"{pin_hash}","createdAt":"2026-01-02T00:00:00.000Z","expiresAt":"{expires_at}","status":"pending"}}"#
    )).expect("seed pending");
    std::env::set_var("CONSENT_PENDING_DIR", &pending_dir);
    std::env::set_var("CONSENT_TRUST_FILE", root.join("trust.json"));
    (root, pending_file)
}

fn read_json(path: &std::path::Path) -> Value {
    serde_json::from_str(&std::fs::read_to_string(path).expect("read")).expect("json")
}

#[test]
fn consent_approve_verifies_pin_writes_trust_entry_and_marks_approved() {
    let _guard = env_lock().lock().expect("env lock");
    let (root, pending_file) = seed("approve", "fleet-recruit", "2999-01-01T00:00:00.000Z");

    let wrong = run_cli(&args(&["consent", "approve", "req-1", "BBBBBB"]));
    assert_ne!(wrong.code, 0);
    assert!(wrong.stderr.contains("PIN mismatch"), "{}", wrong.stderr);

    let output = run_cli(&args(&["consent", "approve", "req-1", "ABC234"]));
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(output.stdout.contains("approved req-1"), "{}", output.stdout);
    assert_eq!(read_json(&pending_file)["status"], "approved");
    let entry = &read_json(&root.join("trust.json"))["trust"]["alpha→local-node:fleet-recruit"];
    assert_eq!(entry["action"], "fleet-recruit");
    assert_eq!(entry["approvedBy"], "human");
    assert_eq!(entry["requestId"], "req-1");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn consent_reject_marks_request_rejected_without_trust_entry() {
    let _guard = env_lock().lock().expect("env lock");
    let (root, pending_file) = seed("reject", "hey", "2999-01-01T00:00:00.000Z");

    let output = run_cli(&args(&["consent", "reject", "req-1"]));
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(read_json(&pending_file)["status"], "rejected");
    assert!(!root.join("trust.json").exists());
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn consent_approve_refuses_expired_request_and_writes_no_trust() {
    let _guard = env_lock().lock().expect("env lock");
    let (root, pending_file) = seed("expired", "fleet-recruit", "2020-01-01T00:00:00.000Z");

    let output = run_cli(&args(&["consent", "approve", "req-1", "ABC234"]));
    assert_ne!(output.code, 0);
    assert!(output.stderr.contains("expired"), "{}", output.stderr);
    assert!(!root.join("trust.json").exists());
    assert_eq!(read_json(&pending_file)["status"], "pending");
    let _ = std::fs::remove_dir_all(root);
}
