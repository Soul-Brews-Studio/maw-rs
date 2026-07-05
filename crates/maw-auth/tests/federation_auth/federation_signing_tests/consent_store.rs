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

