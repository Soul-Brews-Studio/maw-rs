use maw_auth::{
    build_from_sign_payload, build_legacy_from_sign_payload, hash_body, is_loopback,
    is_refuse_decision, resolve_from_address, sign, sign_headers_at, sign_headers_v3_at,
    sign_hmac_sig, sign_request_v3, verify, verify_hmac_sig, verify_request, FromAddressConfig,
    FromVerifyDecision, Headers, VerifyRequestArgs, DEFAULT_ORACLE,
};

const TOKEN: &str = "0123456789abcdef-federation-token";
const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const NOW: i64 = 1_700_000_000;

fn direct_hmac(secret: &str, payload: &str) -> String {
    // sign() includes maw's colon payload shape, so use verify_hmac_sig round-trip
    // by deriving the expected from the implementation under test's public helper.
    let sig = sign_hmac_sig(secret, payload);
    assert_eq!(sig, maw_auth_private_hmac_for_tests(secret, payload));
    assert!(verify_hmac_sig(secret, payload, &sig));
    sig
}

fn maw_auth_private_hmac_for_tests(secret: &str, payload: &str) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
    mac.update(payload.as_bytes());
    let bytes = mac.finalize().into_bytes();
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        write!(&mut out, "{byte:02x}").expect("writing to String cannot fail");
    }
    out
}

#[test]
fn hashing_and_signing_helpers_cover_v1_v2_v3_and_validation_branches() {
    assert_eq!(hash_body(None), "");
    assert_eq!(hash_body(Some(b"")), "");
    assert_eq!(hash_body(Some(b"body")).len(), 64);

    let sig = sign(TOKEN, "POST", "/api/send", NOW, "");
    assert!(verify(TOKEN, "POST", "/api/send", NOW, &sig, "", NOW));
    assert!(!verify(
        TOKEN,
        "POST",
        "/api/send",
        NOW - 301,
        &sig,
        "",
        NOW
    ));
    assert!(!verify(TOKEN, "POST", "/api/send", NOW, "short", "", NOW));
    assert!(!verify(
        TOKEN,
        "POST",
        "/api/send",
        NOW,
        &"z".repeat(64),
        "",
        NOW
    ));

    assert!(is_loopback(Some("127.9.0.1")));
    assert!(is_loopback(Some("::1")));
    assert!(is_loopback(Some("localhost")));
    assert!(!is_loopback(None));

    let h1 = sign_headers_at(TOKEN, "GET", "/api/send", None, NOW);
    assert_eq!(h1.get("X-Maw-Auth-Version"), None);
    let h2 = sign_headers_at(TOKEN, "POST", "/api/send", Some(b"body"), NOW);
    assert_eq!(h2.get("X-Maw-Auth-Version"), Some("v2"));

    assert!(sign_request_v3("", FROM, "POST", "/api/send", NOW, None)
        .expect_err("missing peer key should throw")
        .contains("peerKey"));
    assert!(
        sign_request_v3(PEER_KEY, "", "POST", "/api/send", NOW, None)
            .expect_err("missing from address should throw")
            .contains("fromAddress")
    );
    let v3 = sign_request_v3(PEER_KEY, FROM, "post", "/api/send", NOW, Some(b"body"))
        .expect("valid v3 signing should work");
    assert_eq!(
        v3.signature,
        direct_hmac(
            PEER_KEY,
            &build_from_sign_payload(FROM, NOW, "POST", "/api/send", &hash_body(Some(b"body")))
        )
    );
    assert_eq!(
        sign_headers_v3_at(PEER_KEY, FROM, "POST", "/api/send", None, NOW)
            .expect("v3 headers should sign")
            .get("X-Maw-Auth-Version"),
        Some("v3")
    );
    let get_default = sign_request_v3(PEER_KEY, FROM, "", "/api/send", NOW, None)
        .expect("empty method defaults to GET");
    assert_eq!(
        get_default.signature,
        direct_hmac(
            PEER_KEY,
            &build_from_sign_payload(FROM, NOW, "GET", "/api/send", "")
        )
    );
    assert!(sign_headers_v3_at("", FROM, "POST", "/api/send", None, NOW).is_err());
    assert_eq!(
        resolve_from_address(&FromAddressConfig {
            oracle: None,
            node: Some("m5".to_owned())
        }),
        Some(format!("{DEFAULT_ORACLE}:m5"))
    );
    assert_eq!(
        resolve_from_address(&FromAddressConfig {
            oracle: Some("pulse".to_owned()),
            node: None
        }),
        None
    );
}

