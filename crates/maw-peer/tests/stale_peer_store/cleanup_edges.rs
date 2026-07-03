struct TestDir {
    path: PathBuf,
}

const DAY_MS: u64 = 24 * 60 * 60 * 1000;
const NOW_MS: u64 = 1_779_062_400_000; // 2026-05-18T00:00:00.000Z

fn iso_days_ago(days: u64) -> String {
    let ms = NOW_MS - days * DAY_MS;
    match days {
        1 => "2026-05-17T00:00:00.000Z",
        3 => "2026-05-15T00:00:00.000Z",
        4 => "2026-05-14T00:00:00.000Z",
        7 => "2026-05-11T00:00:00.000Z",
        8 => "2026-05-10T00:00:00.000Z",
        9 => "2026-05-09T00:00:00.000Z",
        10 => "2026-05-08T00:00:00.000Z",
        20 => "2026-04-28T00:00:00.000Z",
        30 => "2026-04-18T00:00:00.000Z",
        40 => "2026-04-08T00:00:00.000Z",
        _ => panic!("unexpected fixture day offset {days}; ms={ms}"),
    }
    .to_owned()
}

fn store_from<const N: usize>(peers: [(&str, &str, String, Option<String>); N]) -> PeerStoreFile {
    let peers = peers
        .into_iter()
        .map(|(alias, url, added_at, last_seen)| {
            (
                alias.to_owned(),
                PeerRecord {
                    url: url.to_owned(),
                    node: None,
                    added_at,
                    last_seen,
                    last_error: None,
                    nickname: None,
                    pubkey: None,
                    pubkey_first_seen: None,
                    identity: None,
                    one_way: None,
                    last_symmetric_check: None,
                },
            )
        })
        .collect();
    PeerStoreFile { version: 1, peers }
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

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn valid_iso_timestamps_drive_stale_age_save_and_removal_paths() {
    assert_eq!(
        stale_age_ms(&peer("1970-01-01T00:00:00.001Z", None), 2),
        Some(1)
    );
    assert_eq!(
        stale_age_ms(&peer("2026-05-21T12:34:56.789Z", None), 1_779_366_896_790),
        Some(1)
    );

    let tmp = TestDir::new("maw-rs-valid-iso-removal");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [
            ("PEERS_FILE", file.to_string_lossy().into_owned()),
            ("MAW_PEER_STALE_TTL_MS", DAY_MS.to_string()),
        ],
    );
    save_peer_store(
        &env,
        &store_from([("old", "http://old.local", iso_days_ago(3), None)]),
    )
    .expect("save peer store through atomic JSON writer");

    let removed = remove_stale_peers(&env, NOW_MS).expect("remove stale peer");
    assert_eq!(removed.message, "removed 1 stale peer");
    assert!(load_peer_store(&env).peers.is_empty());
}

#[test]
fn remove_stale_peers_reports_plural_removed_count() {
    let tmp = TestDir::new("maw-rs-plural-stale-removal");
    let file = tmp.path().join("peers.json");
    let env = PeerStoreEnv::with_vars(
        tmp.path(),
        [
            ("PEERS_FILE", file.to_string_lossy().into_owned()),
            ("MAW_PEER_STALE_TTL_MS", DAY_MS.to_string()),
        ],
    );
    save_peer_store(
        &env,
        &store_from([
            ("alpha", "http://alpha.local", iso_days_ago(4), None),
            ("bravo", "http://bravo.local", iso_days_ago(7), None),
        ]),
    )
    .expect("save peer store through atomic JSON writer");

    let removed = remove_stale_peers(&env, NOW_MS).expect("remove stale peers");

    assert_eq!(removed.message, "removed 2 stale peers");
    assert!(load_peer_store(&env).peers.is_empty());
}
