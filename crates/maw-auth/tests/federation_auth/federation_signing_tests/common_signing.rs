use maw_auth::{
    build_from_sign_payload, build_legacy_from_sign_payload, hash_body, is_loopback,
    is_refuse_decision, resolve_from_address, resolve_sender_oracle, sign, sign_headers_at,
    sign_headers_v3_at, sign_hmac_sig, sign_request_v3, verify, verify_hmac_sig, verify_request,
    FromAddressConfig, FromVerifyDecision, Headers, VerifyRequestArgs, DEFAULT_ORACLE,
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
    let stacked = sign_headers_v3_at(TOKEN, PEER_KEY, FROM, "POST", "/api/send", None, NOW)
        .expect("v3 headers should sign");
    assert_eq!(stacked.get("X-Maw-Auth-Version"), Some("v3"));
    assert_eq!(
        stacked.get("X-Maw-Signature"),
        Some(direct_hmac(TOKEN, "POST:/api/send:1700000000").as_str())
    );
    assert!(stacked.get("X-Maw-Signature-V3").is_some());
    let get_default = sign_request_v3(PEER_KEY, FROM, "", "/api/send", NOW, None)
        .expect("empty method defaults to GET");
    assert_eq!(
        get_default.signature,
        direct_hmac(
            PEER_KEY,
            &build_from_sign_payload(FROM, NOW, "GET", "/api/send", "")
        )
    );
    assert!(sign_headers_v3_at(TOKEN, "", FROM, "POST", "/api/send", None, NOW).is_err());
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
fn sender_oracle_resolution_uses_invocation_window_before_config_default() {
    assert_eq!(
        resolve_sender_oracle(
            Some("33-maw-rs:maw-rs.0"),
            Some("tmux-window"),
            Some("configured")
        ),
        "maw-rs"
    );
    assert_eq!(
        resolve_sender_oracle(Some("maw-rs"), Some("tmux-window"), Some("configured")),
        "maw-rs"
    );
    assert_eq!(
        resolve_sender_oracle(Some("   "), Some("tmux-window.1"), Some("configured")),
        "tmux-window"
    );
    assert_eq!(
        resolve_sender_oracle(
            None,
            Some("homekeeper-oracle.wt-1-bridge"),
            Some("configured")
        ),
        "homekeeper-oracle.wt-1-bridge"
    );
    assert_eq!(
        resolve_sender_oracle(None, Some("  "), Some("configured")),
        "configured"
    );
    assert_eq!(resolve_sender_oracle(None, None, None), DEFAULT_ORACLE);
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
