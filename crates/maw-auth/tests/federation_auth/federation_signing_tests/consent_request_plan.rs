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
