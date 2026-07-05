use maw_peer::{
    apply_tofu_decision, evaluate_peer_identity, forget_peer_pubkey, load_peer_store,
    save_peer_store, tofu_record_peer_identity, PeerRecord, PeerStoreEnv, PeerStoreFile,
    TofuApplyError, TofuDecision, TofuDecisionKind,
};
use std::{
    collections::BTreeMap,
    fs, io,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

fn peer(url: &str) -> PeerRecord {
    PeerRecord {
        url: url.to_owned(),
        node: None,
        added_at: "2026-05-18T00:00:00.000Z".to_owned(),
        last_seen: None,
        last_error: None,
        nickname: None,
        pubkey: None,
        pubkey_first_seen: None,
        identity: None,
        one_way: None,
        last_symmetric_check: None,
    }
}

#[test]
fn evaluate_peer_identity_covers_every_maw_js_tofu_decision_kind() {
    let fresh = evaluate_peer_identity("fresh", None, Some("observed-pubkey-0123456789"));
    assert_eq!(fresh.kind, TofuDecisionKind::TofuBootstrap);
    assert_eq!(fresh.alias, "fresh");
    assert_eq!(fresh.cached, None);
    assert_eq!(
        fresh.observed.as_deref(),
        Some("observed-pubkey-0123456789")
    );
    assert!(fresh.message.contains("first sight"));

    let unpinned = evaluate_peer_identity(
        "unpinned",
        Some(&peer("http://unpinned")),
        Some("observed-after-legacy"),
    );
    assert_eq!(unpinned.kind, TofuDecisionKind::TofuBootstrap);
    assert_eq!(unpinned.observed.as_deref(), Some("observed-after-legacy"));

    let mut empty_cached = peer("http://empty");
    empty_cached.pubkey = Some(String::new());
    let empty_cached_decision = evaluate_peer_identity(
        "empty-cache",
        Some(&empty_cached),
        Some("observed-after-empty-cache"),
    );
    assert_eq!(empty_cached_decision.kind, TofuDecisionKind::TofuBootstrap);

    let legacy = evaluate_peer_identity("legacy", None, None);
    assert_eq!(legacy.kind, TofuDecisionKind::LegacyFirstContact);
    assert_eq!(legacy.cached, None);
    assert_eq!(legacy.observed, None);
    assert!(legacy.message.contains("legacy peer"));

    let mut rollback_peer = peer("http://rollback");
    rollback_peer.pubkey = Some("cached-pubkey-abcdefghijklmnop".to_owned());
    let rollback = evaluate_peer_identity("rollback", Some(&rollback_peer), None);
    assert_eq!(rollback.kind, TofuDecisionKind::LegacyAfterPinned);
    assert_eq!(
        rollback.cached.as_deref(),
        Some("cached-pubkey-abcdefghijklmnop")
    );
    assert_eq!(rollback.observed, None);
    assert!(rollback.message.contains("will hard-fail at v27"));

    let mut stable_peer = peer("http://stable");
    stable_peer.pubkey = Some("same-pubkey".to_owned());
    let stable = evaluate_peer_identity("stable", Some(&stable_peer), Some("same-pubkey"));
    assert_eq!(stable.kind, TofuDecisionKind::Match);
    assert_eq!(stable.cached.as_deref(), Some("same-pubkey"));
    assert_eq!(stable.observed.as_deref(), Some("same-pubkey"));
    assert!(stable.message.contains("pubkey verified"));

    let mut rotated_peer = peer("http://rotated");
    rotated_peer.pubkey = Some("cached-pubkey-abcdefghijklmnop".to_owned());
    let mismatch = evaluate_peer_identity(
        "rotated",
        Some(&rotated_peer),
        Some("observed-pubkey-qrstuvwxyz"),
    );
    assert_eq!(mismatch.kind, TofuDecisionKind::Mismatch);
    assert_eq!(
        mismatch.cached.as_deref(),
        Some("cached-pubkey-abcdefghijklmnop")
    );
    assert_eq!(
        mismatch.observed.as_deref(),
        Some("observed-pubkey-qrstuvwxyz")
    );
    assert!(mismatch.message.contains("maw peers forget rotated"));
}

#[test]
fn apply_tofu_decision_bootstraps_once_preserves_race_safe_pins_and_throws_mismatch() {
    let tmp = TestDir::new("maw-rs-tofu-apply");
    let env = env_for(&tmp);
    let mut peers = BTreeMap::new();
    peers.insert("alice".to_owned(), peer("http://alice"));
    save_peer_store(&env, &PeerStoreFile { version: 1, peers }).unwrap();

    apply_tofu_decision(
        &env,
        &TofuDecision {
            kind: TofuDecisionKind::TofuBootstrap,
            alias: "alice".to_owned(),
            cached: None,
            observed: Some("alice-pubkey".to_owned()),
            message: "cache alice".to_owned(),
        },
        "2026-05-18T12:00:00.000Z",
    )
    .unwrap();
    let alice = load_peer_store(&env).peers.remove("alice").unwrap();
    assert_eq!(alice.pubkey.as_deref(), Some("alice-pubkey"));
    assert_eq!(
        alice.pubkey_first_seen.as_deref(),
        Some("2026-05-18T12:00:00.000Z")
    );

    let mut store = load_peer_store(&env);
    store.peers.get_mut("alice").unwrap().pubkey_first_seen = Some("first-write-wins".to_owned());
    save_peer_store(&env, &store).unwrap();
    apply_tofu_decision(
        &env,
        &TofuDecision {
            kind: TofuDecisionKind::TofuBootstrap,
            alias: "alice".to_owned(),
            cached: None,
            observed: Some("racing-pubkey".to_owned()),
            message: "stale bootstrap should not overwrite".to_owned(),
        },
        "2026-05-18T13:00:00.000Z",
    )
    .unwrap();
    let alice = load_peer_store(&env).peers.remove("alice").unwrap();
    assert_eq!(alice.pubkey.as_deref(), Some("alice-pubkey"));
    assert_eq!(alice.pubkey_first_seen.as_deref(), Some("first-write-wins"));

    apply_tofu_decision(
        &env,
        &TofuDecision {
            kind: TofuDecisionKind::TofuBootstrap,
            alias: "forgotten".to_owned(),
            cached: None,
            observed: Some("lost-race-pubkey".to_owned()),
            message: "peer was deleted between evaluate and apply".to_owned(),
        },
        "2026-05-18T14:00:00.000Z",
    )
    .unwrap();
    assert!(!load_peer_store(&env).peers.contains_key("forgotten"));

    for decision in [
        TofuDecision {
            kind: TofuDecisionKind::Match,
            alias: "stable".to_owned(),
            cached: Some("same".to_owned()),
            observed: Some("same".to_owned()),
            message: "verified".to_owned(),
        },
        TofuDecision {
            kind: TofuDecisionKind::LegacyFirstContact,
            alias: "legacy".to_owned(),
            cached: None,
            observed: None,
            message: "no pubkey yet".to_owned(),
        },
        TofuDecision {
            kind: TofuDecisionKind::LegacyAfterPinned,
            alias: "rollback".to_owned(),
            cached: Some("cached".to_owned()),
            observed: None,
            message: "rollback accepted for migration".to_owned(),
        },
    ] {
        apply_tofu_decision(&env, &decision, "2026-05-18T15:00:00.000Z").unwrap();
    }

    let err = apply_tofu_decision(
        &env,
        &TofuDecision {
            kind: TofuDecisionKind::Mismatch,
            alias: "mallory".to_owned(),
            cached: Some("cached-pubkey-abcdefghijklmnop".to_owned()),
            observed: Some("observed-pubkey-qrstuvwxyz".to_owned()),
            message: "rotation refused".to_owned(),
        },
        "2026-05-18T16:00:00.000Z",
    )
    .unwrap_err();
    let TofuApplyError::Mismatch(err) = err else {
        panic!("expected mismatch error");
    };
    assert_eq!(err.alias, "mallory");
    assert_eq!(err.cached, "cached-pubkey-abcdefghijklmnop");
    assert_eq!(err.observed, "observed-pubkey-qrstuvwxyz");
    assert!(err.to_string().contains("maw peers forget mallory"));
}

#[test]
fn tofu_apply_error_display_preserves_mismatch_and_io_messages() {
    let mismatch = TofuApplyError::from(maw_peer::PeerPubkeyMismatchError::new(
        "mallory",
        "cached-pubkey-abcdefghijklmnop",
        "observed-pubkey-qrstuvwxyz",
    ));
    assert!(mismatch.to_string().contains("pubkey changed"));
    let io_error = TofuApplyError::from(io::Error::other("disk full"));
    assert_eq!(io_error.to_string(), "disk full");
}

