use maw_peer::{
    cmd_peer_add_from_plan, cmd_peer_probe_from_plan, probe_all_from_plan, PeerAddPlan,
    PeerIdentity, PeerProbePlan, PeerRecord, ProbeAllPlan, ProbeErrorCode, ProbeLastError,
    ProbePeerResult,
};
use std::collections::BTreeMap;

fn peer(url: &str) -> PeerRecord {
    PeerRecord {
        url: url.to_owned(),
        node: None,
        added_at: now(),
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

fn now() -> String {
    "2026-05-21T00:00:00.000Z".to_owned()
}

fn ok_probe(node: Option<&str>, pubkey: Option<&str>) -> ProbePeerResult {
    ProbePeerResult {
        node: node.map(str::to_owned),
        nickname: None,
        pubkey: pubkey.map(str::to_owned),
        identity: None,
        error: None,
    }
}

fn err_probe(code: ProbeErrorCode, message: &str) -> ProbePeerResult {
    ProbePeerResult {
        node: None,
        nickname: None,
        pubkey: None,
        identity: None,
        error: Some(ProbeLastError {
            code,
            message: message.to_owned(),
            at: now(),
        }),
    }
}

#[test]
fn peer_add_covers_probe_node_fallback_authenticated_identity_and_new_mismatch() {
    let added = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "newpeer".to_owned(),
        url: "https://newpeer.example".to_owned(),
        node: None,
        authenticated_pubkey: Some("auth-pin".to_owned()),
        authenticated_identity: Some(PeerIdentity {
            oracle: "auth-oracle".to_owned(),
            node: "auth-node".to_owned(),
        }),
        mark_symmetric_check: true,
        one_way: None,
        now: now(),
        peers: BTreeMap::new(),
        probe: ok_probe(Some("probe-node"), None),
    })
    .expect("new peer add succeeds");

    assert!(!added.overwrote);
    assert_eq!(added.peer.node.as_deref(), Some("probe-node"));
    assert_eq!(added.peer.last_seen.as_deref(), Some(now().as_str()));
    assert_eq!(added.peer.pubkey.as_deref(), Some("auth-pin"));
    assert_eq!(added.peer.one_way, Some(false));
    assert_eq!(
        added
            .peer
            .identity
            .as_ref()
            .map(|identity| (identity.oracle.as_str(), identity.node.as_str(),)),
        Some(("auth-oracle", "auth-node"))
    );

    let mismatch = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "surprise".to_owned(),
        url: "https://surprise.example".to_owned(),
        node: None,
        authenticated_pubkey: Some("authenticated-pin".to_owned()),
        authenticated_identity: None,
        mark_symmetric_check: false,
        one_way: None,
        now: now(),
        peers: BTreeMap::new(),
        probe: ok_probe(Some("surprise-node"), Some("different-probed-pin")),
    })
    .expect("mismatch returns a structured result");

    assert!(!mismatch.overwrote);
    assert_eq!(mismatch.peer.node.as_deref(), Some("surprise-node"));
    let pubkey_mismatch = mismatch.pubkey_mismatch.expect("pubkey mismatch");
    assert_eq!(pubkey_mismatch.alias, "surprise");
    assert_eq!(pubkey_mismatch.cached, "authenticated-pin");
    assert_eq!(pubkey_mismatch.observed, "different-probed-pin");
    assert!(mismatch.peers_after.is_empty());
}

#[test]
fn peer_probe_covers_cached_node_fallback_and_absent_optional_updates() {
    let mut cached = peer("https://cached.example");
    cached.node = Some("cached-node".to_owned());
    cached.nickname = Some("cached-nick".to_owned());
    cached.identity = Some(PeerIdentity {
        oracle: "cached-oracle".to_owned(),
        node: "cached-node".to_owned(),
    });
    cached.pubkey = Some("cached-pin".to_owned());
    let peers = BTreeMap::from([("cached".to_owned(), cached.clone())]);

    let mismatch = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "cached".to_owned(),
        now: now(),
        peers: peers.clone(),
        probe: ok_probe(None, Some("rotated-pin")),
        remove_before_mutate: false,
    })
    .expect("mismatch returns structured probe result");
    assert!(!mismatch.ok);
    assert_eq!(mismatch.node.as_deref(), Some("cached-node"));
    assert_eq!(mismatch.peers_after["cached"], cached);

    let stable = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "cached".to_owned(),
        now: now(),
        peers,
        probe: ok_probe(None, Some("cached-pin")),
        remove_before_mutate: false,
    })
    .expect("stable probe succeeds");
    let cached_after = &stable.peers_after["cached"];
    assert!(stable.ok);
    assert_eq!(stable.node.as_deref(), Some("cached-node"));
    assert_eq!(cached_after.node.as_deref(), Some("cached-node"));
    assert_eq!(cached_after.nickname.as_deref(), Some("cached-nick"));
    assert_eq!(
        cached_after
            .identity
            .as_ref()
            .map(|identity| identity.oracle.as_str()),
        Some("cached-oracle")
    );
    assert_eq!(cached_after.last_seen.as_deref(), Some(now().as_str()));
}

#[test]
fn probe_all_covers_success_without_any_node_and_non_dns_exit_code_mapping() {
    let result = probe_all_from_plan(&ProbeAllPlan {
        timeout_ms: 42,
        now: now(),
        peers: vec![
            ("nameless".to_owned(), peer("https://nameless.example")),
            ("badbody".to_owned(), peer("https://badbody.example")),
        ],
        probe_results: vec![
            (
                "https://nameless.example".to_owned(),
                ok_probe(None, None),
                3,
            ),
            (
                "https://badbody.example".to_owned(),
                err_probe(ProbeErrorCode::BadBody, "invalid maw handshake"),
                5,
            ),
        ],
        removed_before_mutate: vec![],
    });

    assert_eq!(result.ok_count, 1);
    assert_eq!(result.fail_count, 1);
    assert_eq!(result.worst_exit_code, 2);
    assert_eq!(result.rows[1].alias, "nameless");
    assert_eq!(result.rows[1].node, None);
    assert_eq!(
        result
            .peers_after
            .get("nameless")
            .and_then(|peer| peer.node.as_deref()),
        None
    );
    assert_eq!(
        result
            .peers_after
            .get("badbody")
            .and_then(|peer| peer.last_error.as_ref())
            .map(|error| error.code),
        Some(ProbeErrorCode::BadBody)
    );
}
