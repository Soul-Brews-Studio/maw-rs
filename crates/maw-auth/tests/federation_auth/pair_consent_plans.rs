#[test]
fn consent_approve_and_reject_plans_match_maw_js_state_machine() {
    use maw_auth::{
        approve_consent_plan, reject_consent_plan, request_consent_plan, ConsentAction,
        ConsentRequestArgs, ConsentStatus, ConsentStore, PeerPostResult,
    };

    let mut store = ConsentStore::default();
    let req = ConsentRequestArgs {
        from: "neo".to_owned(),
        to: "mawjs".to_owned(),
        action: ConsentAction::Hey,
        summary: "x".to_owned(),
        peer_url: None,
        request_id: "req-ok".to_owned(),
        pin: "ABCDEF".to_owned(),
        now_ms: 1_767_312_000_000,
        peer_post: PeerPostResult::Skipped,
    };
    request_consent_plan(&mut store, req);
    let approved = approve_consent_plan(&mut store, "req-ok", "ABCDEF", 1_767_312_001_000);
    assert!(approved.ok);
    assert_eq!(approved.entry.as_ref().expect("entry").from, "neo");
    assert!(store.is_trusted("neo", "mawjs", ConsentAction::Hey));
    assert_eq!(
        store.read_pending("req-ok").expect("pending").status,
        ConsentStatus::Approved
    );

    assert!(!approve_consent_plan(&mut store, "missing", "ABCDEF", 1_767_312_001_000).ok);
    let second = approve_consent_plan(&mut store, "req-ok", "ABCDEF", 1_767_312_001_000);
    assert!(!second.ok);
    assert!(second.error.as_deref().expect("error").contains("approved"));

    let mut store = ConsentStore::default();
    request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            request_id: "req-bad-pin".to_owned(),
            from: "a".to_owned(),
            to: "b".to_owned(),
            action: ConsentAction::Hey,
            summary: "x".to_owned(),
            peer_url: None,
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::Skipped,
        },
    );
    let bad_pin = approve_consent_plan(&mut store, "req-bad-pin", "ZZZZZZ", 1_767_312_001_000);
    assert!(!bad_pin.ok);
    assert!(bad_pin.error.as_deref().expect("error").contains("PIN"));
    assert!(!store.is_trusted("a", "b", ConsentAction::Hey));

    let rejected = reject_consent_plan(&mut store, "req-bad-pin");
    assert!(rejected.ok);
    assert_eq!(
        store.read_pending("req-bad-pin").expect("pending").status,
        ConsentStatus::Rejected
    );
    assert!(!store.is_trusted("a", "b", ConsentAction::Hey));
}

// Ported from maw-js `src/core/consent/request.ts` and
// `test/core/consent/consent.test.ts` newRequestId cases.
#[test]
fn consent_request_id_generation_matches_maw_js_24_hex_contract() {
    use maw_auth::consent_request_id_from_bytes;

    let id = consent_request_id_from_bytes(&[0, 1, 2, 10, 15, 16, 31, 127, 128, 200, 254, 255]);
    assert_eq!(id, "0001020a0f101f7f80c8feff");
    assert_eq!(id.len(), 24);
    assert!(id
        .chars()
        .all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase()));
    assert_eq!(
        consent_request_id_from_bytes(&[0xab; 20]),
        "abababababababababababab"
    );
}

