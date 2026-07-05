#[test]
fn tofu_record_and_forget_peer_pubkey_match_maw_js_outcomes_and_preserve_fields() {
    let tmp = TestDir::new("maw-rs-tofu-record-forget");
    let env = env_for(&tmp);
    let mut peers = BTreeMap::new();
    peers.insert("carol".to_owned(), peer("http://carol"));
    save_peer_store(&env, &PeerStoreFile { version: 1, peers }).unwrap();

    let carol = load_peer_store(&env).peers.get("carol").cloned();
    let bootstrapped = tofu_record_peer_identity(
        &env,
        "carol",
        carol.as_ref(),
        Some("carol-pubkey"),
        "2026-05-18T12:00:00.000Z",
    )
    .unwrap();
    assert_eq!(bootstrapped.kind, TofuDecisionKind::TofuBootstrap);
    assert_eq!(
        load_peer_store(&env).peers["carol"].pubkey.as_deref(),
        Some("carol-pubkey")
    );

    let carol = load_peer_store(&env).peers.get("carol").cloned();
    let matched = tofu_record_peer_identity(
        &env,
        "carol",
        carol.as_ref(),
        Some("carol-pubkey"),
        "2026-05-18T12:01:00.000Z",
    )
    .unwrap();
    assert_eq!(matched.kind, TofuDecisionKind::Match);

    let carol = load_peer_store(&env).peers.get("carol").cloned();
    assert!(tofu_record_peer_identity(
        &env,
        "carol",
        carol.as_ref(),
        Some("rotated-carol-pubkey"),
        "2026-05-18T12:02:00.000Z"
    )
    .is_err());

    assert_eq!(forget_peer_pubkey(&env, "missing").unwrap(), "not-found");

    let mut store = load_peer_store(&env);
    let mut legacy = peer("http://legacy");
    legacy.nickname = Some("old-node".to_owned());
    store.peers.insert("legacy".to_owned(), legacy);
    let mut pinned = peer("http://pinned");
    pinned.node = Some("node".to_owned());
    pinned.nickname = Some("keep-me".to_owned());
    pinned.pubkey = Some("pinned-pubkey".to_owned());
    pinned.pubkey_first_seen = Some("2026-05-18T00:00:00.000Z".to_owned());
    store.peers.insert("pinned".to_owned(), pinned);
    save_peer_store(&env, &store).unwrap();

    assert_eq!(forget_peer_pubkey(&env, "legacy").unwrap(), "no-pubkey");
    let legacy = load_peer_store(&env).peers.remove("legacy").unwrap();
    assert_eq!(legacy.url, "http://legacy");
    assert_eq!(legacy.nickname.as_deref(), Some("old-node"));

    assert_eq!(forget_peer_pubkey(&env, "pinned").unwrap(), "cleared");
    let pinned = load_peer_store(&env).peers.remove("pinned").unwrap();
    assert_eq!(pinned.url, "http://pinned");
    assert_eq!(pinned.node.as_deref(), Some("node"));
    assert_eq!(pinned.nickname.as_deref(), Some("keep-me"));
    assert_eq!(pinned.pubkey, None);
    assert_eq!(pinned.pubkey_first_seen, None);
}

fn env_for(tmp: &TestDir) -> PeerStoreEnv {
    let file = tmp.path.join("peers.json");
    PeerStoreEnv::with_vars(
        tmp.path.clone(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    )
}

struct TestDir {
    path: PathBuf,
}

impl TestDir {
    fn new(prefix: &str) -> Self {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("{prefix}-{}-{unique}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
