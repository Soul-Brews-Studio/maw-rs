    #[test]
    fn servecore_simple_workon_executes_self_runner_and_matches_golden() {
        let root = servecore_test_root("simple-exec");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let orchestrator =
            ServecoreCommandOrchestrator::servecore_with_runner(root.clone(), runner.clone());
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("spawn");
        assert_eq!(handle.engine, "maw-rs");
        assert_eq!(handle.status, "spawned");
        assert_eq!(handle.message, None);
        assert_eq!(handle.leader_argv, None);
        assert_eq!(handle.swarm_argv, None);
        assert_eq!(handle.swarm_skipped, None);
        let calls = runner
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(
            calls[0].0,
            vec!["workon", "acme/demo", "feat-295", "--layout", "nested"]
        );
        assert_eq!(calls[0].1, repo.canonicalize().expect("canon"));
        let golden = serde_json::json!({"argv": handle.argv, "engine": handle.engine, "status": handle.status}).to_string();
        assert_eq!(
            format!("{golden}\n"),
            include_str!("../../../tests/fixtures/native-serve-engine/simple-workon.stdout")
        );
    }

    #[test]
    fn servecore_advanced_wake_swarm_executes_and_matches_golden() {
        let root = servecore_test_root("advanced-live");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::with_panes(vec![ServecorePaneCandidate {
            id: "%42".to_owned(),
            title: "nova feat-295 leader".to_owned(),
        }]));
        let orchestrator = ServecoreCommandOrchestrator::servecore_with_runners(
            root,
            runner.clone(),
            pane_runner.clone(),
        );
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    target: Some("nova".to_owned()),
                    prompt: Some("SECRET prompt $(touch pwn)".to_owned()),
                    with_oracles: vec!["wish".to_owned(), "codex".to_owned()],
                    split: true,
                    tiled: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("advanced");
        assert_eq!(handle.engine, "claude47");
        assert_eq!(handle.status, "spawned");
        assert_eq!(handle.pane.as_deref(), Some("%42"));
        let expected_public_leader = servecore_expected_public_leader();
        assert_eq!(handle.leader_argv, Some(expected_public_leader.clone()));
        assert_eq!(handle.argv, expected_public_leader);
        assert_eq!(
            handle.swarm_argv,
            Some(vec![
                "swarm".to_owned(),
                "wish".to_owned(),
                "codex".to_owned(),
                "--tiled".to_owned()
            ])
        );
        let handle_json = serde_json::to_string(&handle).expect("handle json");
        assert!(!handle_json.contains("SECRET"));
        assert!(!handle_json.contains("touch pwn"));
        assert!(!handle_json.contains("workon"));
        let calls = runner
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, servecore_expected_private_leader());
        let sends = pane_runner
            .sends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let expected_line = format!(
            "{} 'swarm' 'wish' 'codex' '--tiled'",
            servecore_shell_quote(
                &engine::serveengine_self_bin()
                    .expect("self")
                    .to_string_lossy()
            )
        );
        assert_eq!(sends.as_slice(), [("%42".to_owned(), expected_line)]);
        let golden = serde_json::json!({
            "argv": handle.argv,
            "engine": handle.engine,
            "leader_argv": handle.leader_argv,
            "pane": handle.pane,
            "status": handle.status,
            "swarm_argv": handle.swarm_argv,
        })
        .to_string();
        assert_eq!(
            format!("{golden}\n"),
            include_str!("../../../tests/fixtures/native-serve-engine/advanced-wake-swarm.stdout")
        );
    }