#[test]
fn sign_is_deterministic_and_sensitive_to_payload_fields() {
    let base = sign(TOKEN, "POST", "/api/send", NOW, "");
    assert_eq!(base.len(), 64);
    assert_eq!(base, sign(TOKEN, "POST", "/api/send", NOW, ""));
    assert_ne!(base, sign(TOKEN, "GET", "/api/send", NOW, ""));
    assert_ne!(base, sign(TOKEN, "POST", "/api/talk", NOW, ""));
    assert_ne!(base, sign(TOKEN, "POST", "/api/send", NOW + 1, ""));
    assert_ne!(
        base,
        sign("different-token-also-long", "POST", "/api/send", NOW, "")
    );
}

fn verify_req(headers: Headers, body: &[u8], cached_pubkey: Option<&str>) -> FromVerifyDecision {
    verify_request(&VerifyRequestArgs {
        method: "POST".to_owned(),
        path: "/api/send".to_owned(),
        headers,
        body: Some(body.to_vec()),
        cached_pubkey: cached_pubkey.map(str::to_owned),
        now: NOW,
    })
}

#[test]
fn verify_request_covers_o6_current_v3_decisions_and_malformed_cases() {
    assert_eq!(
        verify_req(Headers::new([] as [(&str, &str); 0]), b"", None),
        FromVerifyDecision::AcceptLegacy {
            reason: "no-cache-no-sig".to_owned()
        }
    );

    let signed = sign_headers_v3_at(PEER_KEY, FROM, "POST", "/api/send", Some(b"body"), NOW)
        .expect("v3 headers should sign");
    assert_eq!(
        verify_req(signed.clone(), b"body", None).kind(),
        "accept-tofu-record"
    );
    assert_eq!(
        verify_req(Headers::new([("x-maw-from", FROM)]), b"", Some(PEER_KEY)),
        FromVerifyDecision::RefuseUnsigned {
            reason: "cache-no-sig".to_owned(),
            from: Some(FROM.to_owned()),
        }
    );
    assert_eq!(
        verify_req(signed.clone(), b"body", Some(PEER_KEY)).kind(),
        "accept-verified"
    );
    assert_eq!(
        verify_req(signed.clone(), b"tampered", Some(PEER_KEY)).kind(),
        "refuse-mismatch"
    );
    assert_eq!(
        verify_req(
            Headers::new([
                ("X-Maw-From", FROM),
                (
                    "X-Maw-Signature-V3",
                    signed.get("x-maw-signature-v3").expect("sig")
                ),
                ("X-Maw-Timestamp", &(NOW - 301).to_string()),
            ]),
            b"body",
            Some(PEER_KEY),
        )
        .kind(),
        "refuse-skew"
    );
    assert_eq!(
        verify_req(
            Headers::new([
                ("x-maw-from", FROM),
                ("x-maw-signature-v3", &"0".repeat(64)),
                ("x-maw-timestamp", "nope"),
            ]),
            b"",
            Some(PEER_KEY),
        ),
        FromVerifyDecision::RefuseMalformed {
            reason: "invalid-timestamp".to_owned()
        }
    );
}

#[test]
fn verify_request_accepts_legacy_from_signing_and_identifies_refusals() {
    let iso = "2023-11-14T22:13:20.000Z";
    let legacy_payload =
        build_legacy_from_sign_payload(FROM, iso, "POST", "/api/send", &hash_body(Some(b"body")));
    let legacy_headers = Headers::new([
        ("x-maw-from", FROM),
        ("x-maw-signature", &direct_hmac(PEER_KEY, &legacy_payload)),
        ("x-maw-signed-at", iso),
    ]);
    let legacy = verify_req(legacy_headers, b"body", Some(PEER_KEY));
    assert_eq!(legacy.kind(), "accept-verified");
    assert!(!is_refuse_decision(&legacy));
    assert!(is_refuse_decision(&FromVerifyDecision::RefuseMismatch {
        reason: "signature-invalid".to_owned(),
        from: FROM.to_owned(),
    }));
}


