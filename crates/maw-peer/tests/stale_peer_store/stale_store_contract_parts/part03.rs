
#[test]
fn stale_peer_enumeration_matches_maw_js_stable_ordering_and_age_fallback() {
    let tmp = TestDir::new("maw-rs-stale-peer-enumeration");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [
            ("PEERS_FILE", file.to_string_lossy().into_owned()),
            ("MAW_PEER_STALE_TTL_MS", (7 * DAY_MS).to_string()),
        ],
    );
    save_peer_store(
        &env,
        &store_from([
            (
                "zebra",
                "http://zebra.local",
                iso_days_ago(40),
                Some(iso_days_ago(10)),
            ),
            (
                "fresh",
                "http://fresh.local",
                iso_days_ago(20),
                Some(iso_days_ago(1)),
            ),
            ("alpha", "http://alpha.local", iso_days_ago(8), None),
            (
                "exactTtl",
                "http://exact.local",
                iso_days_ago(30),
                Some(iso_days_ago(7)),
            ),
            (
                "brokenClock",
                "http://broken.local",
                "not-a-date".to_owned(),
                None,
            ),
        ]),
    )
    .unwrap();

    let stale = stale_peers(&env, NOW_MS);
    assert_eq!(stale.len(), 3);
    assert_eq!(stale[0].alias, "alpha");
    assert_eq!(stale[0].url, "http://alpha.local");
    assert_eq!(stale[0].age_ms, Some(8 * DAY_MS));
    assert_eq!(stale[1].alias, "brokenClock");
    assert_eq!(stale[1].url, "http://broken.local");
    assert_eq!(stale[1].age_ms, None);
    assert_eq!(stale[2].alias, "zebra");
    assert_eq!(stale[2].url, "http://zebra.local");
    assert_eq!(stale[2].age_ms, Some(10 * DAY_MS));
}

#[test]
fn stale_peer_check_matches_maw_js_no_stale_singular_and_plural_messages() {
    let tmp = TestDir::new("maw-rs-stale-peer-check");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [
            ("PEERS_FILE", file.to_string_lossy().into_owned()),
            ("MAW_PEER_STALE_TTL_MS", (2 * DAY_MS).to_string()),
        ],
    );
    save_peer_store(
        &env,
        &store_from([("fresh", "http://fresh.local", iso_days_ago(1), None)]),
    )
    .unwrap();
    assert_eq!(stale_peer_check(&env, NOW_MS).name, "peers:stale");
    assert!(stale_peer_check(&env, NOW_MS).ok);
    assert_eq!(stale_peer_check(&env, NOW_MS).message, "no stale peers");

    save_peer_store(
        &env,
        &store_from([("old", "http://old.local", iso_days_ago(3), None)]),
    )
    .unwrap();
    let singular = stale_peer_check(&env, NOW_MS);
    assert!(!singular.ok);
    assert_eq!(
        singular.message,
        "1 stale peer (>2d) — run 'maw doctor --fix-stale' to remove"
    );

    save_peer_store(
        &env,
        &store_from([
            ("old", "http://old.local", iso_days_ago(3), None),
            ("older", "http://older.local", iso_days_ago(4), None),
        ]),
    )
    .unwrap();
    let plural = stale_peer_check(&env, NOW_MS);
    assert!(!plural.ok);
    assert_eq!(
        plural.message,
        "2 stale peers (>2d) — run 'maw doctor --fix-stale' to remove"
    );
}

#[test]
fn remove_stale_peers_preserves_fresh_peers_and_reports_maw_js_messages() {
    let tmp = TestDir::new("maw-rs-stale-peer-remove");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [
            ("PEERS_FILE", file.to_string_lossy().into_owned()),
            ("MAW_PEER_STALE_TTL_MS", (7 * DAY_MS).to_string()),
        ],
    );
    save_peer_store(
        &env,
        &store_from([
            (
                "old",
                "http://old.local",
                iso_days_ago(30),
                Some(iso_days_ago(9)),
            ),
            ("never", "http://never.local", iso_days_ago(10), None),
            (
                "fresh",
                "http://fresh.local",
                iso_days_ago(30),
                Some(iso_days_ago(1)),
            ),
        ]),
    )
    .unwrap();

    let result = remove_stale_peers(&env, NOW_MS).unwrap();
    assert_eq!(result.name, "peers:fix-stale");
    assert!(result.ok);
    assert_eq!(result.message, "removed 2 stale peers");
    assert_eq!(
        load_peer_store(&env)
            .peers
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        vec!["fresh"]
    );

    let result = remove_stale_peers(&env, NOW_MS).unwrap();
    assert!(result.ok);
    assert_eq!(result.message, "no stale peers");
}