// Ported from maw-js `test/pair-api-default.test.ts` generate/probe cases
// and `src/api/pair.ts` route decision behavior.
#[test]
fn pair_api_generate_and_probe_plans_match_maw_js_status_contract() {
    use maw_auth::{pair_api_generate_plan, pair_api_probe_plan, PairApiConfig, PairCodeStore};

    let config = PairApiConfig {
        node: "node-a".to_owned(),
        oracle: "oracle-a".to_owned(),
        port: 4567,
        base_url: "http://localhost:4567".to_owned(),
        federation_token: "token-a".to_owned(),
        pubkey: "p".repeat(64),
    };
    let mut store = PairCodeStore::default();

    let generated = pair_api_generate_plan(&mut store, &config, "ABC234", None, None, 1_000_000);
    assert_eq!(generated.status, 201);
    assert!(generated.ok);
    assert_eq!(generated.code, "ABC-234");
    assert_eq!(generated.ttl_ms, 120_000);
    assert_eq!(generated.expires_at, 1_120_000);
    assert_eq!(generated.node, "node-a");
    assert_eq!(generated.port, 4567);

    let generated = pair_api_generate_plan(&mut store, &config, "ABC234", Some(5), None, 1_000_000);
    assert_eq!(generated.ttl_ms, 5_000);
    assert_eq!(generated.expires_at, 1_005_000);

    let generated =
        pair_api_generate_plan(&mut store, &config, "ABC234", None, Some(42), 1_000_000);
    assert_eq!(generated.ttl_ms, 42);
    assert_eq!(generated.expires_at, 1_000_042);

    let invalid = pair_api_probe_plan(&store, &config, "bad", 1_000_000);
    assert_eq!(invalid.status, 400);
    assert_eq!(invalid.error.as_deref(), Some("invalid_shape"));

    let missing = pair_api_probe_plan(&store, &config, "ZZZ999", 1_000_000);
    assert_eq!(missing.status, 404);
    assert_eq!(missing.error.as_deref(), Some("not_found"));

    let _ = store.register_at("DEF456", 1, 0);
    let expired = pair_api_probe_plan(&store, &config, "DEF456", 1_000_000);
    assert_eq!(expired.status, 410);
    assert_eq!(expired.error.as_deref(), Some("expired"));

    let live = pair_api_probe_plan(&store, &config, "ABC234", 1_000_000);
    assert_eq!(live.status, 200);
    assert!(live.ok);
    assert_eq!(live.node.as_deref(), Some("node-a"));
}