#[test]
fn verify_request_accepts_loopback_real_ip_and_rejects_xff_spoof() {
    use maw_auth::{RequestAuthDecision, RequestAuthParts};
    use std::net::{IpAddr, Ipv4Addr};

    let loopback = maw_auth::verify_request(&RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new([] as [(&str, &str); 0]),
        body: None,
        peer_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        workspace_key: None,
        cached_pubkey: None,
        ed25519_pins: None,
        now: NOW,
    });
    assert_eq!(
        loopback,
        RequestAuthDecision::Accept {
            who: "loopback".to_owned()
        }
    );

    let spoof = maw_auth::verify_request(&RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new([("x-forwarded-for", "127.0.0.1")]),
        body: None,
        peer_ip: Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10))),
        workspace_key: None,
        cached_pubkey: None,
        ed25519_pins: None,
        now: NOW,
    });
    assert_eq!(spoof.reason(), Some("missing-credentials"));
}

fn maw_js_hmac_fixture() -> serde_json::Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/maw-js-hmac-v3-stacked.json"
    )))
    .expect("real maw-js hmac fixture json")
}

fn maw_js_hmac_request() -> maw_auth::RequestAuthParts {
    use maw_auth::RequestAuthParts;
    use std::net::{IpAddr, Ipv4Addr};

    let fixture = maw_js_hmac_fixture();
    let fleet = &fixture["fleetHeaders"];
    let v3 = &fixture["v3Headers"];
    RequestAuthParts {
        method: fixture["method"].as_str().expect("method").to_owned(),
        path: fixture["path"].as_str().expect("path").to_owned(),
        headers: Headers::new([
            ("x-maw-from", v3["X-Maw-From"].as_str().expect("from")),
            (
                "x-maw-signature",
                fleet["X-Maw-Signature"].as_str().expect("fleet sig"),
            ),
            (
                "x-maw-signature-v3",
                v3["X-Maw-Signature-V3"].as_str().expect("v3 sig"),
            ),
            (
                "x-maw-timestamp",
                v3["X-Maw-Timestamp"].as_str().expect("timestamp"),
            ),
            (
                "x-maw-auth-version",
                v3["X-Maw-Auth-Version"].as_str().expect("version"),
            ),
        ]),
        body: Some(fixture["body"].as_str().expect("body").as_bytes().to_vec()),
        peer_ip: Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10))),
        workspace_key: Some(fixture["token"].as_str().expect("token").to_owned()),
        cached_pubkey: None,
        ed25519_pins: None,
        now: fixture["timestamp"].as_i64().expect("timestamp"),
    }
}

#[test]
fn verify_request_accepts_real_maw_js_stacked_fleet_hmac_v3_fixture() {
    use maw_auth::hash_body;

    let fixture = maw_js_hmac_fixture();
    let request = maw_js_hmac_request();
    assert_eq!(
        hash_body(request.body.as_deref()),
        fixture["bodyHash"].as_str().expect("body hash")
    );
    assert_eq!(
        fixture["generatedBy"].as_str().expect("proof"),
        "read-only import of real maw-js federation-auth.ts signing functions; no daemon/comms"
    );
    let accepted = maw_auth::verify_request(&request);
    assert!(accepted.is_accept(), "{accepted:?}");
}

#[test]
fn verify_request_rejects_wrong_token_pin_mismatch_and_expired_real_maw_js_fixture() {
    let base = maw_js_hmac_request();

    let mut wrong_token = base.clone();
    wrong_token.workspace_key = Some("wrong-federation-token".to_owned());
    assert_eq!(
        maw_auth::verify_request(&wrong_token).reason(),
        Some("signature-invalid")
    );

    let fixture = maw_js_hmac_fixture();
    let mut pin_mismatch = base.clone();
    pin_mismatch.cached_pubkey = Some("wrong-peer-key".to_owned());
    assert_eq!(
        maw_auth::verify_request(&pin_mismatch).reason(),
        Some("pin-mismatch")
    );

    let mut pinned = base.clone();
    pinned.cached_pubkey = Some(fixture["peerKey"].as_str().expect("peer key").to_owned());
    assert!(maw_auth::verify_request(&pinned).is_accept());

    let mut expired = base.clone();
    expired.now += 301;
    assert_eq!(
        maw_auth::verify_request(&expired).reason(),
        Some("timestamp-out-of-window")
    );

    let mut replay = base.clone();
    let mut headers = replay.headers.to_btree_map();
    headers.insert("x-maw-timestamp".to_owned(), (replay.now + 301).to_string());
    replay.headers = Headers::new(headers);
    assert_eq!(
        maw_auth::verify_request(&replay).reason(),
        Some("timestamp-out-of-window")
    );
}

