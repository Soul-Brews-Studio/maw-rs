
#[test]
fn probe_all_skips_peers_removed_between_load_and_mutate() {
    let refused = error(ProbeErrorCode::Refused, "closed port");
    let result = probe_all_from_plan(&ProbeAllPlan {
        timeout_ms: 2000,
        now: "2026-05-18T12:00:00.000Z".to_owned(),
        peers: vec![
            (
                "gone".to_owned(),
                peer("http://gone.local", None, None, None),
            ),
            ("ok".to_owned(), peer("http://ok.local", None, None, None)),
        ],
        probe_results: vec![
            ("http://gone.local".to_owned(), failed(refused), 0),
            ("http://ok.local".to_owned(), ok("ok-node"), 0),
        ],
        removed_before_mutate: vec!["gone".to_owned()],
    });

    assert_eq!(
        result
            .rows
            .iter()
            .map(|row| row.alias.as_str())
            .collect::<Vec<_>>(),
        vec!["gone", "ok"]
    );
    assert!(!result.peers_after.contains_key("gone"));
    assert_eq!(
        result
            .peers_after
            .get("ok")
            .and_then(|peer| peer.node.as_deref()),
        Some("ok-node")
    );
    assert_eq!(result.mutate_calls, 1);
}

#[test]
fn format_probe_all_matches_maw_js_empty_and_colored_table_contract() {
    assert_eq!(
        format_probe_all(&ProbeAllResult {
            rows: vec![],
            ok_count: 0,
            fail_count: 0,
            worst_exit_code: 0,
            probe_calls: vec![],
            mutate_calls: 0,
            peers_after: BTreeMap::default(),
        }),
        "no peers"
    );

    let output = format_probe_all(&ProbeAllResult {
        ok_count: 1,
        fail_count: 1,
        worst_exit_code: 6,
        probe_calls: vec![],
        mutate_calls: 0,
        peers_after: BTreeMap::default(),
        rows: vec![
            ProbeAllRow {
                alias: "alpha".to_owned(),
                url: "http://alpha.local".to_owned(),
                node: Some("alpha-node".to_owned()),
                last_seen: Some("2026-05-18T00:00:00.000Z".to_owned()),
                ok: true,
                ms: 12,
                error: None,
            },
            ProbeAllRow {
                alias: "beta".to_owned(),
                url: "http://beta.local".to_owned(),
                node: None,
                last_seen: None,
                ok: false,
                ms: 5,
                error: Some(error(ProbeErrorCode::Http5xx, "boom")),
            },
        ],
    });

    assert!(output.contains("alias"));
    assert!(output.contains("alpha-node"));
    assert!(output.contains("\u{1b}[32m✓\u{1b}[0m ok (12ms)"));
    assert!(output.contains("\u{1b}[31m✗\u{1b}[0m HTTP_5XX"));
    assert!(output.contains("1/2 ok, 1 failed"));
}
