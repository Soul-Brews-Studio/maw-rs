
    #[test]
    fn private_iso_parser_rejects_malformed_components_and_handles_negative_eras() {
        for invalid in [
            "2026-05-21T00:00:00",
            "2026-05-2100:00:00Z",
            "year-05-21T00:00:00Z",
            "2026-month-21T00:00:00Z",
            "2026-05-dayT00:00:00Z",
            "2026-05-21Thour:00:00Z",
            "2026-05-21T00:minute:00Z",
            "2026-05-21T00:00:secondZ",
            "2026-05-21T00:00:00.badZ",
            "2026-05-21T00:00:00.Z",
            "2026-02-29T00:00:00Z",
            "2026-05-21T24:00:00Z",
            "2026-05-21T00:60:00Z",
            "2026-05-21T00:00:60Z",
        ] {
            assert_eq!(parse_iso_timestamp_ms(invalid), None, "{invalid}");
        }

        for (input, expected) in [
            ("2026-05-21T00:00:00.7Z", 1_779_321_600_700),
            ("2026-05-21T00:00:00.78Z", 1_779_321_600_780),
            ("2026-05-21T00:00:00.7899Z", 1_779_321_600_789),
        ] {
            assert_eq!(parse_iso_timestamp_ms(input), Some(expected), "{input}");
        }
        assert!(days_from_civil(-1, 3, 1) < 0);
    }

    #[test]
    fn peer_store_io_and_parser_error_edges_surface_without_mutation() {
        let blocked_parent = temp_dir("blocked-parent");
        fs::write(&blocked_parent, "not a directory").expect("write blocked parent");
        let blocked_file = blocked_parent.join("peers.json").display().to_string();
        let blocked_env =
            PeerStoreEnv::with_vars(temp_dir("blocked-home"), [("PEERS_FILE", blocked_file)]);

        let save_err = save_peer_store(&blocked_env, &empty_peer_store())
            .expect_err("file parent prevents save mkdir");
        assert_eq!(save_err.kind(), io::ErrorKind::AlreadyExists);
        let _ = fs::remove_file(&blocked_parent);

        let dir_path = temp_dir("rename-target-dir");
        fs::create_dir_all(&dir_path).expect("create directory target");
        let dir_env = PeerStoreEnv::with_vars(
            temp_dir("dir-home"),
            [("PEERS_FILE", dir_path.display().to_string())],
        );
        assert!(mutate_peer_store(&dir_env, |_| {}).is_err());
        let _ = fs::remove_dir_all(&dir_path);

        assert!(
            parse_peer_store(r#"{"peers":{"bad":{"url":1,"addedAt":2}}}"#)
                .expect_err("bad peer shape")
                .contains("invalid type")
        );
    }

    #[test]
    fn probe_mismatch_result_falls_back_to_existing_node() {
        let mut existing = peer_record("http://white:3456");
        existing.node = Some("white-node".to_owned());
        existing.pubkey = Some("cached-key".to_owned());
        let plan = PeerProbePlan {
            alias: "white".to_owned(),
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::from([("white".to_owned(), existing)]),
            probe: ProbePeerResult {
                node: None,
                nickname: None,
                pubkey: Some("observed-key".to_owned()),
                identity: None,
                error: None,
            },
            remove_before_mutate: false,
        };

        let result = cmd_peer_probe_from_plan(&plan).expect("probe mismatch result");

        assert_eq!(result.node.as_deref(), Some("white-node"));
        assert_eq!(
            result.pubkey_mismatch,
            Some(PeerPubkeyMismatchError::new(
                "white",
                "cached-key",
                "observed-key"
            ))
        );
    }