#[test]
fn verify_request_from_sign_accepts_real_maw_js_api_path_when_receiver_path_is_stripped() {
    let fixture = maw_js_hmac_fixture();
    let mut stripped = maw_js_hmac_request();
    stripped.path = "/triggers/fire".to_owned();
    stripped.workspace_key = None;
    stripped.cached_pubkey = Some(fixture["peerKey"].as_str().expect("peer key").to_owned());

    assert_eq!(
        maw_auth::verify_request(&stripped),
        maw_auth::RequestAuthDecision::Accept {
            who: "from-sign:nova:codex4".to_owned()
        }
    );

    let mut wrong_key = stripped.clone();
    wrong_key.cached_pubkey = Some("wrong-peer-key-393av3".to_owned());
    assert_eq!(
        maw_auth::verify_request(&wrong_key).reason(),
        Some("pin-mismatch")
    );
}

#[test]
fn verify_request_supports_fleet_v1_v2_and_legacy_newline_without_slot_conflation() {
    use maw_auth::{build_legacy_from_sign_payload, hash_body, sign_hmac_sig, RequestAuthParts};
    use std::net::{IpAddr, Ipv4Addr};

    let body = b"{\"event\":\"agent-idle\"}";
    let body_hash = hash_body(Some(body));
    let peer = Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10)));

    let v2_sig = sign(TOKEN, "POST", "/api/triggers/fire", NOW, &body_hash);
    let v2 = RequestAuthParts {
        method: "POST".to_owned(),
        path: "/api/triggers/fire".to_owned(),
        headers: Headers::new([
            ("x-maw-signature", v2_sig.as_str()),
            ("x-maw-timestamp", &NOW.to_string()),
            ("x-maw-auth-version", "v2"),
        ]),
        body: Some(body.to_vec()),
        peer_ip: peer,
        workspace_key: Some(TOKEN.to_owned()),
        cached_pubkey: None,
        ed25519_pins: None,
        now: NOW,
    };
    assert!(maw_auth::verify_request(&v2).is_accept());

    let legacy_signed_at = "2023-11-14T22:13:20.000Z";
    let legacy_payload = build_legacy_from_sign_payload(
        FROM,
        legacy_signed_at,
        "POST",
        "/api/triggers/fire",
        &body_hash,
    );
    let legacy_sig = sign_hmac_sig(PEER_KEY, &legacy_payload);
    let legacy = RequestAuthParts {
        method: "POST".to_owned(),
        path: "/api/triggers/fire".to_owned(),
        headers: Headers::new([
            ("x-maw-from", FROM),
            ("x-maw-signature", legacy_sig.as_str()),
            ("x-maw-signed-at", legacy_signed_at),
            ("x-maw-auth-version", "v3"),
        ]),
        body: Some(body.to_vec()),
        peer_ip: peer,
        workspace_key: None,
        cached_pubkey: Some(PEER_KEY.to_owned()),
        ed25519_pins: None,
        now: NOW,
    };
    assert!(maw_auth::verify_request(&legacy).is_accept());
}

#[test]
fn verify_request_legacy_from_sign_accepts_api_path_when_receiver_path_is_stripped() {
    use maw_auth::RequestAuthParts;
    use std::net::{IpAddr, Ipv4Addr};

    let body = b"{\"event\":\"legacy\"}";
    let body_hash = hash_body(Some(body));
    let signed_at = "2023-11-14T22:13:20.000Z";
    let payload =
        build_legacy_from_sign_payload(FROM, signed_at, "POST", "/api/triggers/fire", &body_hash);
    let signature = sign_hmac_sig(PEER_KEY, &payload);
    let decision = maw_auth::verify_request(&RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new([
            ("x-maw-from", FROM),
            ("x-maw-signature", signature.as_str()),
            ("x-maw-signed-at", signed_at),
            ("x-maw-auth-version", "v3"),
        ]),
        body: Some(body.to_vec()),
        peer_ip: Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10))),
        workspace_key: None,
        cached_pubkey: Some(PEER_KEY.to_owned()),
        ed25519_pins: None,
        now: NOW,
    });
    assert!(decision.is_accept(), "{decision:?}");
}

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

