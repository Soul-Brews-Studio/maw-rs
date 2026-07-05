
#[test]
fn peer_add_covers_legacy_no_pin_paths_without_symmetric_metadata() {
    let legacy_new = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "legacynew".to_owned(),
        url: "https://legacynew.example".to_owned(),
        node: None,
        authenticated_pubkey: None,
        authenticated_identity: None,
        mark_symmetric_check: false,
        one_way: None,
        now: now(),
        peers: BTreeMap::new(),
        probe: ok_probe(Some("legacy-node"), None),
    })
    .expect("legacy first contact without pubkey is accepted");
    assert!(!legacy_new.overwrote);
    assert_eq!(legacy_new.peer.pubkey, None);
    assert_eq!(legacy_new.peer.pubkey_first_seen, None);
    assert_eq!(legacy_new.peer.last_symmetric_check, None);

    let existing = peer("https://legacy-existing.example");
    let legacy_existing = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "legacyexisting".to_owned(),
        url: "https://legacy-existing-new.example".to_owned(),
        node: None,
        authenticated_pubkey: None,
        authenticated_identity: None,
        mark_symmetric_check: false,
        one_way: None,
        now: now(),
        peers: BTreeMap::from([("legacyexisting".to_owned(), existing)]),
        probe: ok_probe(Some("legacy-existing-node"), None),
    })
    .expect("legacy re-add without pubkey is accepted");
    assert!(legacy_existing.overwrote);
    assert_eq!(legacy_existing.peer.pubkey, None);
    assert_eq!(legacy_existing.peer.pubkey_first_seen, None);
    assert_eq!(legacy_existing.peer.last_symmetric_check, None);
}

#[test]
fn probe_all_failure_persists_last_error_like_maw_js() {
    let result = probe_all_from_plan(&ProbeAllPlan {
        timeout_ms: 10,
        now: now(),
        peers: vec![("down".to_owned(), peer("https://down.example"))],
        probe_results: vec![(
            "https://down.example".to_owned(),
            err_probe(ProbeErrorCode::Refused, "connection refused"),
            9,
        )],
        removed_before_mutate: vec![],
    });

    assert_eq!(result.ok_count, 0);
    assert_eq!(result.fail_count, 1);
    let stored_error = result.peers_after["down"]
        .last_error
        .as_ref()
        .expect("failed probe writes lastError");
    assert_eq!(stored_error.code, ProbeErrorCode::Refused);
    assert_eq!(stored_error.message, "connection refused");
    assert_eq!(result.rows[0].error.as_ref(), Some(stored_error));
}

#[test]
fn peer_add_probe_error_defaults_one_way_and_preserves_last_error() {
    let result = cmd_peer_add_from_plan(&PeerAddPlan {
        alias: "flaky".to_owned(),
        url: "https://flaky.example".to_owned(),
        node: None,
        authenticated_pubkey: None,
        authenticated_identity: None,
        mark_symmetric_check: true,
        one_way: None,
        now: now(),
        peers: BTreeMap::new(),
        probe: err_probe(ProbeErrorCode::Timeout, "timed out"),
    })
    .expect("probe errors still cache one-way peer records");

    assert_eq!(result.peer.one_way, Some(true));
    assert_eq!(result.peer.last_seen, None);
    assert_eq!(
        result.peer.last_error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::Timeout)
    );
    assert_eq!(
        result.probe_error.as_ref().map(|err| err.message.as_str()),
        Some("timed out")
    );
}

#[test]
fn peer_store_mutations_surface_atomic_write_parent_errors() {
    let root = std::env::temp_dir().join(format!(
        "maw-rs-peer-store-parent-file-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&root);
    std::fs::write(&root, b"not a directory").expect("create parent blocker");
    let file = root.join("peers.json");
    let env = maw_peer::PeerStoreEnv::with_vars(
        std::env::temp_dir(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );

    let err = maw_peer::save_peer_store(&env, &maw_peer::empty_peer_store())
        .expect_err("file parent blocks atomic store write");

    assert!(
        matches!(
            err.kind(),
            std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotADirectory
        ),
        "unexpected error kind: {err:?}"
    );
    let _ = std::fs::remove_file(root);
}
