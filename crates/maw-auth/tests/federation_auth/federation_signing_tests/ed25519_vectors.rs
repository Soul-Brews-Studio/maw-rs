const ED25519_PUBKEY_HEX: &str =
    "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664";
const ED25519_SIG_HEX: &str = concat!(
    "d232e00767facc77aca0eaaf2ebc18dc3c608639430f93167679805c7e3ccf69",
    "f15a856c7d8f4eddf64730cc61d4ccc0c28ca91b9a9df1a5016c628d737b3a0f"
);

fn ed25519_tofu_test_path(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("test clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "maw-auth-tofu-{label}-{}-{nanos}-{unique}/pins.json",
        std::process::id()
    ))
}

fn ed25519_request_parts(
    signature: &str,
    pubkey: Option<&str>,
    pins: maw_auth::Ed25519TofuPins,
) -> maw_auth::RequestAuthParts {
    use std::net::{IpAddr, Ipv4Addr};

    let body = b"{\"event\":\"agent-idle\"}".to_vec();
    let mut headers = vec![
        ("x-maw-from".to_owned(), FROM.to_owned()),
        ("x-maw-ed25519-signature".to_owned(), signature.to_owned()),
        ("x-maw-timestamp".to_owned(), NOW.to_string()),
        ("x-maw-auth-version".to_owned(), "ed25519".to_owned()),
    ];
    if let Some(pubkey) = pubkey {
        headers.push(("x-maw-ed25519-pubkey".to_owned(), pubkey.to_owned()));
    }
    maw_auth::RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new(headers),
        body: Some(body),
        peer_ip: Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10))),
        workspace_key: None,
        cached_pubkey: None,
        ed25519_pins: Some(pins),
        now: NOW,
    }
}

#[test]
fn verify_request_ed25519_from_sign_accepts_byte_exact_804_vector_and_pins_tofu() {
    use maw_auth::{build_from_sign_payload, hash_body, Ed25519TofuStore};
    use std::sync::{Arc, Mutex};

    let body = b"{\"event\":\"agent-idle\"}";
    assert_eq!(
        build_from_sign_payload(FROM, NOW, "POST", "/triggers/fire", &hash_body(Some(body))),
        "POST:/triggers/fire:1700000000:98e31c8f0c5f043066b34e52684d8c0a9bbc61e0393e4dbba1d644b04abb8878:mawjs:m5"
    );
    let pins = Arc::new(Mutex::new(Ed25519TofuStore::default()));
    let decision = maw_auth::verify_request(&ed25519_request_parts(
        ED25519_SIG_HEX,
        Some(ED25519_PUBKEY_HEX),
        pins.clone(),
    ));
    assert_eq!(
        decision,
        maw_auth::RequestAuthDecision::Accept {
            who: format!("ed25519:{FROM}")
        }
    );
    let guard = pins.lock().expect("test pin lock");
    assert_eq!(guard.pinned(FROM), Some(ED25519_PUBKEY_HEX));
}

#[test]
fn verify_request_ed25519_accepts_api_path_when_receiver_path_is_stripped() {
    use ed25519_dalek::{Signer, SigningKey};
    use maw_auth::{Ed25519TofuStore, RequestAuthParts};
    use std::{
        net::{IpAddr, Ipv4Addr},
        sync::{Arc, Mutex},
    };

    fn hex_lower(bytes: &[u8]) -> String {
        let mut out = String::with_capacity(bytes.len() * 2);
        for byte in bytes {
            use std::fmt::Write as _;
            write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
        }
        out
    }

    let body = b"{\"event\":\"ed25519-api-path\"}";
    let signing_key = SigningKey::from_bytes(&[7_u8; 32]);
    let verifying_key = signing_key.verifying_key();
    let pubkey_hex = hex_lower(verifying_key.as_bytes());
    let payload = build_from_sign_payload(
        FROM,
        NOW,
        "POST",
        "/api/triggers/fire",
        &hash_body(Some(body)),
    );
    let signature = signing_key.sign(payload.as_bytes());
    let signature_hex = hex_lower(&signature.to_bytes());
    let pins = Arc::new(Mutex::new(Ed25519TofuStore::default()));

    let decision = maw_auth::verify_request(&RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new([
            ("x-maw-from", FROM),
            ("x-maw-ed25519-signature", signature_hex.as_str()),
            ("x-maw-ed25519-pubkey", pubkey_hex.as_str()),
            ("x-maw-timestamp", &NOW.to_string()),
            ("x-maw-auth-version", "ed25519"),
        ]),
        body: Some(body.to_vec()),
        peer_ip: Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10))),
        workspace_key: None,
        cached_pubkey: None,
        ed25519_pins: Some(pins),
        now: NOW,
    });
    assert_eq!(
        decision,
        maw_auth::RequestAuthDecision::Accept {
            who: format!("ed25519:{FROM}")
        }
    );
}