// Ported from maw-js `test/scout-pair-proof.test.ts` and
// `src/transports/scout-pair-proof.ts`.
#[test]
fn auto_pair_proofs_sign_stable_canonical_identity_fields() {
    use maw_auth::{sign_auto_pair_proof, AutoPairIdentity};

    let identity = AutoPairIdentity {
        node: "m5".to_owned(),
        oracle: "mawjs".to_owned(),
        url: "http://m5.local:3456".to_owned(),
        pubkey: "pub-abc".to_owned(),
    };

    let proof = sign_auto_pair_proof(&identity, "token-a");
    assert_eq!(proof.len(), 64);
    assert!(proof.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(sign_auto_pair_proof(&identity.clone(), "token-a"), proof);
    assert_ne!(
        sign_auto_pair_proof(
            &AutoPairIdentity {
                node: "other".to_owned(),
                ..identity
            },
            "token-a",
        ),
        proof
    );
}

// Ported from maw-js `test/scout-pair-proof.test.ts` and
// `src/transports/scout-pair-proof.ts`.
#[test]
fn auto_pair_proofs_verify_valid_proofs_and_reject_wrong_inputs() {
    use maw_auth::{sign_auto_pair_proof, verify_auto_pair_proof, AutoPairIdentity};

    let identity = AutoPairIdentity {
        node: "m5".to_owned(),
        oracle: "mawjs".to_owned(),
        url: "http://m5.local:3456".to_owned(),
        pubkey: "pub-abc".to_owned(),
    };
    let proof = sign_auto_pair_proof(&identity, "token-a");

    assert!(verify_auto_pair_proof(&identity, "token-a", &proof));
    assert!(!verify_auto_pair_proof(&identity, "token-b", &proof));
    assert!(!verify_auto_pair_proof(
        &AutoPairIdentity {
            pubkey: "pub-other".to_owned(),
            ..identity.clone()
        },
        "token-a",
        &proof,
    ));
    assert!(!verify_auto_pair_proof(&identity, "token-a", &proof[2..]));
    assert!(!verify_auto_pair_proof(
        &identity,
        "token-a",
        &"z".repeat(64)
    ));
}

// Ported from maw-js `src/lib/pair-codes.ts` and `test/pair-api-default.test.ts`.
#[test]
fn pair_code_helpers_match_maw_js_shape_format_and_redaction() {
    use maw_auth::{
        generate_pair_code_from_bytes, is_valid_pair_code_shape, normalize_pair_code,
        pretty_pair_code, redact_pair_code, PAIR_CODE_ALPHABET,
    };

    assert_eq!(normalize_pair_code("abc-234"), "ABC234");
    assert_eq!(normalize_pair_code(" ab c-2 34\n"), "ABC234");
    assert!(is_valid_pair_code_shape("ABC-234"));
    assert!(is_valid_pair_code_shape("abc234"));
    assert!(!is_valid_pair_code_shape("ABCDE"));
    assert!(!is_valid_pair_code_shape("ABCDEFG"));
    assert!(!is_valid_pair_code_shape("ABCDE0"));
    assert!(!is_valid_pair_code_shape("ABCDE1"));
    assert!(!is_valid_pair_code_shape("ABCDEI"));
    assert!(!is_valid_pair_code_shape("ABCDEO"));

    assert_eq!(pretty_pair_code("abc234"), "ABC-234");
    assert_eq!(pretty_pair_code("bad"), "BAD");
    assert_eq!(redact_pair_code("abc234"), "ABC-***");
    assert_eq!(redact_pair_code("ab"), "***");

    let code = generate_pair_code_from_bytes(&[0, 1, 31, 32, 33, 255]);
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|ch| PAIR_CODE_ALPHABET.contains(ch)));
    assert_eq!(code, "AB9AB9");
}