// Ported from maw-js `test/pair-api-default.test.ts` pair POST/status cases
// and `src/api/pair.ts` consumed-code behavior.
#[test]
fn pair_api_accept_and_status_plans_match_maw_js_consumed_contract() {
    use maw_auth::{
        pair_api_accept_plan, pair_api_status_plan, PairAcceptInput, PairApiConfig, PairCodeStore,
    };

    let config = PairApiConfig {
        node: "node-a".to_owned(),
        oracle: "oracle-a".to_owned(),
        port: 4567,
        base_url: "http://localhost:4567".to_owned(),
        federation_token: "ab".repeat(32),
        pubkey: "p".repeat(64),
    };
    let mut store = PairCodeStore::default();

    let invalid = pair_api_accept_plan(&mut store, &config, "bad", None, 1_000_000);
    assert_eq!(invalid.status, 400);
    assert_eq!(invalid.error.as_deref(), Some("invalid_shape"));

    let bad = pair_api_accept_plan(
        &mut store,
        &config,
        "ABC234",
        Some(PairAcceptInput {
            node: "remote".to_owned(),
            url: None,
        }),
        1_000_000,
    );
    assert_eq!(bad.status, 400);
    assert_eq!(bad.error.as_deref(), Some("bad_request"));

    let missing = pair_api_accept_plan(
        &mut store,
        &config,
        "ABC234",
        Some(PairAcceptInput {
            node: "remote".to_owned(),
            url: Some("http://remote".to_owned()),
        }),
        1_000_000,
    );
    assert_eq!(missing.status, 404);
    assert_eq!(missing.error.as_deref(), Some("not_found"));

    let _ = store.register_at("DEF456", 1, 0);
    let expired = pair_api_accept_plan(
        &mut store,
        &config,
        "DEF456",
        Some(PairAcceptInput {
            node: "remote".to_owned(),
            url: Some("http://remote".to_owned()),
        }),
        1_000_000,
    );
    assert_eq!(expired.status, 410);
    assert_eq!(expired.error.as_deref(), Some("expired"));

    let _ = store.register_at("ABC234", 1_000_000, 1_000_000);
    let accepted = pair_api_accept_plan(
        &mut store,
        &config,
        "ABC234",
        Some(PairAcceptInput {
            node: "remote".to_owned(),
            url: Some("http://remote".to_owned()),
        }),
        1_000_000,
    );
    assert_eq!(accepted.status, 200);
    assert!(accepted.ok);
    assert_eq!(accepted.node.as_deref(), Some("node-a"));
    assert_eq!(accepted.url.as_deref(), Some("http://localhost:4567"));
    assert_eq!(
        accepted.federation_token.as_deref(),
        Some("abababababababababababababababababababababababababababababababab")
    );

    let consumed = pair_api_status_plan(&store, "ABC-234", 1_000_000);
    assert_eq!(consumed.status, 200);
    assert!(consumed.ok);
    assert_eq!(consumed.consumed, Some(true));
    assert_eq!(consumed.remote_node.as_deref(), Some("remote"));
    assert_eq!(consumed.remote_url.as_deref(), Some("http://remote"));

    let not_found = pair_api_status_plan(&store, "ZZZ999", 1_000_000);
    assert_eq!(not_found.status, 404);
    assert_eq!(not_found.error.as_deref(), Some("not_found"));

    let _ = store.register_at("GHJ789", 1, 0);
    let expired_status = pair_api_status_plan(&store, "GHJ789", 1_000_000);
    assert_eq!(expired_status.status, 410);
    assert_eq!(expired_status.error.as_deref(), Some("expired"));

    let _ = store.register_at("JKL234", 1_000_000, 1_000_000);
    let pending = pair_api_status_plan(&store, "JKL234", 1_000_000);
    assert_eq!(pending.status, 200);
    assert_eq!(pending.consumed, Some(false));
}

// Ported from maw-js `test/pair-api-default.test.ts` auto-pair cases
// and `src/api/pair.ts` hello freshness / add-result behavior.
fn pair_api_test_config() -> maw_auth::PairApiConfig {
    maw_auth::PairApiConfig {
        node: "node-a".to_owned(),
        oracle: "oracle-a".to_owned(),
        port: 4567,
        base_url: "http://localhost:4567".to_owned(),
        federation_token: "token-a".to_owned(),
        pubkey: "p".repeat(64),
    }
}

fn auto_pair_input(zid: &str) -> maw_auth::AutoPairInput {
    maw_auth::AutoPairInput {
        node: "remote".to_owned(),
        oracle: None,
        url: "http://remote".to_owned(),
        zid: zid.to_owned(),
        pubkey: None,
    }
}

#[test]
fn pair_api_auto_plan_rejects_missing_stale_and_add_error_cases() {
    use maw_auth::{pair_api_auto_plan, AutoPairAddOutcome, RecentHelloStore};

    let config = pair_api_test_config();
    let mut hellos = RecentHelloStore::default();

    let missing_fields = pair_api_auto_plan(
        &config,
        &hellos,
        None,
        AutoPairAddOutcome::Ok { one_way: true },
        70_001,
    );
    assert_eq!(missing_fields.status, 400);
    assert_eq!(missing_fields.error.as_deref(), Some("missing_fields"));

    let missing = pair_api_auto_plan(
        &config,
        &hellos,
        Some(auto_pair_input("missing")),
        AutoPairAddOutcome::Ok { one_way: true },
        70_001,
    );
    assert_eq!(missing.status, 403);
    assert_eq!(missing.error.as_deref(), Some("no_recent_hello"));

    hellos.record("old", 0);
    hellos.record("fresh", 70_001);
    let stale = pair_api_auto_plan(
        &config,
        &hellos,
        Some(auto_pair_input("old")),
        AutoPairAddOutcome::Ok { one_way: true },
        70_001,
    );
    assert_eq!(stale.status, 403);
    assert_eq!(stale.error.as_deref(), Some("no_recent_hello"));

    hellos.record("mismatch", 70_001);
    let mismatch = pair_api_auto_plan(
        &config,
        &hellos,
        Some(auto_pair_input("mismatch")),
        AutoPairAddOutcome::PubkeyMismatch("key mismatch".to_owned()),
        70_001,
    );
    assert_eq!(mismatch.status, 409);
    assert_eq!(mismatch.error.as_deref(), Some("key mismatch"));

    hellos.record("throws", 70_001);
    let add_error = pair_api_auto_plan(
        &config,
        &hellos,
        Some(auto_pair_input("throws")),
        AutoPairAddOutcome::Error("bad peer".to_owned()),
        70_001,
    );
    assert_eq!(add_error.status, 400);
    assert_eq!(add_error.error.as_deref(), Some("bad peer"));
}

