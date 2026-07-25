use maw_peer::{
    probe_all_from_plan, PeerRecord, ProbeAllPlan, ProbeErrorCode, ProbeLastError, ProbePeerResult,
};

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
        auth_ok: None,
    }
}

#[test]
fn probe_all_failed_existing_peer_records_last_error() {
    let err = ProbeLastError {
        code: ProbeErrorCode::Timeout,
        message: "timed out".to_owned(),
        at: "2026-05-21T00:00:05Z".to_owned(),
    };
    let plan = ProbeAllPlan {
        timeout_ms: 5000,
        now: "2026-05-21T00:00:10Z".to_owned(),
        peers: vec![("white".to_owned(), peer_record("http://white:3456"))],
        probe_results: vec![(
            "http://white:3456".to_owned(),
            ProbePeerResult {
                node: None,
                nickname: None,
                pubkey: None,
                identity: None,
                error: Some(err.clone()),
                ..Default::default()
            },
            5000,
        )],
        removed_before_mutate: Vec::new(),
    };

    let result = probe_all_from_plan(&plan);

    assert_eq!(result.fail_count, 1);
    assert_eq!(result.peers_after["white"].last_error, Some(err));
}