// Ported from maw-js `src/lib/pair-codes.ts` and `test/pair-api-default.test.ts`.
#[test]
fn pair_code_store_register_lookup_and_consume_match_maw_js_ttl_contract() {
    use maw_auth::{LookupResult, PairCodeStore};

    let mut store = PairCodeStore::default();
    let entry = store.register_at("abc-234", 120_000, 1_000_000);
    assert_eq!(entry.code, "ABC234");
    assert_eq!(entry.created_at, 1_000_000);
    assert_eq!(entry.expires_at, 1_120_000);
    assert!(!entry.consumed);

    assert_eq!(
        store.lookup_at("ABC234", 1_000_000),
        LookupResult::Live(entry)
    );
    assert_eq!(store.lookup_at("ZZZ999", 1_000_000), LookupResult::NotFound);
    assert_eq!(store.lookup_at("ABC234", 1_120_001), LookupResult::Expired);

    let consumed = store.consume_at("abc 234", 1_000_001);
    assert!(matches!(consumed, LookupResult::Live(_)));
    assert_eq!(
        store.lookup_at("ABC-234", 1_000_002),
        LookupResult::Consumed
    );
    assert_eq!(
        store.consume_at("ABC234", 1_000_003),
        LookupResult::Consumed
    );
}

// Ported from maw-js `src/core/consent/pin.ts` and
// `test/core/consent/consent.test.ts`.
#[test]
fn consent_pin_hash_and_verify_match_maw_js_normalized_shape_contract() {
    use maw_auth::{hash_consent_pin, verify_consent_pin};

    let h1 = hash_consent_pin("ABC-DEF");
    let h2 = hash_consent_pin("abcdef");
    let h3 = hash_consent_pin("ABCDEF");
    assert_eq!(h1, h2);
    assert_eq!(h2, h3);
    assert_eq!(h1.len(), 64);
    assert!(h1.chars().all(|ch| ch.is_ascii_hexdigit()));

    assert!(verify_consent_pin("ABC-DEF", &h1));
    assert!(verify_consent_pin("abcdef", &h1));
    assert!(!verify_consent_pin("BBBBBB", &h1));
    assert!(!verify_consent_pin("ABCDE", &h1));
    assert!(!verify_consent_pin("ABCDEFG", &h1));
    assert!(!verify_consent_pin("ABCDE0", &h1));
}

// Ported from maw-js `src/core/consent/store.ts` and
// `test/core/consent/consent.test.ts` trust/pending store cases.
#[test]
fn consent_trust_store_matches_maw_js_key_asymmetry_and_sorting() {
    use maw_auth::{trust_key, ApprovedBy, ConsentAction, ConsentStore, TrustEntry};

    assert_eq!(trust_key("a", "b", ConsentAction::Hey), "a→b:hey");

    let mut store = ConsentStore::default();
    assert!(!store.is_trusted("a", "b", ConsentAction::Hey));

    store.record_trust(TrustEntry {
        from: "a".to_owned(),
        to: "b".to_owned(),
        action: ConsentAction::Hey,
        approved_at: "2026-01-02".to_owned(),
        approved_by: ApprovedBy::Human,
        request_id: Some("r1".to_owned()),
    });
    store.record_trust(TrustEntry {
        from: "c".to_owned(),
        to: "d".to_owned(),
        action: ConsentAction::Hey,
        approved_at: "2026-01-01".to_owned(),
        approved_by: ApprovedBy::Human,
        request_id: None,
    });

    assert!(store.is_trusted("a", "b", ConsentAction::Hey));
    assert!(!store.is_trusted("b", "a", ConsentAction::Hey));
    assert!(!store.is_trusted("a", "b", ConsentAction::TeamInvite));
    assert_eq!(
        store
            .list_trust()
            .into_iter()
            .map(|entry| entry.from)
            .collect::<Vec<_>>(),
        vec!["c", "a"]
    );
    assert!(store.remove_trust("a", "b", ConsentAction::Hey));
    assert!(!store.is_trusted("a", "b", ConsentAction::Hey));
    assert!(!store.remove_trust("a", "b", ConsentAction::Hey));
}

