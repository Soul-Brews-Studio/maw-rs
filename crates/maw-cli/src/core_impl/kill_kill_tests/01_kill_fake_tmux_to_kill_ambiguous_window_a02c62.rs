    use super::*;

    #[derive(Debug, Default)]
    struct KillFakeTmux {
        sessions_raw: String,
        panes_all_raw: String,
        pane_indexes_raw: String,
        calls: Vec<(String, Vec<String>)>,
        fail_kill: Option<String>,
    }

    impl KillTmux for KillFakeTmux {
        fn kill_list_sessions(&mut self) -> Result<Vec<KillSession>, String> {
            self.calls.push((
                "list-windows".to_owned(),
                kill_strings(&["-a", "-F", KILL_WINDOW_FORMAT]),
            ));
            Ok(kill_parse_sessions(&self.sessions_raw))
        }

        fn kill_list_panes_all(&mut self) -> Result<String, String> {
            self.calls.push((
                "list-panes".to_owned(),
                kill_strings(&["-a", "-F", maw_tmux::PANE_TARGET_FORMAT]),
            ));
            Ok(self.panes_all_raw.clone())
        }

        fn kill_list_pane_indexes(&mut self, target: &str) -> Result<Vec<u32>, String> {
            kill_validate_tmux_target(target)?;
            self.calls.push((
                "list-panes".to_owned(),
                kill_strings(&["-t", target, "-F", "#{pane_index}"]),
            ));
            Ok(kill_parse_numbers(&self.pane_indexes_raw))
        }

        fn kill_kill_session(&mut self, session: &str) -> Result<(), String> {
            kill_validate_tmux_target(session)?;
            self.calls
                .push(("kill-session".to_owned(), kill_strings(&["-t", session])));
            kill_maybe_fail(self.fail_kill.as_ref())
        }

        fn kill_kill_window(&mut self, target: &str) -> Result<(), String> {
            kill_validate_tmux_target(target)?;
            self.calls
                .push(("kill-window".to_owned(), kill_strings(&["-t", target])));
            kill_maybe_fail(self.fail_kill.as_ref())
        }

        fn kill_kill_pane(&mut self, target: &str) -> Result<(), String> {
            kill_validate_tmux_target(target)?;
            self.calls
                .push(("kill-pane".to_owned(), kill_strings(&["-t", target])));
            kill_maybe_fail(self.fail_kill.as_ref())
        }
    }

    #[derive(Debug, Default)]
    struct KillFakePeer {
        requests: Vec<KillPeerRequest>,
        response: Option<KillPeerResponse>,
        fail: Option<String>,
    }

    impl KillPeerTransport for KillFakePeer {
        fn kill_peer(&mut self, request: &KillPeerRequest) -> Result<KillPeerResponse, String> {
            kill_validate_peer_request(request)?;
            self.requests.push(request.clone());
            if let Some(message) = &self.fail {
                return Err(message.clone());
            }
            Ok(self.response.clone().unwrap_or(KillPeerResponse { output: None }))
        }
    }

    struct KillEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
        _lock: std::sync::MutexGuard<'static, ()>,
        dir: std::path::PathBuf,
    }

    impl KillEnvGuard {
        fn new(label: &str) -> Self {
            let lock = env_test_lock().lock().expect("kill env lock");
            let keys = ["PEERS_FILE", "MAW_SENDER", "MAW_PEER_KEY", "HOME", "MAW_HOME", "MAW_STATE_DIR", "XDG_STATE_HOME"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let dir = std::env::temp_dir().join(format!("maw-rs-kill-peer-{label}-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            for key in ["MAW_HOME", "MAW_STATE_DIR", "XDG_STATE_HOME"] { std::env::remove_var(key); }
            std::env::set_var("HOME", &dir);
            std::env::set_var("PEERS_FILE", dir.join("peers.json"));
            std::env::set_var("MAW_SENDER", "local:test-oracle");
            std::env::set_var("MAW_PEER_KEY", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef");
            Self { saved, _lock: lock, dir }
        }

        fn write_peers(&self, body: &str) {
            std::fs::write(self.dir.join("peers.json"), body).expect("write peers");
        }
    }

    impl Drop for KillEnvGuard {
        fn drop(&mut self) {
            for (key, value) in &self.saved {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn kill_run_fake(argv: &[String], tmux: &mut impl KillTmux) -> CliOutput {
        let mut peer = KillFakePeer::default();
        kill_run_command_with(
            argv,
            tmux,
            &mut peer,
            &HeyConfig { node: Some("local".to_owned()), oracle: Some("test-oracle".to_owned()), route: RouteConfig::default() },
            || Ok("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_owned()),
            || 1_700_000_000,
        )
    }

    fn kill_maybe_fail(error: Option<&String>) -> Result<(), String> {
        error.cloned().map_or(Ok(()), Err)
    }

    fn kill_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn kill_fake(sessions_raw: &str) -> KillFakeTmux {
        KillFakeTmux {
            sessions_raw: sessions_raw.to_owned(),
            ..KillFakeTmux::default()
        }
    }

    #[test]
    fn kill_dispatch_registers_native_kill() {
        assert_eq!(DISPATCH_78.len(), 1);
        assert_eq!(DISPATCH_78[0].command, "kill");
    }

    #[test]
    fn kill_session_resolves_and_validates_before_destructive_call() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["demo"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m killed session 07-demo\n");
        assert_eq!(tmux.calls[0].0, "list-windows");
        assert_eq!(
            tmux.calls[1],
            ("kill-session".to_owned(), kill_strings(&["-t", "07-demo"]))
        );
    }

    #[test]
    fn kill_rejects_leading_dash_target_before_listing_or_kill() {
        let mut tmux = kill_fake("07-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["-Sbad"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn kill_refuses_invalid_resolved_session_before_destructive_call() {
        let mut tmux = kill_fake("-Sbad-demo|||0|||main|||1|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["demo"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("target/session"));
        assert_eq!(
            tmux.calls.len(),
            1,
            "listed before refusing resolved kill target"
        );
    }

    #[test]
    fn kill_window_index_and_all_are_validated_against_listing() {
        let mut tmux = kill_fake("07-demo|||0|||work|||1|||/tmp\n07-demo|||2|||work|||0|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["07-demo:work", "--all"]), &mut tmux);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("killed 2 windows"));
        assert_eq!(
            tmux.calls[1],
            ("kill-window".to_owned(), kill_strings(&["-t", "07-demo:0"]))
        );
        assert_eq!(
            tmux.calls[2],
            ("kill-window".to_owned(), kill_strings(&["-t", "07-demo:2"]))
        );
    }

    #[test]
    fn kill_ambiguous_window_requires_index_or_all_and_does_not_kill() {
        let mut tmux = kill_fake("07-demo|||0|||work|||1|||/tmp\n07-demo|||2|||work|||0|||/tmp\n");
        let output = kill_run_fake(&kill_strings(&["07-demo:work"]), &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("ambiguous"));
        assert_eq!(tmux.calls.len(), 1);
    }

