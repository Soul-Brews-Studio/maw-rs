
#[test]
fn state_path_is_primary_while_legacy_home_peers_are_migrated_on_mutation() {
    let tmp = TestDir::new("maw-rs-peer-store-migrate");
    let home = tmp.path().join("home");
    let state = tmp.path().join("state");
    let env = PeerStoreEnv::with_vars(
        &home,
        [("MAW_STATE_DIR", state.to_string_lossy().into_owned())],
    );
    let legacy_file = home.join(".maw").join("peers.json");
    fs::create_dir_all(legacy_file.parent().unwrap()).unwrap();
    fs::write(
        &legacy_file,
        r#"{"version":1,"peers":{"legacy":{"url":"http://legacy.local:3456","node":"legacy-node","addedAt":"2026-05-20T00:00:00.000Z","lastSeen":null}}}"#,
    )
    .unwrap();

    assert_eq!(peer_store_path(&env), state.join("peers.json"));
    assert_eq!(
        load_peer_store(&env).peers["legacy"].node.as_deref(),
        Some("legacy-node")
    );

    let migrated = mutate_peer_store(&env, |data| {
        data.peers.insert(
            "state".to_owned(),
            PeerRecord {
                url: "http://state.local:3456".to_owned(),
                node: Some("state-node".to_owned()),
                added_at: "2026-05-20T01:00:00.000Z".to_owned(),
                last_seen: None,
                last_error: None,
                nickname: None,
                pubkey: None,
                pubkey_first_seen: None,
                identity: None,
                one_way: None,
                last_symmetric_check: None,
            },
        );
    })
    .unwrap();

    assert_eq!(
        migrated.peers.keys().cloned().collect::<Vec<_>>(),
        vec!["legacy", "state"]
    );
    assert_eq!(
        load_peer_store(&env).peers["legacy"].node.as_deref(),
        Some("legacy-node")
    );
    let legacy_after: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(legacy_file).unwrap()).unwrap();
    assert!(legacy_after["peers"]["state"].is_null());
}

#[test]
fn invalid_json_and_invalid_shapes_are_moved_aside_while_callers_get_empty_store() {
    let tmp = TestDir::new("maw-rs-peer-store-corrupt");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );

    save_peer_store(&env, &PeerStoreFile::default()).unwrap();
    fs::write(&file, "{not-json").unwrap();
    assert_eq!(load_peer_store(&env), PeerStoreFile::default());
    assert!(!file.exists());
    assert_eq!(load_peer_store(&env), PeerStoreFile::default());

    save_peer_store(&env, &PeerStoreFile::default()).unwrap();
    fs::write(&file, r#"{"version":1,"peers":[]}"#).unwrap();
    assert_eq!(load_peer_store(&env), PeerStoreFile::default());
    assert!(!file.exists());
}

#[test]
fn mutate_peer_store_reads_inside_lock_and_tolerates_malformed_existing_contents() {
    let tmp = TestDir::new("maw-rs-peer-store-mutates");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );
    let mut peers = BTreeMap::new();
    peers.insert("before".to_owned(), peer("bad", None));
    save_peer_store(&env, &PeerStoreFile { version: 1, peers }).unwrap();

    let first = mutate_peer_store(&env, |data| {
        data.peers.insert(
            "after".to_owned(),
            PeerRecord {
                url: "http://after".to_owned(),
                node: Some("after-node".to_owned()),
                added_at: "2026-05-18T00:00:00.000Z".to_owned(),
                last_seen: Some("2026-05-18T01:00:00.000Z".to_owned()),
                last_error: None,
                nickname: None,
                pubkey: None,
                pubkey_first_seen: None,
                identity: None,
                one_way: None,
                last_symmetric_check: None,
            },
        );
    })
    .unwrap();
    assert_eq!(
        first.peers.keys().cloned().collect::<Vec<_>>(),
        vec!["after", "before"]
    );
    assert_eq!(
        load_peer_store(&env).peers["after"].node.as_deref(),
        Some("after-node")
    );

    fs::write(&file, r#"{"peers":[]}"#).unwrap();
    let recovered = mutate_peer_store(&env, |data| {
        data.peers.insert(
            "recovered".to_owned(),
            PeerRecord {
                url: "http://recovered".to_owned(),
                node: None,
                added_at: "x".to_owned(),
                last_seen: None,
                last_error: None,
                nickname: None,
                pubkey: None,
                pubkey_first_seen: None,
                identity: None,
                one_way: None,
                last_symmetric_check: None,
            },
        );
    })
    .unwrap();
    assert_eq!(
        recovered.peers.keys().cloned().collect::<Vec<_>>(),
        vec!["recovered"]
    );
    assert_eq!(
        load_peer_store(&env).peers["recovered"].url,
        "http://recovered"
    );
}

#[test]
fn read_errors_and_unlocked_parse_errors_recover_as_empty_stores() {
    let tmp = TestDir::new("maw-rs-peer-store-read-errors");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );

    fs::create_dir_all(&file).unwrap();
    assert_eq!(load_peer_store(&env), PeerStoreFile::default());

    fs::remove_dir_all(&file).unwrap();
    fs::write(&file, "{not-json").unwrap();
    let recovered = mutate_peer_store(&env, |data| {
        data.peers.insert(
            "recovered".to_owned(),
            PeerRecord {
                url: "http://recovered".to_owned(),
                node: None,
                added_at: "bad".to_owned(),
                last_seen: None,
                last_error: None,
                nickname: None,
                pubkey: None,
                pubkey_first_seen: None,
                identity: None,
                one_way: None,
                last_symmetric_check: None,
            },
        );
    })
    .unwrap();
    assert_eq!(
        recovered.peers.keys().cloned().collect::<Vec<_>>(),
        vec!["recovered"]
    );
    assert_eq!(
        load_peer_store(&env).peers["recovered"].url,
        "http://recovered"
    );
}

#[test]
fn explicit_stale_cleanup_ignores_missing_and_removes_leftover_tmp_files() {
    let tmp = TestDir::new("maw-rs-peer-store-clear-tmp");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );

    maw_peer::clear_stale_peer_store_tmp(&env);
    save_peer_store(&env, &PeerStoreFile::default()).unwrap();
    fs::write(format!("{}.tmp", file.display()), "leftover").unwrap();
    maw_peer::clear_stale_peer_store_tmp(&env);
    assert!(!PathBuf::from(format!("{}.tmp", file.display())).exists());
}