// Ported from maw-js `src/core/consent/store.ts` and
// `test/core/consent/consent.test.ts` trust/pending store cases.
#[test]
fn consent_pending_store_matches_maw_js_status_expiry_and_ordering() {
    use maw_auth::{
        apply_consent_expiry, ConsentAction, ConsentStatus, ConsentStore, PendingRequest,
    };

    let pending = PendingRequest {
        id: "abc".to_owned(),
        from: "neo".to_owned(),
        to: "mawjs".to_owned(),
        action: ConsentAction::Hey,
        summary: "test".to_owned(),
        pin_hash: "hash".to_owned(),
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        expires_at: "2026-01-02T00:01:00.000Z".to_owned(),
        status: ConsentStatus::Pending,
    };
    assert_eq!(
        apply_consent_expiry(&pending, 1_767_312_061_000).status,
        ConsentStatus::Expired
    );
    assert_eq!(
        apply_consent_expiry(
            &PendingRequest {
                status: ConsentStatus::Approved,
                ..pending.clone()
            },
            1_767_312_061_000
        )
        .status,
        ConsentStatus::Approved
    );

    let mut store = ConsentStore::default();
    store.write_pending(pending.clone());
    store.write_pending(PendingRequest {
        id: "newer".to_owned(),
        created_at: "2026-01-03T00:00:00.000Z".to_owned(),
        ..pending.clone()
    });

    assert_eq!(store.read_pending("abc").expect("pending").id, "abc");
    assert_eq!(
        store
            .list_pending()
            .into_iter()
            .map(|req| req.id)
            .collect::<Vec<_>>(),
        vec!["newer", "abc"]
    );
    assert!(store.update_status("abc", ConsentStatus::Rejected));
    assert_eq!(
        store.read_pending("abc").expect("updated").status,
        ConsentStatus::Rejected
    );
    assert!(!store.update_status("missing", ConsentStatus::Approved));
    assert!(store.delete_pending("abc"));
    assert!(store.read_pending("abc").is_none());
    assert!(!store.delete_pending("abc"));
}

// Ported from maw-js `src/core/consent/request.ts` and
// `test/core/consent/consent.test.ts` request/approve/reject cases.
#[test]
fn consent_request_plan_mirrors_pending_and_models_peer_post_failures() {
    use maw_auth::{
        request_consent_plan, ConsentAction, ConsentRequestArgs, ConsentStore, PeerPostResult,
    };

    let mut store = ConsentStore::default();
    let ok = request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            from: "neo".to_owned(),
            to: "mawjs".to_owned(),
            action: ConsentAction::Hey,
            summary: "hello".to_owned(),
            peer_url: None,
            request_id: "00112233445566778899aabb".to_owned(),
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::Skipped,
        },
    );
    assert!(ok.ok);
    assert_eq!(ok.pin.as_deref(), Some("ABCDEF"));
    assert_eq!(ok.request_id.as_deref(), Some("00112233445566778899aabb"));
    assert_eq!(
        store
            .read_pending("00112233445566778899aabb")
            .expect("pending")
            .summary,
        "hello"
    );

    let peer_ok = request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            from: "neo".to_owned(),
            to: "mawjs".to_owned(),
            action: ConsentAction::Hey,
            summary: "peer ok".to_owned(),
            peer_url: Some("http://peer:3456/".to_owned()),
            request_id: "req-peer-ok".to_owned(),
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::Ok,
        },
    );
    assert!(peer_ok.ok);
    assert_eq!(peer_ok.peer_method.as_deref(), Some("POST"));
    assert_eq!(
        peer_ok.peer_url.as_deref(),
        Some("http://peer:3456/api/consent/request")
    );

    let mut store = ConsentStore::default();
    let posted = request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            from: "neo".to_owned(),
            to: "mawjs".to_owned(),
            action: ConsentAction::Hey,
            summary: "hi".to_owned(),
            peer_url: Some("http://peer:3456".to_owned()),
            request_id: "req-http".to_owned(),
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::HttpStatus(500),
        },
    );
    assert!(!posted.ok);
    assert_eq!(
        posted.peer_url.as_deref(),
        Some("http://peer:3456/api/consent/request")
    );
    assert_eq!(posted.peer_method.as_deref(), Some("POST"));
    assert!(posted.peer_body.as_ref().expect("body").pin.is_none());
    assert!(posted.error.as_deref().expect("error").contains("500"));
    assert!(store.read_pending("req-http").is_some());

    let network = request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            from: "neo".to_owned(),
            to: "mawjs".to_owned(),
            action: ConsentAction::Hey,
            summary: "hi".to_owned(),
            peer_url: Some("http://peer:3456".to_owned()),
            request_id: "req-network".to_owned(),
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::NetworkError("ECONNREFUSED".to_owned()),
        },
    );
    assert!(!network.ok);
    assert!(network
        .error
        .as_deref()
        .expect("error")
        .contains("ECONNREFUSED"));
}

// Ported from maw-js `src/core/consent/request.ts` and
// `test/core/consent/consent.test.ts` request/approve/reject cases.
