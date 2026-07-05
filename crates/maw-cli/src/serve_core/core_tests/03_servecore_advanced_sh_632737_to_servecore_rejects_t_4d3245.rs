    #[test]
    fn servecore_advanced_shell_quote_and_metachar_guards_block_injection() {
        assert_eq!(servecore_shell_quote("builder'one"), "'builder'\\''one'");
        assert_eq!(servecore_shell_quote("$(touch pwn)"), "'$(touch pwn)'");
        assert_eq!(servecore_shell_quote("`touch pwn`;"), "'`touch pwn`;'");

        let root = servecore_test_root("advanced-quote");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::with_panes(vec![ServecorePaneCandidate {
            id: "%7".to_owned(),
            title: "feat-295".to_owned(),
        }]));
        let orchestrator = ServecoreCommandOrchestrator::servecore_with_runners(
            root.clone(),
            runner.clone(),
            pane_runner.clone(),
        );
        orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    target: Some("nova".to_owned()),
                    prompt: Some("data $(touch pwn) `whoami`;".to_owned()),
                    with_oracles: vec!["wish".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("spawn");
        let sends = pane_runner
            .sends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let expected_line = format!(
            "{} 'swarm' 'wish'",
            servecore_shell_quote(
                &engine::serveengine_self_bin()
                    .expect("self")
                    .to_string_lossy()
            )
        );
        assert_eq!(
            sends[0].1, expected_line,
            "send-keys receives one quoted literal line, not shell-expanded fragments"
        );

        for (label, mut request) in [
            (
                "target",
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    target: Some("bad;name".to_owned()),
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
            ),
            (
                "with",
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["$(touch-pwn)".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
            ),
            (
                "with-quote",
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["bad'name".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
            ),
        ] {
            request.engine = Some("claude47".to_owned());
            assert!(
                servecore_prepare_workon(&root, request, "maw-rs").is_err(),
                "{label} metachar must reject before runner"
            );
        }
        assert_eq!(
            runner
                .calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .len(),
            1,
            "bad metachar requests never reach child runner"
        );
    }

    #[test]
    fn servecore_advanced_pane_discovery_fail_is_soft_loud() {
        let root = servecore_test_root("advanced-no-pane");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::default());
        let orchestrator =
            ServecoreCommandOrchestrator::servecore_with_runners(root, runner, pane_runner.clone());
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["wish".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("soft");
        assert_eq!(handle.status, "leader-spawned");
        assert_eq!(
            handle.swarm_skipped.as_deref(),
            Some("pane discovery failed")
        );
        assert!(pane_runner
            .sends
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_empty());
    }

    #[test]
    fn servecore_advanced_pane_send_fail_is_soft_loud() {
        let root = servecore_test_root("advanced-send-fail");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let runner = Arc::new(FakeExecRunner::default());
        let pane_runner = Arc::new(FakePaneRunner::with_send_failure(vec![
            ServecorePaneCandidate {
                id: "%9".to_owned(),
                title: "feat-295".to_owned(),
            },
        ]));
        let orchestrator =
            ServecoreCommandOrchestrator::servecore_with_runners(root, runner, pane_runner);
        let handle = orchestrator
            .spawn_workon(
                ServecoreWorkonRequest {
                    repo: "acme/demo".to_owned(),
                    task: Some("feat-295".to_owned()),
                    with_oracles: vec!["wish".to_owned()],
                    split: true,
                    ..ServecoreWorkonRequest::default()
                },
                Arc::new(ServecoreNativeEngine),
            )
            .expect("soft");
        assert_eq!(handle.status, "leader-spawned");
        assert_eq!(handle.swarm_skipped.as_deref(), Some("pane send failed"));
    }

    #[test]
    fn servecore_advanced_refuses_attach_and_requires_task() {
        let root = servecore_test_root("advanced-guards");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let attach = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            task: Some("feat-295".to_owned()),
            attach: true,
            split: true,
            ..ServecoreWorkonRequest::default()
        };
        let Err(attach_err) = servecore_prepare_workon(&root, attach, "maw-rs") else {
            panic!("attach must fail");
        };
        assert!(attach_err.contains("attach is not supported"));

        let no_task = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            split: true,
            ..ServecoreWorkonRequest::default()
        };
        let Err(task_err) = servecore_prepare_workon(&root, no_task, "maw-rs") else {
            panic!("task must fail");
        };
        assert!(task_err.contains("advanced wake requires task"));
    }

    #[test]
    fn servecore_rejects_task_engine_and_repo_guards() {
        let root = servecore_test_root("guards");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let bad_task = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            task: Some("-bad".to_owned()),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_task, "maw-rs").is_err());
        let bad_engine = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            engine: Some("bad\nengine".to_owned()),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_engine, "maw-rs").is_err());
        let bad_repo = ServecoreWorkonRequest {
            repo: "../demo".to_owned(),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_repo, "maw-rs").is_err());
    }

