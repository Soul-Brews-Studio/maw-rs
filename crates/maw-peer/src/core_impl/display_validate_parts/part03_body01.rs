    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("maw-peer-{name}-{nonce}"))
    }

    fn peer_record(url: &str) -> PeerRecord {
        PeerRecord {
            url: url.to_owned(),
            node: None,
            added_at: "2026-05-21T00:00:00Z".to_owned(),
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

    fn successful_probe(pubkey: Option<&str>) -> ProbePeerResult {
        ProbePeerResult {
            node: Some("white".to_owned()),
            nickname: Some("White".to_owned()),
            pubkey: pubkey.map(str::to_owned),
            identity: None,
            error: None,
        }
    }

    #[test]
    fn peer_store_parent_dir_helper_tolerates_parentless_paths() {
        create_peer_store_parent_dir(Path::new("")).expect("parentless path is already usable");
    }

    #[test]
    fn save_and_mutate_create_peer_store_parent_dirs() {
        let home = temp_dir("store-parent");
        let env = PeerStoreEnv::new(&home);
        let mut data = empty_peer_store();
        data.peers
            .insert("white".to_owned(), peer_record("http://white:3456"));

        save_peer_store(&env, &data).expect("save peer store");
        assert!(peer_store_path(&env).exists());

        let updated = mutate_peer_store(&env, |store| {
            store
                .peers
                .insert("mba".to_owned(), peer_record("http://mba:3456"));
        })
        .expect("mutate peer store");

        assert!(updated.peers.contains_key("white"));
        assert!(updated.peers.contains_key("mba"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn peer_add_authenticated_and_probe_pubkey_mismatch_preserves_existing_store() {
        let mut peers = BTreeMap::new();
        peers.insert("white".to_owned(), peer_record("http://old:3456"));
        let plan = PeerAddPlan {
            alias: "white".to_owned(),
            url: "http://white:3456".to_owned(),
            node: None,
            authenticated_pubkey: Some("auth-key".to_owned()),
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers,
            probe: successful_probe(Some("probe-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add mismatch result");

        assert!(result.overwrote);
        assert_eq!(result.peer.url, "http://old:3456");
        assert_eq!(
            result.pubkey_mismatch,
            Some(PeerPubkeyMismatchError::new(
                "white",
                "auth-key",
                "probe-key"
            ))
        );
        assert_eq!(result.peers_after, plan.peers);
    }

    #[test]
    fn peer_add_authenticated_probe_mismatch_without_existing_peer_stays_empty() {
        let plan = PeerAddPlan {
            alias: "new-peer".to_owned(),
            url: "http://new-peer:3456".to_owned(),
            node: None,
            authenticated_pubkey: Some("auth-key".to_owned()),
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::new(),
            probe: successful_probe(Some("probe-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add mismatch result");

        assert!(!result.overwrote);
        assert_eq!(result.peer.url, "http://new-peer:3456");
        assert_eq!(
            result.pubkey_mismatch,
            Some(PeerPubkeyMismatchError::new(
                "new-peer",
                "auth-key",
                "probe-key"
            ))
        );
        assert!(result.peers_after.is_empty());
    }

    #[test]
    fn peer_add_allows_matching_authenticated_and_probe_pubkeys() {
        let plan = PeerAddPlan {
            alias: "white".to_owned(),
            url: "http://white:3456".to_owned(),
            node: None,
            authenticated_pubkey: Some("same-key".to_owned()),
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::new(),
            probe: successful_probe(Some("same-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add succeeds");

        assert_eq!(result.pubkey_mismatch, None);
        assert_eq!(result.peer.pubkey.as_deref(), Some("same-key"));
        assert_eq!(
            result.peers_after["white"].pubkey.as_deref(),
            Some("same-key")
        );
    }

    #[test]
    fn unreadable_peer_store_path_returns_empty_store() {
        let dir = temp_dir("unreadable");
        fs::create_dir_all(&dir).expect("create dir path");

        assert_eq!(read_peer_store_unlocked(&dir), empty_peer_store());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_iso_month_hits_zero_day_count() {
        assert_eq!(days_in_month(2026, 13), 0);
        assert_eq!(parse_iso_timestamp_ms("2026-13-01T00:00:00Z"), None);
    }
