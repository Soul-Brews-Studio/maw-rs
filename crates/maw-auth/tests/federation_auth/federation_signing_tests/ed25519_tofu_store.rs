#[test]
fn verify_request_ed25519_accepts_existing_pin_without_repin_header() {
    use maw_auth::Ed25519TofuStore;
    use std::sync::{Arc, Mutex};

    let mut store = Ed25519TofuStore::default();
    assert!(store.pin_first_contact(FROM, ED25519_PUBKEY_HEX));
    let pins = Arc::new(Mutex::new(store));
    let decision = maw_auth::verify_request(&ed25519_request_parts(
        ED25519_SIG_HEX,
        None,
        pins,
    ));
    assert!(decision.is_accept(), "{decision:?}");
}

#[test]
fn verify_request_ed25519_rejects_pin_mismatch_without_silent_repin() {
    use maw_auth::Ed25519TofuStore;
    use std::sync::{Arc, Mutex};

    let other_key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let mut store = Ed25519TofuStore::default();
    assert!(store.pin_first_contact(FROM, other_key));
    let pins = Arc::new(Mutex::new(store));
    let decision = maw_auth::verify_request(&ed25519_request_parts(
        ED25519_SIG_HEX,
        Some(ED25519_PUBKEY_HEX),
        pins.clone(),
    ));
    assert_eq!(decision.reason(), Some("ed25519-pin-mismatch"));
    let guard = pins.lock().expect("test pin lock");
    assert_eq!(guard.pinned(FROM), Some(other_key));
}

#[test]
fn ed25519_tofu_file_backed_pin_persists_and_reloads_pubkey_only() {
    use maw_auth::Ed25519TofuStore;

    let path = ed25519_tofu_test_path("persist");
    let mut store = Ed25519TofuStore::file_backed(&path);
    assert!(store.pin_first_contact(FROM, ED25519_PUBKEY_HEX));
    assert_eq!(store.pinned(FROM), Some(ED25519_PUBKEY_HEX));

    let reloaded = Ed25519TofuStore::file_backed(&path);
    assert_eq!(reloaded.pinned(FROM), Some(ED25519_PUBKEY_HEX));
    let raw = std::fs::read_to_string(&path).expect("persisted tofu pins");
    assert!(raw.contains(FROM));
    assert!(raw.contains(ED25519_PUBKEY_HEX));
    assert!(!raw.contains("token"));
    assert!(!raw.contains("secret"));
}

#[test]
fn ed25519_tofu_reloaded_pin_rejects_mismatch_without_silent_repin() {
    use maw_auth::Ed25519TofuStore;
    use std::sync::{Arc, Mutex};

    let path = ed25519_tofu_test_path("mismatch");
    let mut store = Ed25519TofuStore::file_backed(&path);
    let other_key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    assert!(store.pin_first_contact(FROM, other_key));
    drop(store);

    let pins = Arc::new(Mutex::new(Ed25519TofuStore::file_backed(&path)));
    let decision = maw_auth::verify_request(&ed25519_request_parts(
        ED25519_SIG_HEX,
        Some(ED25519_PUBKEY_HEX),
        pins.clone(),
    ));
    assert_eq!(decision.reason(), Some("ed25519-pin-mismatch"));
    let guard = pins.lock().expect("test pin lock");
    assert_eq!(guard.pinned(FROM), Some(other_key));
}

#[test]
fn ed25519_tofu_corrupt_file_poison_rejects_without_auto_pin() {
    use maw_auth::Ed25519TofuStore;
    use std::sync::{Arc, Mutex};

    let path = ed25519_tofu_test_path("corrupt");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("test dir");
    std::fs::write(&path, b"{not-json").expect("corrupt fixture");

    let mut store = Ed25519TofuStore::file_backed(&path);
    assert!(store.is_poisoned());
    assert!(!store.pin_first_contact(FROM, ED25519_PUBKEY_HEX));

    let pins = Arc::new(Mutex::new(store));
    let decision = maw_auth::verify_request(&ed25519_request_parts(
        ED25519_SIG_HEX,
        Some(ED25519_PUBKEY_HEX),
        pins.clone(),
    ));
    assert_eq!(decision.reason(), Some("tofu-store-corrupt"));
    let guard = pins.lock().expect("test pin lock");
    assert!(guard.is_empty());
    assert!(guard.is_poisoned());
}

#[test]
fn ed25519_tofu_missing_file_still_allows_first_use_pin() {
    use maw_auth::Ed25519TofuStore;

    let path = ed25519_tofu_test_path("missing");
    assert!(!path.exists());
    let mut store = Ed25519TofuStore::file_backed(&path);
    assert!(!store.is_poisoned());
    assert!(store.pin_first_contact(FROM, ED25519_PUBKEY_HEX));

    let reloaded = Ed25519TofuStore::file_backed(&path);
    assert_eq!(reloaded.pinned(FROM), Some(ED25519_PUBKEY_HEX));
}

#[test]
fn ed25519_tofu_atomic_concurrent_pins_survive_reload() {
    use maw_auth::Ed25519TofuStore;
    use std::sync::{Arc, Mutex};

    let path = ed25519_tofu_test_path("concurrent");
    let pins = Arc::new(Mutex::new(Ed25519TofuStore::file_backed(&path)));
    let mut handles = Vec::new();
    for index in 0..8 {
        let pins = pins.clone();
        handles.push(std::thread::spawn(move || {
            let from = format!("mawjs:m{index}");
            let pubkey = format!("{index:064x}");
            let mut guard = pins.lock().expect("test pin lock");
            assert!(guard.pin_first_contact(&from, &pubkey));
        }));
    }
    for handle in handles {
        handle.join().expect("pin thread");
    }

    let reloaded = Ed25519TofuStore::file_backed(&path);
    assert_eq!(reloaded.len(), 8);
    let expected = format!("{:064x}", 7);
    assert_eq!(reloaded.pinned("mawjs:m7"), Some(expected.as_str()));
}

#[test]
fn ed25519_tofu_traversal_path_poisons_without_pin() {
    use maw_auth::Ed25519TofuStore;

    let path = ed25519_tofu_test_path("traversal")
        .parent()
        .expect("parent")
        .join("..")
        .join("escaped.json");
    let mut store = Ed25519TofuStore::file_backed(path);
    assert!(store.is_poisoned());
    assert!(!store.pin_first_contact(FROM, ED25519_PUBKEY_HEX));
    assert!(store.is_empty());
}

#[test]
fn verify_request_ed25519_rejects_bad_sig_and_malformed_inputs_fail_closed() {
    use maw_auth::Ed25519TofuStore;
    use std::sync::{Arc, Mutex};

    let pins = Arc::new(Mutex::new(Ed25519TofuStore::default()));
    let bad = maw_auth::verify_request(&ed25519_request_parts(
        &"0".repeat(128),
        Some(ED25519_PUBKEY_HEX),
        pins.clone(),
    ));
    assert_eq!(bad.reason(), Some("ed25519-signature-invalid"));
    assert!(pins.lock().expect("test pin lock").is_empty());

    let malformed = maw_auth::verify_request(&ed25519_request_parts(
        "base64-ed25519-placeholder",
        Some(ED25519_PUBKEY_HEX),
        pins,
    ));
    assert_eq!(malformed.reason(), Some("ed25519-signature-invalid"));
}

