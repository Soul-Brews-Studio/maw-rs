
#[test]
fn cmd_peer_add_bootstraps_existing_unpinned_peer_and_marks_symmetric_check() {
    let mut unpinned = peer("http://old-ivy");
    unpinned.last_symmetric_check = Some("previous-check".to_owned());
    unpinned.one_way = Some(true);
    let bootstrapped_existing = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "ivy".to_owned(),
        url: "http://new-ivy".to_owned(),
        node: None,
        authenticated_pubkey: None,
        authenticated_identity: None,
        mark_symmetric_check: true,
        one_way: None,
        now: "2026-05-18T12:02:00.000Z".to_owned(),
        peers: BTreeMap::from([("ivy".to_owned(), unpinned)]),
        probe: ok_probe("ivy-node", Some("new-pin")),
    })
    .unwrap();

    assert!(bootstrapped_existing.overwrote);
    assert_eq!(
        bootstrapped_existing.peer.pubkey.as_deref(),
        Some("new-pin")
    );
    assert_eq!(
        bootstrapped_existing.peer.pubkey_first_seen.as_deref(),
        Some("2026-05-18T12:02:00.000Z")
    );
    assert_eq!(bootstrapped_existing.peer.one_way, Some(false));
}

#[test]
fn cmd_peer_add_reports_validation_failures_before_cache_mutation() {
    let bad_alias = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "Bad".to_owned(),
        url: "http://bad".to_owned(),
        node: None,
        authenticated_pubkey: None,
        authenticated_identity: None,
        mark_symmetric_check: false,
        one_way: None,
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: BTreeMap::new(),
        probe: ok_probe("bad", None),
    })
    .unwrap_err();
    assert!(bad_alias.contains("invalid alias"));

    let bad_url = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "bad".to_owned(),
        url: "ftp://bad".to_owned(),
        node: None,
        authenticated_pubkey: None,
        authenticated_identity: None,
        mark_symmetric_check: false,
        one_way: None,
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: BTreeMap::new(),
        probe: ok_probe("bad", None),
    })
    .unwrap_err();
    assert!(bad_url.contains("must be http:// or https://"));
}

#[test]
fn cmd_peer_probe_mismatch_skips_mutation_and_success_refreshes_identity() {
    let mut erin = peer("http://erin");
    erin.node = Some("old-node".to_owned());
    erin.pubkey = Some("cached-key".to_owned());
    erin.last_seen = Some("old-seen".to_owned());
    let mismatch = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "erin".to_owned(),
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: BTreeMap::from([("erin".to_owned(), erin.clone())]),
        probe: ok_probe("rotated-node", Some("new-key")),
        remove_before_mutate: false,
    })
    .unwrap();

    assert_eq!(mismatch.alias, "erin");
    assert!(!mismatch.ok);
    assert_eq!(mismatch.node.as_deref(), Some("rotated-node"));
    assert!(mismatch.pubkey_mismatch.is_some());
    assert_eq!(mismatch.peers_after["erin"], erin);

    let mut dave = peer("http://dave");
    dave.node = Some("seed-node".to_owned());
    dave.nickname = Some("seed-nick".to_owned());
    dave.identity = Some(PeerIdentity {
        oracle: "seed".to_owned(),
        node: "seed-node".to_owned(),
    });
    dave.pubkey = Some("stable-key".to_owned());
    let refreshed = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "dave".to_owned(),
        now: "2026-05-18T12:03:00.000Z".to_owned(),
        peers: BTreeMap::from([("dave".to_owned(), dave)]),
        probe: ProbePeerResult {
            node: Some("fresh-node".to_owned()),
            nickname: Some("fresh-nick".to_owned()),
            pubkey: Some("stable-key".to_owned()),
            identity: Some(PeerIdentity {
                oracle: "fresh".to_owned(),
                node: "fresh-node".to_owned(),
            }),
            error: None,
        },
        remove_before_mutate: false,
    })
    .unwrap();

    assert!(refreshed.ok);
    let dave_after = &refreshed.peers_after["dave"];
    assert_eq!(
        dave_after.last_seen.as_deref(),
        Some("2026-05-18T12:03:00.000Z")
    );
    assert_eq!(dave_after.last_error, None);
    assert_eq!(dave_after.node.as_deref(), Some("fresh-node"));
    assert_eq!(dave_after.nickname.as_deref(), Some("fresh-nick"));
    assert_eq!(
        dave_after
            .identity
            .as_ref()
            .map(|identity| identity.oracle.as_str()),
        Some("fresh")
    );
}

#[test]
fn cmd_peer_probe_reports_missing_alias_and_records_probe_errors() {
    let missing = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "missing".to_owned(),
        now: "2026-05-18T12:04:00.000Z".to_owned(),
        peers: BTreeMap::new(),
        probe: ok_probe("missing", None),
        remove_before_mutate: false,
    })
    .unwrap_err();
    assert_eq!(missing, "peer \"missing\" not found");

    let probe_error = ProbeLastError {
        code: ProbeErrorCode::Refused,
        message: "closed".to_owned(),
        at: "2026-05-18T12:05:00.000Z".to_owned(),
    };
    let failed = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "dave".to_owned(),
        now: "2026-05-18T12:05:00.000Z".to_owned(),
        peers: BTreeMap::from([("dave".to_owned(), peer("http://dave"))]),
        probe: ProbePeerResult {
            node: None,
            nickname: None,
            pubkey: None,
            identity: None,
            error: Some(probe_error.clone()),
        },
        remove_before_mutate: false,
    })
    .unwrap();

    assert!(!failed.ok);
    assert_eq!(failed.error, Some(probe_error.clone()));
    assert_eq!(failed.peers_after["dave"].last_error, Some(probe_error));
}

#[test]
fn cmd_peer_probe_bootstraps_empty_pin_and_tolerates_removed_peer_before_mutation() {
    let mut unpinned = peer("http://grace");
    unpinned.pubkey = Some(String::new());
    let bootstrapped = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "grace".to_owned(),
        now: "2026-05-18T12:06:00.000Z".to_owned(),
        peers: BTreeMap::from([("grace".to_owned(), unpinned)]),
        probe: ok_probe("grace-node", Some("grace-pin")),
        remove_before_mutate: false,
    })
    .unwrap();
    assert_eq!(
        bootstrapped.peers_after["grace"].pubkey.as_deref(),
        Some("grace-pin")
    );
    assert_eq!(
        bootstrapped.peers_after["grace"]
            .pubkey_first_seen
            .as_deref(),
        Some("2026-05-18T12:06:00.000Z")
    );

    let removed_before_mutate = cmd_peer_probe_from_plan(&PeerProbePlan {
        alias: "grace".to_owned(),
        now: "2026-05-18T12:07:00.000Z".to_owned(),
        peers: bootstrapped.peers_after,
        probe: ok_probe("grace-new", Some("grace-pin")),
        remove_before_mutate: true,
    })
    .unwrap();
    assert!(removed_before_mutate.ok);
    assert!(!removed_before_mutate.peers_after.contains_key("grace"));
    assert_eq!(removed_before_mutate.node.as_deref(), Some("grace-new"));
}
