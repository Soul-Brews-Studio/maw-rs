    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use std::{net::Ipv4Addr, time::Duration};
    use tokio::sync::oneshot;
    use tower::ServiceExt;

    #[derive(Default)]
    struct FakeOrchestrator {
        calls: Mutex<Vec<ServecoreWorkonRequest>>,
    }

    impl ServecoreOrchestrator for FakeOrchestrator {
        fn spawn_workon(
            &self,
            request: ServecoreWorkonRequest,
            engine: Arc<dyn ServecoreEngine>,
        ) -> Result<ServecoreWorkonHandle, String> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            Ok(ServecoreWorkonHandle {
                ok: true,
                repo: request.repo,
                cwd: "/tmp/fake-worktree".to_owned(),
                engine: request
                    .engine
                    .unwrap_or_else(|| engine.servecore_engine_name().to_owned()),
                target: request.target,
                argv: vec!["workon".to_owned(), "demo".to_owned()],
                status: "fake-spawned".to_owned(),
                message: None,
                leader_argv: None,
                swarm_argv: None,
                pane: None,
                swarm_skipped: None,
            })
        }
    }

    #[derive(Default)]
    struct FakeExecRunner {
        calls: Mutex<Vec<(Vec<String>, PathBuf)>>,
    }

    impl ServecoreExecRunner for FakeExecRunner {
        fn servecore_run(&self, argv: &[String], cwd: &Path) -> Result<(), String> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((argv.to_vec(), cwd.to_path_buf()));
            Ok(())
        }
    }

    #[derive(Default)]
    struct FakePaneRunner {
        panes: Mutex<Vec<ServecorePaneCandidate>>,
        sends: Mutex<Vec<(String, String)>>,
        fail_send: Mutex<Option<String>>,
    }

    impl FakePaneRunner {
        fn with_panes(panes: Vec<ServecorePaneCandidate>) -> Self {
            Self {
                panes: Mutex::new(panes),
                sends: Mutex::new(Vec::new()),
                fail_send: Mutex::new(None),
            }
        }

        fn with_send_failure(panes: Vec<ServecorePaneCandidate>) -> Self {
            Self {
                panes: Mutex::new(panes),
                sends: Mutex::new(Vec::new()),
                fail_send: Mutex::new(Some("send failed".to_owned())),
            }
        }
    }

    impl ServecorePaneRunner for FakePaneRunner {
        fn servecore_list_panes(&self) -> Result<Vec<ServecorePaneCandidate>, String> {
            Ok(self
                .panes
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone())
        }

        fn servecore_send_literal_enter(&self, pane: &str, line: &str) -> Result<(), String> {
            if let Some(error) = self
                .fail_send
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
            {
                return Err(error);
            }
            self.sends
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((pane.to_owned(), line.to_owned()));
            Ok(())
        }
    }

    fn servecore_test_root(name: &str) -> PathBuf {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        root.push(format!(
            "maw-rs-core-orchestrator-{name}-{}-{nanos}",
            std::process::id()
        ));
        root
    }

    fn servecore_expected_public_leader() -> Vec<String> {
        [
            "wake",
            "nova",
            "--task",
            "feat-295",
            "--engine",
            "claude47",
            "--split",
            "--no-attach",
            "--repo",
            "acme/demo",
            "--prompt",
            "<redacted>",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    }

    fn servecore_expected_private_leader() -> Vec<String> {
        [
            "wake",
            "nova",
            "--task",
            "feat-295",
            "--engine",
            "claude47",
            "--split",
            "--no-attach",
            "--repo",
            "acme/demo",
            "--prompt",
            "SECRET prompt $(touch pwn)",
        ]
        .into_iter()
        .map(ToOwned::to_owned)
        .collect()
    }

    #[test]
    fn servecore_orchestrator_validates_engine_and_repo_bounds() {
        let root = servecore_test_root("bounds");
        let repo = root.join("github.com/acme/demo");
        std::fs::create_dir_all(&repo).expect("repo");
        let valid = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            task: Some("feat-219".to_owned()),
            engine: Some("codex-anything".to_owned()),
            target: Some("nova:1".to_owned()),
            prompt: Some("ship it".to_owned()),
            with_oracles: vec!["wish".to_owned()],
            attach: false,
            split: true,
            tiled: false,
        };
        let plan = servecore_prepare_workon(&root, valid, "stub").expect("plan");
        let ServecorePreparedOrchestration::Advanced(plan) = plan else {
            panic!("advanced plan");
        };
        assert_eq!(plan.engine, "codex-anything");
        assert_eq!(
            plan.leader_argv,
            vec![
                "wake",
                "nova:1",
                "--task",
                "feat-219",
                "--engine",
                "codex-anything",
                "--split",
                "--no-attach",
                "--repo",
                "acme/demo",
                "--prompt",
                "ship it",
            ]
        );
        assert_eq!(plan.repo_path, repo.canonicalize().expect("canon"));

        let bad_engine = ServecoreWorkonRequest {
            repo: "acme/demo".to_owned(),
            engine: Some("-shell".to_owned()),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, bad_engine, "stub").is_err());

        let escaped = ServecoreWorkonRequest {
            repo: "../demo".to_owned(),
            ..ServecoreWorkonRequest::default()
        };
        assert!(servecore_prepare_workon(&root, escaped, "stub").is_err());
    }

