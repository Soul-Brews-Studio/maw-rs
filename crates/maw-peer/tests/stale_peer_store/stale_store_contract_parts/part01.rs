use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use maw_peer::{
    default_stale_ttl_ms, empty_peer_store, is_peer_stale, load_peer_store, mutate_peer_store,
    parse_stale_ttl_ms, peer_store_path, remove_stale_peers, save_peer_store, stale_age_ms,
    stale_peer_check, stale_peers, PeerRecord, PeerStoreEnv, PeerStoreFile,
};

fn peer(added_at: &str, last_seen: Option<&str>) -> PeerRecord {
    PeerRecord {
        url: "u".to_owned(),
        node: None,
        added_at: added_at.to_owned(),
        last_seen: last_seen.map(str::to_owned),
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
fn stale_ttl_parsing_matches_maw_js_env_contract() {
    assert_eq!(default_stale_ttl_ms(), 7 * 24 * 60 * 60 * 1000);
    assert_eq!(parse_stale_ttl_ms(None), default_stale_ttl_ms());
    assert_eq!(parse_stale_ttl_ms(Some("1234")), 1234);
    assert_eq!(parse_stale_ttl_ms(Some("0")), default_stale_ttl_ms());
    assert_eq!(parse_stale_ttl_ms(Some("-1")), default_stale_ttl_ms());
    assert_eq!(
        parse_stale_ttl_ms(Some("not-a-number")),
        default_stale_ttl_ms()
    );
    assert_eq!(parse_stale_ttl_ms(Some("")), default_stale_ttl_ms());
}

#[test]
fn stale_age_uses_last_seen_then_added_at_and_clamps_future_dates() {
    let now = 1_779_105_600_000; // 2026-05-18T12:00:00.000Z

    assert_eq!(
        stale_age_ms(&peer("2026-05-18T11:59:50.000Z", None), now),
        Some(10_000)
    );
    assert_eq!(
        stale_age_ms(
            &peer("2026-05-18T00:00:00.000Z", Some("2026-05-18T12:00:05.000Z")),
            now,
        ),
        Some(0)
    );
    assert_eq!(stale_age_ms(&peer("not-date", None), now), None);
    assert_eq!(
        stale_age_ms(&peer("2026-05-18T00:00:00.000Zx", None), now),
        None
    );
    assert_eq!(
        stale_age_ms(&peer("2026-05-18-extraT00:00:00.000Z", None), now),
        None
    );
    assert_eq!(
        stale_age_ms(
            &peer("2026-05-18T00:00:00.000Z", Some("2026-05-18T00:00:00:00Z")),
            now
        ),
        None
    );
    assert_eq!(
        stale_age_ms(&peer("2026-02-30T00:00:00.000Z", None), now),
        None
    );
    assert_eq!(
        stale_age_ms(&peer("2026-05-18T24:00:00.000Z", None), now),
        None
    );
    assert_eq!(
        stale_age_ms(&peer("2024-02-29T00:00:00.1Z", None), 1_709_164_800_100),
        Some(0)
    );
    assert_eq!(
        stale_age_ms(&peer("2026-04-30T00:00:00.12Z", None), 1_777_507_200_120),
        Some(0)
    );
    assert_eq!(
        stale_age_ms(&peer("2026-05-18T00:00:00.Z", None), now),
        None
    );
    assert_eq!(
        stale_age_ms(&peer("2026-05-18T00:00:00.aZ", None), now),
        None
    );
    for invalid in [
        "year-05-18T00:00:00.000Z",
        "2026-month-18T00:00:00.000Z",
        "2026-05-dayT00:00:00.000Z",
        "2026-05-18Thour:00:00.000Z",
        "2026-05-18T00:minute:00.000Z",
        "2026-05-18T00:00Z",
    ] {
        assert_eq!(stale_age_ms(&peer(invalid, None), now), None, "{invalid}");
    }
}

#[test]
fn is_peer_stale_matches_maw_js_threshold_and_invalid_provenance_rules() {
    let now = 1_779_105_600_000;
    let ten_seconds_old = peer("2026-05-18T11:59:50.000Z", None);

    assert!(is_peer_stale(&ten_seconds_old, 9_999, now));
    assert!(!is_peer_stale(&ten_seconds_old, 10_000, now));
    assert!(is_peer_stale(&peer("not-date", None), 10_000, now));
}

#[test]
fn peer_store_path_empty_stale_tmp_save_and_load_round_trip_match_maw_js() {
    let tmp = TestDir::new("maw-rs-peer-store-round-trip");
    assert!(peer_store_path(&PeerStoreEnv::new(tmp.path())).ends_with("peers.json"));
    let file = tmp.path().join("nested").join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );

    assert_eq!(peer_store_path(&env), file);
    assert_eq!(empty_peer_store(), PeerStoreFile::default());
    assert_eq!(load_peer_store(&env), PeerStoreFile::default());

    let mut peers = BTreeMap::new();
    peers.insert(
        "alpha".to_owned(),
        PeerRecord {
            url: "http://alpha.local:3210".to_owned(),
            node: Some("alpha-node".to_owned()),
            added_at: "2026-05-18T00:00:00.000Z".to_owned(),
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
    save_peer_store(&env, &PeerStoreFile { version: 1, peers }).unwrap();
    fs::write(format!("{}.tmp", file.display()), "stale partial write").unwrap();

    assert!(PathBuf::from(format!("{}.tmp", file.display())).exists());
    assert_eq!(
        load_peer_store(&env).peers["alpha"].node.as_deref(),
        Some("alpha-node")
    );
    assert!(!PathBuf::from(format!("{}.tmp", file.display())).exists());
    assert!(fs::read_to_string(file).unwrap().contains("alpha-node"));
}

#[test]
fn peer_store_defaults_missing_and_shorthand_json_shapes_like_maw_js() {
    let tmp = TestDir::new("maw-rs-peer-store-default-shapes");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [("PEERS_FILE", file.to_string_lossy().into_owned())],
    );

    let mutated = mutate_peer_store(&env, |store| {
        assert!(store.peers.is_empty());
        store
            .peers
            .insert("added".to_owned(), peer("2026-05-18T00:00:00.000Z", None));
    })
    .unwrap();
    assert!(mutated.peers.contains_key("added"));

    fs::write(&file, "{}\n").unwrap();
    assert_eq!(load_peer_store(&env), PeerStoreFile::default());
}
