use maw_cli::run_cli;

#[test]
fn bind_host_plan_cli_matches_maw_js_heuristic_cases() {
    let cases = [
        (
            "loopback when nothing is configured",
            vec![],
            "127.0.0.1",
            None,
        ),
        (
            "config peers populated",
            vec![("--config-peers-len", "1")],
            "0.0.0.0",
            Some("config.peers"),
        ),
        (
            "config named peers populated",
            vec![("--config-named-peers-len", "1")],
            "0.0.0.0",
            Some("config.namedPeers"),
        ),
        (
            "maw host zero env opt in",
            vec![("--maw-host", "0.0.0.0")],
            "0.0.0.0",
            Some("MAW_HOST"),
        ),
        (
            "peers json non empty",
            vec![("--peers-store-len", "1")],
            "0.0.0.0",
            Some("peers.json"),
        ),
        (
            "empty peers json stays loopback",
            vec![("--peers-store-len", "0")],
            "127.0.0.1",
            None,
        ),
        (
            "maw host non zero does not trigger",
            vec![("--maw-host", "white")],
            "127.0.0.1",
            None,
        ),
        (
            "peers store error falls through",
            vec![("--peers-store-error", "disk read failed")],
            "127.0.0.1",
            None,
        ),
        (
            "config peers takes priority over maw host",
            vec![("--config-peers-len", "1"), ("--maw-host", "0.0.0.0")],
            "0.0.0.0",
            Some("config.peers"),
        ),
    ];

    for (name, flags, expected_host, expected_reason) in cases {
        let mut argv = vec!["bind-host".to_owned(), "--plan-json".to_owned()];
        for (flag, value) in flags {
            argv.push(flag.to_owned());
            argv.push(value.to_owned());
        }
        let output = run_cli(&argv);
        assert_eq!(output.code, 0, "{name}: {}", output.stderr);
        let json: serde_json::Value = serde_json::from_str(&output.stdout)
            .unwrap_or_else(|error| panic!("{name} invalid json: {error}\n{}", output.stdout));
        assert_eq!(json["command"], "bind-host", "{name}");
        assert_eq!(json["hostname"], expected_host, "{name}");
        match expected_reason {
            Some(reason) => assert_eq!(json["reason"], reason, "{name}"),
            None => assert!(json["reason"].is_null(), "{name}: {}", output.stdout),
        }
    }
}

#[test]
fn bind_host_plan_rejects_bad_counts() {
    let output = run_cli(&[
        "bind-host".to_owned(),
        "--config-peers-len".to_owned(),
        "many".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("--config-peers-len must be"));
}