#[test]
fn pair_api_auto_plan_returns_signed_identity_and_peer_add_plan() {
    use maw_auth::{pair_api_auto_plan, AutoPairAddOutcome, AutoPairInput, RecentHelloStore};

    let config = pair_api_test_config();
    let mut hellos = RecentHelloStore::default();
    hellos.record("success", 70_001);

    let success = pair_api_auto_plan(
        &config,
        &hellos,
        Some(AutoPairInput {
            node: "remote".to_owned(),
            oracle: Some("remote-oracle".to_owned()),
            url: "http://remote".to_owned(),
            zid: "success".to_owned(),
            pubkey: Some("r".repeat(64)),
        }),
        AutoPairAddOutcome::Ok { one_way: false },
        70_001,
    );
    assert_eq!(success.status, 200);
    assert!(success.ok);
    assert_eq!(success.node.as_deref(), Some("node-a"));
    assert_eq!(success.oracle.as_deref(), Some("oracle-a"));
    assert_eq!(success.url.as_deref(), Some("http://localhost:4567"));
    assert_eq!(success.pubkey.as_deref(), Some(&"p".repeat(64)[..]));
    assert_eq!(success.one_way, Some(false));
    assert_eq!(success.add_alias.as_deref(), Some("remote"));
    assert_eq!(success.add_url.as_deref(), Some("http://remote"));
    assert_eq!(success.add_node.as_deref(), Some("remote"));
    assert_eq!(success.add_pubkey.as_deref(), Some(&"r".repeat(64)[..]));
    assert_eq!(
        success.add_identity_oracle.as_deref(),
        Some("remote-oracle")
    );
    assert_eq!(success.add_identity_node.as_deref(), Some("remote"));
    assert!(success.mark_symmetric_check);
    assert_eq!(
        success.proof.as_deref(),
        Some("95e63fc871ab14ce17c14e0046cd41b9dd305c086f1ed325fd2c5e62e6ee849f")
    );
}

