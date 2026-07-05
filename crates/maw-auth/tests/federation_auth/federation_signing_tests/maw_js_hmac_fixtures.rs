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

