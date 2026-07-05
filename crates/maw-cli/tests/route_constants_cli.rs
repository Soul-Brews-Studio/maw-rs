use maw_cli::{run_cli, CliOutput};

#[test]
fn route_constants_plan_json_locks_maw_js_routing_vocabulary() {
    let output = run_cli(&[
        "route".to_owned(),
        "constants".to_owned(),
        "--plan-json".to_owned(),
    ]);

    assert_eq!(
        output,
        CliOutput {
            code: 0,
            stdout: concat!(
                "{\"command\":\"route\",\"kind\":\"constants\",",
                "\"resultTypes\":[\"local\",\"peer\",\"self-node\",\"error\"],",
                "\"inputs\":[\"query\",\"node\",\"named-peer\",\"peer\",\"agent\",\"session\",\"source\",\"window\"],",
                "\"windowShape\":\"index:name:active\",\"keyValueShapes\":{\"namedPeer\":\"name=url\",\"agent\":\"agent=node\"},",
                "\"precedence\":[\"empty-query-error\",\"filter-writable-local-sessions\",\"bare-session-alias-window\",\"direct-local-window\",\"node-agent-prefix\",\"agents-map\",\"not-found\"],",
                "\"localFilters\":{\"ignoreViewSessions\":true,\"localSourceOnly\":true},",
                "\"nodeRouting\":{\"selfAliases\":[\"configured-node\",\"local\"],\"peerSources\":[\"namedPeers exact name\",\"legacy peers URL contains node\"],\"slashDisablesNodeRouting\":true,\"multipleColonsKeepAgentSuffix\":true},",
                "\"aliasRules\":[\"skip queries ending -oracle\",\"strip numeric fleet session prefix\",\"prefer oracle-named window\",\"single-window session fallback\",\"refuse ambiguous session aliases\",\"refuse first-window fallback for multi-window alias miss\"],",
                "\"errorReasons\":[\"empty_query\",\"self_not_running\",\"unknown_node\",\"no_peer_url\",\"not_found\",\"session_alias_ambiguous\",\"session_window_not_found\"],",
                "\"fixtureCounts\":{\"total\":20,\"local\":7,\"peer\":4,\"self-node\":1,\"error\":8}}\n"
            )
            .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn route_constants_plan_rejects_unknown_flags() {
    let output = run_cli(&[
        "route".to_owned(),
        "constants".to_owned(),
        "--bad".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output
        .stderr
        .contains("route constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs route constants"));
}