#[test]
fn additional_auth_edge_cases_cover_refuse_consumed_and_parse_errors() {
    use maw_auth::{
        pair_api_accept_plan, pair_api_probe_plan, pair_api_status_plan, verify_hmac_sig,
        PairAcceptInput, PairApiConfig, PairCodeStore,
    };

    let unsigned = verify_req(Headers::new([("x-maw-from", FROM)]), b"", Some(PEER_KEY));
    assert_eq!(unsigned.kind(), "refuse-unsigned");
    assert!(is_refuse_decision(&unsigned));

    let invalid_legacy = verify_req(
        Headers::new([
            ("x-maw-from", FROM),
            ("x-maw-signature", &"0".repeat(64)),
            ("x-maw-signed-at", "not-an-iso-date"),
        ]),
        b"",
        Some(PEER_KEY),
    );
    assert_eq!(
        invalid_legacy,
        FromVerifyDecision::RefuseMalformed {
            reason: "invalid-signed-at".to_owned()
        }
    );

    assert!(!verify_hmac_sig(TOKEN, "payload", "00"));

    let config = PairApiConfig {
        node: "node-a".to_owned(),
        oracle: "oracle-a".to_owned(),
        port: 4567,
        base_url: "http://localhost:4567".to_owned(),
        federation_token: "token-a".to_owned(),
        pubkey: "p".repeat(64),
    };
    let mut store = PairCodeStore::default();
    let _ = store.register_at("ABC234", 60_000, 1_000);
    let accepted = pair_api_accept_plan(
        &mut store,
        &config,
        "ABC234",
        Some(PairAcceptInput {
            node: "remote".to_owned(),
            url: Some("http://remote".to_owned()),
        }),
        1_000,
    );
    assert!(accepted.ok);

    let consumed_probe = pair_api_probe_plan(&store, &config, "ABC234", 1_001);
    assert_eq!(consumed_probe.status, 410);
    assert_eq!(consumed_probe.error.as_deref(), Some("consumed"));

    let consumed_accept = pair_api_accept_plan(
        &mut store,
        &config,
        "ABC234",
        Some(PairAcceptInput {
            node: "remote".to_owned(),
            url: Some("http://remote".to_owned()),
        }),
        1_001,
    );
    assert_eq!(consumed_accept.status, 410);
    assert_eq!(consumed_accept.error.as_deref(), Some("consumed"));

    let invalid_status = pair_api_status_plan(&store, "bad", 1_001);
    assert_eq!(invalid_status.status, 400);
    assert_eq!(invalid_status.error.as_deref(), Some("invalid_shape"));
}

fn pending_with_expires_at(expires_at: &str) -> maw_auth::PendingRequest {
    maw_auth::PendingRequest {
        id: "edge".to_owned(),
        from: "neo".to_owned(),
        to: "mawjs".to_owned(),
        action: maw_auth::ConsentAction::Hey,
        summary: "edge".to_owned(),
        pin_hash: "hash".to_owned(),
        created_at: "2026-01-02T00:00:00.000Z".to_owned(),
        expires_at: expires_at.to_owned(),
        status: maw_auth::ConsentStatus::Pending,
    }
}

#[test]
fn consent_reject_errors_keep_statuses_precise() {
    use maw_auth::{
        approve_consent_plan, reject_consent_plan, request_consent_plan, ConsentAction,
        ConsentRequestArgs, ConsentStore, PeerPostResult,
    };

    let mut store = ConsentStore::default();
    assert_eq!(
        reject_consent_plan(&mut store, "missing").error.as_deref(),
        Some("request not found: missing")
    );

    request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            from: "neo".to_owned(),
            to: "mawjs".to_owned(),
            action: ConsentAction::Hey,
            summary: "reject me".to_owned(),
            peer_url: None,
            request_id: "req-reject".to_owned(),
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::Skipped,
        },
    );
    assert!(reject_consent_plan(&mut store, "req-reject").ok);
    assert_eq!(
        reject_consent_plan(&mut store, "req-reject")
            .error
            .as_deref(),
        Some("request is rejected, cannot reject")
    );

    let mut expired_store = ConsentStore::default();
    request_consent_plan(
        &mut expired_store,
        ConsentRequestArgs {
            from: "neo".to_owned(),
            to: "mawjs".to_owned(),
            action: ConsentAction::Hey,
            summary: "expire me".to_owned(),
            peer_url: None,
            request_id: "req-expired".to_owned(),
            pin: "ABCDEF".to_owned(),
            now_ms: 1_767_312_000_000,
            peer_post: PeerPostResult::Skipped,
        },
    );
    assert_eq!(
        approve_consent_plan(
            &mut expired_store,
            "req-expired",
            "ABCDEF",
            1_767_312_601_000
        )
        .error
        .as_deref(),
        Some("request is expired, cannot approve")
    );
}

