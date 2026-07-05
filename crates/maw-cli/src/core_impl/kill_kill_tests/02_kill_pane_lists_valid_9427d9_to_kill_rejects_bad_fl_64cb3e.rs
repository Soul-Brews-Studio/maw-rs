    #[test]
    fn kill_pane_lists_valid_indexes_before_kill_pane() {
        let mut tmux = kill_fake("07-demo|||1|||main|||1|||/tmp\n");
        tmux.pane_indexes_raw = "0\n2\n".to_owned();
        let output = kill_run_fake(&kill_strings(&["demo:1", "--pane", "2"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert_eq!(
            output.stdout,
            "  \x1b[32m✓\x1b[0m killed pane 07-demo:1.2\n"
        );
        assert_eq!(
            tmux.calls[1],
            (
                "list-panes".to_owned(),
                kill_strings(&["-t", "07-demo:1", "-F", "#{pane_index}"])
            )
        );
        assert_eq!(
            tmux.calls[2],
            ("kill-pane".to_owned(), kill_strings(&["-t", "07-demo:1.2"]))
        );
    }

    #[test]
    fn kill_pane_rejects_missing_pane_without_kill() {
        let mut tmux = kill_fake("07-demo|||1|||main|||1|||/tmp\n");
        tmux.pane_indexes_raw = "0\n".to_owned();
        let output = kill_run_fake(&kill_strings(&["demo:1", "--pane=2"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("pane 2 does not exist"));
        assert!(!tmux.calls.iter().any(|call| call.0 == "kill-pane"));
    }

    #[test]
    fn kill_orphan_pane_fallback_uses_pane_resolver_before_kill() {
        let mut tmux = kill_fake("");
        tmux.panes_all_raw = "%42|||07-demo:1.0|||agent|||role|||/repo/demo\n".to_owned();
        let output = kill_run_fake(&kill_strings(&["agent"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("killed pane agent → %42"));
        assert_eq!(tmux.calls[0].0, "list-windows");
        assert_eq!(tmux.calls[1].0, "list-panes");
        assert_eq!(
            tmux.calls[2],
            ("kill-pane".to_owned(), kill_strings(&["-t", "%42"]))
        );
    }

    #[test]
    fn kill_missing_session_prints_hints_and_does_not_kill() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["dem"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("did you mean"));
        assert!(!tmux.calls.iter().any(|call| call.0.starts_with("kill-")));
    }



    #[test]
    fn kill_peer_forward_posts_signed_body_and_skips_local_tmux() {
        let env = KillEnvGuard::new("forward");
        env.write_peers(r#"{"version":1,"peers":{"neo":{"url":"http://peer.example:3456","node":"neo-node","addedAt":"1"}}}"#);
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let mut peer = KillFakePeer { response: Some(KillPeerResponse { output: Some("remote log\n".to_owned()) }), ..KillFakePeer::default() };
        let output = kill_run_command_with(
            &kill_strings(&["target", "--pane", "3", "--peer", "neo"]),
            &mut tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned()),
            || 1_700_000_000,
        );
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "\x1b[32m✓\x1b[0m forwarded kill → neo (http://peer.example:3456) — target\nremote log\n");
        assert!(tmux.calls.is_empty(), "peer kill must not touch local tmux");
        assert_eq!(peer.requests.len(), 1);
        let request = &peer.requests[0];
        assert_eq!(request.peer.alias, "neo");
        assert_eq!(request.peer.url, "http://peer.example:3456");
        assert_eq!(request.target, "target");
        assert_eq!(request.pane, Some(3));
        assert_eq!(request.from, "test-oracle:local");
    }

    #[test]
    fn kill_peer_unknown_alias_is_clean_error_without_transport() {
        let env = KillEnvGuard::new("missing");
        env.write_peers(r#"{"version":1,"peers":{}}"#);
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let mut peer = KillFakePeer::default();
        let output = kill_run_command_with(
            &kill_strings(&["target", "--peer", "missing"]),
            &mut tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("key".to_owned()),
            || 1_700_000_000,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("unknown peer alias: missing"));
        assert!(tmux.calls.is_empty());
        assert!(peer.requests.is_empty());
    }

    #[test]
    fn kill_peer_validates_alias_and_target_before_transport() {
        let env = KillEnvGuard::new("invalid");
        env.write_peers(r#"{"version":1,"peers":{"neo":{"url":"http://peer.example"}}}"#);
        let mut tmux = kill_fake("");
        let mut peer = KillFakePeer::default();
        let output = kill_run_command_with(
            &kill_strings(&["target", "--peer", "bad;alias"]),
            &mut tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("key".to_owned()),
            || 1_700_000_000,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("invalid peer alias"));
        assert!(peer.requests.is_empty());
    }

    #[test]
    fn kill_peer_body_and_curl_argv_are_argv_no_shell() {
        let request = KillPeerRequest {
            peer: KillPeer { alias: "neo".to_owned(), url: "http://peer".to_owned(), node: None },
            target: "target".to_owned(),
            pane: Some(1),
            index: Some(2),
            all: true,
            from: "oracle:node".to_owned(),
            peer_key: "key".to_owned(),
            timestamp: 1,
        };
        let body = kill_peer_body(&request).expect("body");
        let value = serde_json::from_str::<serde_json::Value>(&body).expect("json body");
        assert_eq!(value["target"], "target");
        assert_eq!(value["pane"], 1);
        assert_eq!(value["index"], 2);
        assert_eq!(value["all"], true);
        let headers = sign_headers_v3_at("key", "oracle:node", "POST", KILL_PEER_API_PATH, Some(body.as_bytes()), 1).expect("headers");
        let argv = kill_peer_curl_argv("http://peer/", &headers, &body).expect("argv");
        assert!(argv.iter().any(|arg| arg == "--"));
        assert!(argv.iter().any(|arg| arg == "http://peer/api/kill"));
        assert!(argv.windows(2).any(|pair| pair == ["--data-binary", body.as_str()]));
        assert!(!argv.iter().any(|arg| arg == "sh" || arg == "-c"));
    }

    #[test]
    fn kill_peer_response_maps_404_and_remote_errors_like_maw_js() {
        let unsupported = kill_parse_peer_response("neo", "http://peer", 404, r"{}").unwrap_err();
        assert_eq!(unsupported, "peer neo does not support /api/kill (HTTP 404 at http://peer)");
        let maintenance = kill_parse_peer_response("neo", "http://peer", 503, r#"{"error":"maintenance"}"#).unwrap_err();
        assert_eq!(maintenance, "peer kill failed (neo http://peer): maintenance");
        let ok = kill_parse_peer_response("neo", "http://peer", 200, r#"{"ok":true,"output":"remote log"}"#).expect("ok");
        assert_eq!(ok.output.as_deref(), Some("remote log"));
    }

    #[test]
    fn kill_peer_split_http_output_reads_marker() {
        let (status, body) = kill_split_peer_http_output("{\"ok\":true}\n__MAW_HTTP_STATUS__:200").expect("split");
        assert_eq!(status, 200);
        assert_eq!(body, "{\"ok\":true}");
    }

    #[test]
    fn kill_rejects_bad_flag_combinations_before_kill() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(
            &kill_strings(&["demo:main", "--all", "--pane", "0"]),
            &mut tmux,
        );
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("cannot combine --all and --pane"));
        assert_eq!(tmux.calls.len(), 1);
    }
