use std::collections::BTreeMap;

use maw_peer::{
    format_probe_all, probe_all_from_plan, PeerRecord, ProbeAllPlan, ProbeAllResult, ProbeAllRow,
    ProbeErrorCode, ProbeLastError, ProbePeerResult,
};

fn error(code: ProbeErrorCode, message: &str) -> ProbeLastError {
    ProbeLastError {
        code,
        message: message.to_owned(),
        at: "2026-05-18T00:00:00.000Z".to_owned(),
    }
}

fn peer(
    url: &str,
    node: Option<&str>,
    last_seen: Option<&str>,
    last_error: Option<ProbeLastError>,
) -> PeerRecord {
    PeerRecord {
        url: url.to_owned(),
        node: node.map(str::to_owned),
        added_at: "2026-05-17T00:00:00.000Z".to_owned(),
        last_seen: last_seen.map(str::to_owned),
        last_error,
        nickname: None,
        pubkey: None,
        pubkey_first_seen: None,
        identity: None,
        one_way: None,
        last_symmetric_check: None,
    }
}

fn ok(node: &str) -> ProbePeerResult {
    ProbePeerResult {
        node: Some(node.to_owned()),
        nickname: None,
        pubkey: None,
        identity: None,
        error: None,
    }
}

fn failed(err: ProbeLastError) -> ProbePeerResult {
    ProbePeerResult {
        node: None,
        nickname: None,
        pubkey: None,
        identity: None,
        error: Some(err),
    }
}

#[test]
fn probe_all_probes_alias_order_and_batch_mutates_successes_and_failures() {
    let prior_failure = error(ProbeErrorCode::Refused, "old refusal");
    let dns_failure = error(ProbeErrorCode::Dns, "host not found");
    let timeout_failure = error(ProbeErrorCode::Timeout, "too slow");
    let plan = ProbeAllPlan {
        timeout_ms: 321,
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: vec![
            (
                "zebra".to_owned(),
                peer(
                    "http://zebra.local",
                    Some("old-zebra"),
                    Some("2026-05-01T00:00:00.000Z"),
                    Some(prior_failure),
                ),
            ),
            (
                "alpha".to_owned(),
                peer(
                    "http://alpha.local",
                    Some("old-alpha"),
                    Some("2026-05-02T00:00:00.000Z"),
                    None,
                ),
            ),
            (
                "beta".to_owned(),
                peer("http://beta.local", Some("old-beta"), None, None),
            ),
        ],
        probe_results: vec![
            ("http://alpha.local".to_owned(), ok("new-alpha"), 7),
            (
                "http://beta.local".to_owned(),
                failed(dns_failure.clone()),
                7,
            ),
            (
                "http://zebra.local".to_owned(),
                failed(timeout_failure.clone()),
                7,
            ),
        ],
        removed_before_mutate: vec![],
    };

    let result = probe_all_from_plan(&plan);

    assert_eq!(
        result.probe_calls,
        vec![
            ("http://alpha.local".to_owned(), 321),
            ("http://beta.local".to_owned(), 321),
            ("http://zebra.local".to_owned(), 321),
        ]
    );
    assert_eq!(result.mutate_calls, 1);
    assert_eq!(result.ok_count, 1);
    assert_eq!(result.fail_count, 2);
    assert_eq!(result.worst_exit_code, 5);
    assert_eq!(
        result
            .rows
            .iter()
            .map(|row| row.alias.as_str())
            .collect::<Vec<_>>(),
        vec!["alpha", "beta", "zebra"]
    );
    assert_eq!(result.rows[0].node.as_deref(), Some("new-alpha"));
    assert_eq!(
        result.rows[0].last_seen.as_deref(),
        Some("2026-05-18T12:00:00.000Z")
    );
    assert_eq!(result.rows[1].node.as_deref(), Some("old-beta"));
    assert_eq!(result.rows[1].error, Some(dns_failure));
    assert_eq!(result.rows[2].node.as_deref(), Some("old-zebra"));
    assert_eq!(result.rows[2].error, Some(timeout_failure));
    assert_eq!(
        result
            .peers_after
            .get("alpha")
            .and_then(|peer| peer.node.as_deref()),
        Some("new-alpha")
    );
    assert!(result
        .peers_after
        .get("alpha")
        .and_then(|peer| peer.last_error.as_ref())
        .is_none());
    assert_eq!(
        result
            .peers_after
            .get("beta")
            .and_then(|peer| peer.last_error.as_ref())
            .map(|err| err.code),
        Some(ProbeErrorCode::Dns)
    );
}

#[test]
fn probe_all_does_not_mutate_empty_store() {
    let result = probe_all_from_plan(&ProbeAllPlan {
        timeout_ms: 2000,
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: vec![],
        probe_results: vec![],
        removed_before_mutate: vec![],
    });

    assert_eq!(result.rows, Vec::<ProbeAllRow>::new());
    assert_eq!(result.ok_count, 0);
    assert_eq!(result.fail_count, 0);
    assert_eq!(result.worst_exit_code, 0);
    assert_eq!(result.probe_calls, Vec::<(String, u64)>::new());
    assert_eq!(result.mutate_calls, 0);
}

#[test]
fn probe_all_missing_probe_result_counts_as_unknown_success_and_all_ok_format_has_no_failure_suffix(
) {
    let result = probe_all_from_plan(&ProbeAllPlan {
        timeout_ms: 2000,
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: vec![(
            "solo".to_owned(),
            peer("http://solo.local", Some("cached-node"), None, None),
        )],
        probe_results: vec![],
        removed_before_mutate: vec![],
    });

    assert_eq!(result.ok_count, 1);
    assert_eq!(result.fail_count, 0);
    assert_eq!(result.worst_exit_code, 0);
    assert_eq!(result.rows[0].node.as_deref(), Some("cached-node"));
    assert_eq!(
        result.rows[0].last_seen.as_deref(),
        Some("2026-05-18T12:00:00.000Z")
    );
    assert_eq!(
        result
            .peers_after
            .get("solo")
            .and_then(|peer| peer.last_seen.as_deref()),
        Some("2026-05-18T12:00:00.000Z")
    );

    let output = format_probe_all(&result);
    assert!(output.contains("1/1 ok"), "{output}");
    assert!(!output.contains("failed"), "{output}");
}
