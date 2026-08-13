// The send/ACL suite, kept whole.
//
// Shares one hermetic environment builder plus signed-request and sink fixtures;
// splitting by topic would mean exporting that harness across sibling modules,
// which a private item in a `mod` cannot cross. Moving it out of
// send_federation.rs was the point.

#[cfg(test)]
mod send_acl_hotpath_tests {
    use super::*;

    #[test]
    fn send_empty_body_output_refuses_empty_and_whitespace_text() {
        let hey = send_empty_body_output("hey", "").expect("empty");
        assert_eq!(hey.code, 1);
        assert!(hey.stdout.is_empty());
        assert_eq!(hey.stderr, "hey: refusing to deliver an empty message body\n");

        let whitespace = send_empty_body_output("hey", "   \n\t").expect("whitespace-only");
        assert_eq!(whitespace.stderr, hey.stderr);

        let notify = send_empty_body_output("notify", "").expect("empty");
        assert_eq!(notify.code, 2);
        assert_eq!(notify.stderr, "notify: refusing to deliver an empty message body\n");
    }

    #[test]
    fn send_empty_body_output_allows_real_text() {
        assert!(send_empty_body_output("hey", "hello fleet").is_none());
        assert!(send_empty_body_output("hey", "  padded  ").is_none());
    }

    #[test]
    fn send_self_node_refusal_output_explains_the_cross_node_loopback() {
        let output = send_self_node_refusal_output("hey", "black:33-maw-rs", "33-maw-rs:0");
        assert_eq!(output.code, 1);
        assert!(output.stdout.is_empty());
        assert!(output.stderr.contains("refusing to deliver"));
        assert!(output.stderr.contains("black:33-maw-rs"));
        assert!(output.stderr.contains("33-maw-rs:0"));
    }

    #[test]
    fn send_local_inbox_only_writes_to_the_receiver_inbox_not_the_pane() {
        // #672 defect 2: `hey --inbox local:<agent>` must write the RECEIVER's
        // ψ/inbox, never inject into the pane. send_local_message_with_audit
        // (the pane-injection path) is never called here -- reverting the
        // wiring in run_send_like_async_with_args back to always calling it
        // leaves the receiver inbox empty -> RED.
        let receiver = std::env::temp_dir().join(format!("maw-rs-hey-inbox-recv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&receiver);
        std::fs::create_dir_all(&receiver).expect("receiver repo");
        let receiver_str = receiver.to_string_lossy().into_owned();
        let resolve = |oracle: &str| {
            assert_eq!(oracle, "arra-oracle-v3", "resolver is asked for the receiver, not the sender");
            Some(receiver_str.clone())
        };

        let out = send_local_inbox_only_with(
            "hey",
            "local:arra-oracle-v3",
            "41-arra-oracle-v3:1",
            "inbox-probe",
            &HeyConfig::default(),
            "ui",
            None,
            &resolve,
        );
        assert_eq!(out.code, 0, "stderr={}", out.stderr);
        assert!(out.stdout.starts_with("queued inbox arra-oracle-v3 "), "{}", out.stdout);

        let inbox = receiver.join("ψ").join("inbox");
        let files: Vec<_> = std::fs::read_dir(&inbox).expect("receiver inbox exists").filter_map(Result::ok).collect();
        assert_eq!(files.len(), 1, "exactly one message filed in the RECEIVER inbox");
        let body = std::fs::read_to_string(files[0].path()).expect("read msg");
        assert!(body.contains("to: arra-oracle-v3"), "{body}");
        assert!(body.contains("inbox-probe"), "{body}");
        std::fs::remove_dir_all(&receiver).ok();
    }

    #[test]
    fn send_local_inbox_only_errors_clearly_when_receiver_unknown() {
        let out = send_local_inbox_only_with(
            "hey",
            "local:ghost-oracle",
            "ghost:0",
            "hi",
            &HeyConfig::default(),
            "ui",
            None,
            &|_oracle: &str| None,
        );
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert!(out.stderr.contains("cannot resolve a local inbox for 'ghost-oracle'"), "{}", out.stderr);
    }

    // #695 regression, pinned at the wiring level (not just the leaf
    // formatters above): send_route_gate is the exact function
    // run_send_like_async_with_args consults, called with real RouteResult
    // variants — a wiring mistake (e.g. forgetting to check SelfNode, or
    // checking the wrong field) fails these, not just a helper in isolation.
    #[test]
    fn send_route_gate_refuses_self_node_regardless_of_text() {
        let result = RouteResult::SelfNode { target: "33-maw-rs:0".to_owned() };
        let refusal = send_route_gate("hey", "black:33-maw-rs", "hello", &result)
            .expect("SelfNode must be refused");
        assert!(refusal.stderr.contains("black:33-maw-rs"));
        assert!(refusal.stderr.contains("33-maw-rs:0"));
    }

    // Regression: maw-routing resolves the documented `local:<agent>` form
    // through the SAME SelfNode variant as a real cross-node alias that
    // happens to equal this node's own identity (see
    // send_query_uses_explicit_local_prefix's doc comment) -- v1 of this
    // gate refused BOTH, which broke `local:` targeting fleet-wide (caught
    // live: `hey local:maw-rs` started erroring right after #703 shipped).
    // `local:` must always be allowed through; only a real alias resolving
    // to self is the #695 bug.
    #[test]
    fn send_route_gate_allows_self_node_reached_via_explicit_local_prefix() {
        let result = RouteResult::SelfNode { target: "33-maw-rs:0".to_owned() };
        assert!(
            send_route_gate("hey", "local:maw-rs", "hello", &result).is_none(),
            "local: is documented, intentional same-node routing, not the loopback-self bug"
        );
    }

    #[test]
    fn send_query_uses_explicit_local_prefix_matches_only_the_local_node_name() {
        assert!(send_query_uses_explicit_local_prefix("local:maw-rs"));
        assert!(send_query_uses_explicit_local_prefix("local:"));
        assert!(!send_query_uses_explicit_local_prefix("black:33-maw-rs"));
        assert!(!send_query_uses_explicit_local_prefix("blackmachine:33-maw-rs"));
        assert!(!send_query_uses_explicit_local_prefix("maw-rs"));
        assert!(!send_query_uses_explicit_local_prefix(""));
    }

    #[test]
    fn send_route_gate_refuses_empty_text_on_every_route_shape() {
        let local = RouteResult::Local { target: "s:0".to_owned() };
        assert!(send_route_gate("hey", "s", "", &local).is_some());

        let peer = RouteResult::Peer {
            peer_url: "http://peer".to_owned(),
            target: "s:0".to_owned(),
            node: "m5".to_owned(),
        };
        assert!(send_route_gate("hey", "m5:s", "   ", &peer).is_some());

        let error = RouteResult::Error {
            reason: "not_found".to_owned(),
            detail: "nope".to_owned(),
            hint: None,
        };
        assert!(send_route_gate("hey", "s", "", &error).is_some());
    }

    #[test]
    fn send_route_gate_lets_local_and_peer_proceed_with_real_text() {
        let local = RouteResult::Local { target: "s:0".to_owned() };
        assert!(send_route_gate("hey", "s", "hello fleet", &local).is_none());

        let peer = RouteResult::Peer {
            peer_url: "http://peer".to_owned(),
            target: "s:0".to_owned(),
            node: "m5".to_owned(),
        };
        assert!(send_route_gate("hey", "m5:s", "hello fleet", &peer).is_none());

        let error = RouteResult::Error {
            reason: "not_found".to_owned(),
            detail: "nope".to_owned(),
            hint: None,
        };
        assert!(
            send_route_gate("hey", "s", "hello fleet", &error).is_none(),
            "Error is not a delivery decision the gate should intercept — the existing Error arm handles it"
        );
    }

    #[derive(Debug, Default)]
    struct SendFakeTmuxRunner {
        current_session: Option<Result<String, String>>,
        caller_window: Option<Result<String, String>>,
        focused_window: Option<Result<String, String>>,
        calls: Vec<(String, Vec<String>)>,
    }

    impl maw_tmux::TmuxRunner for SendFakeTmuxRunner {
        fn run(
            &mut self,
            subcommand: &str,
            args: &[String],
        ) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" if args.last().is_some_and(|arg| arg == "#{window_name}") => args
                    .windows(2)
                    .find(|pair| pair[0] == "-t")
                    .map_or(&self.focused_window, |_| &self.caller_window)
                    .clone()
                    .unwrap_or_else(|| Ok(String::new()))
                    .map_err(maw_tmux::TmuxError::new),
                "display-message" => self
                    .current_session
                    .clone()
                    .unwrap_or_else(|| Ok(String::new()))
                    .map_err(maw_tmux::TmuxError::new),
                other => Err(maw_tmux::TmuxError::new(format!(
                    "unexpected tmux command {other}"
                ))),
            }
        }
    }

    struct SendAclEnvGuard {
        _home: EnvVarRestore,
        _maw_home: EnvVarRestore,
        _config: EnvVarRestore,
        _state: EnvVarRestore,
        _bypass: EnvVarRestore,
        root: std::path::PathBuf,
    }

    impl SendAclEnvGuard {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos());
            let root = std::env::temp_dir().join(format!("maw-send-acl-{name}-{}-{nanos}", std::process::id()));
            let _ = std::fs::create_dir_all(root.join("home"));
            let _ = std::fs::create_dir_all(root.join("config"));
            let _ = std::fs::create_dir_all(root.join("state"));
            let guard = Self {
                _home: EnvVarRestore::capture("HOME"),
                _maw_home: EnvVarRestore::capture("MAW_HOME"),
                _config: EnvVarRestore::capture("MAW_CONFIG_DIR"),
                _state: EnvVarRestore::capture("MAW_STATE_DIR"),
                _bypass: EnvVarRestore::capture("MAW_ACL_BYPASS"),
                root: root.clone(),
            };
            std::env::set_var("HOME", root.join("home"));
            std::env::remove_var("MAW_HOME");
            std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
            std::env::set_var("MAW_STATE_DIR", root.join("state"));
            std::env::remove_var("MAW_ACL_BYPASS");
            guard
        }
    }

    fn send_acl_config(oracle: &str) -> HeyConfig {
        HeyConfig { node: Some("node-a".to_owned()), oracle: Some(oracle.to_owned()), route: RouteConfig::default() }
    }

    fn send_audit_test_env(name: &str) -> (std::path::PathBuf, [EnvVarRestore; 11]) {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!("maw-send-audit-{name}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(root.join("maw/config")).expect("config");
        // MAW_SESSION_WINDOW is captured/removed here (not left to each test)
        // because resolve_hey_canonical_sender_oracle reads it before falling
        // back to config.oracle -- on any machine with an active maw oracle
        // session (every real dev box), it silently overrides the sender
        // identity a test just constructed (#700). Individual tests that
        // deliberately exercise pane-derived sender resolution still set
        // their own value after this returns; that layering is safe (capture
        // + restore nests correctly).
        let restores = ["HOME", "MAW_HOME", "MAW_CONFIG_DIR", "MAW_DATA_DIR", "MAW_STATE_DIR", "USER", "LOGNAME", "HOSTNAME", "MAW_AUDIT_TEST_NOW_MS", "MAW_MESSAGE_LEDGER_DISABLE", "MAW_SESSION_WINDOW"].map(EnvVarRestore::capture);
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_HOME", root.join("maw"));
        for key in ["MAW_CONFIG_DIR", "MAW_DATA_DIR", "MAW_STATE_DIR", "LOGNAME", "MAW_MESSAGE_LEDGER_DISABLE", "MAW_SESSION_WINDOW"] { std::env::remove_var(key); }
        std::env::set_var("USER", "nat");
        std::env::set_var("HOSTNAME", "m5");
        std::env::set_var("MAW_AUDIT_TEST_NOW_MS", "1783565423347");
        (root, restores)
    }

    struct SendCwdRestore(std::path::PathBuf);

    impl SendCwdRestore {
        fn enter(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().expect("current dir");
            std::env::set_current_dir(path).expect("set current dir");
            Self(previous)
        }
    }

    impl Drop for SendCwdRestore {
        fn drop(&mut self) { std::env::set_current_dir(&self.0).expect("restore current dir"); }
    }

    fn assert_message_sink_from(root: &std::path::Path, expected: &str) {
        let log: serde_json::Value = serde_json::from_str(std::fs::read_to_string(root.join("maw/maw-log.jsonl")).unwrap().trim()).unwrap();
        assert_eq!(log["from"], expected);
        let output = std::process::Command::new("sqlite3")
            .arg(root.join("maw/message-ledger.sqlite"))
            .arg("select from_id from messages;")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap().trim(), expected);
    }

    fn send_acl_args(target: &str, text: &str) -> SendArgs {
        SendArgs { target: target.to_owned(), text: text.to_owned(), inbox: None, from: None, approve: false, trust: false, dry_run: false }
    }

    fn send_route_window(index: u32, name: &str) -> RouteWindow {
        RouteWindow {
            index,
            name: name.to_owned(),
            active: index == 0,
            kind: None,
        }
    }

    fn send_route_session(name: &str, windows: Vec<RouteWindow>) -> RouteSession {
        RouteSession {
            name: name.to_owned(),
            windows,
            source: None,
        }
    }

    #[test]
    fn send_self_alias_uses_current_tmux_session_from_runner() {
        let sessions = vec![send_route_session(
            "188-maw-rs",
            vec![
                send_route_window(0, "work"),
                send_route_window(1, "maw-rs-oracle"),
            ],
        )];
        let mut runner = SendFakeTmuxRunner {
            current_session: Some(Ok("188-maw-rs\n".to_owned())),
            ..SendFakeTmuxRunner::default()
        };

        assert_eq!(
            resolve_send_route_target(
                "me",
                &RouteConfig::default(),
                &sessions,
                true,
                &mut runner
            ),
            RouteResult::Local {
                target: "188-maw-rs:1".to_owned()
            }
        );
        assert_eq!(
            runner.calls,
            vec![(
                "display-message".to_owned(),
                vec!["-p".to_owned(), "#{session_name}".to_owned()]
            )]
        );
    }

    #[test]
    fn send_self_alias_outside_tmux_does_not_match_literal_me_window() {
        let sessions = vec![send_route_session(
            "scratch",
            vec![send_route_window(0, "me"), send_route_window(1, "shell")],
        )];
        let mut runner = SendFakeTmuxRunner::default();

        let result = resolve_send_route_target(
            "me",
            &RouteConfig::default(),
            &sessions,
            false,
            &mut runner,
        );

        assert!(matches!(
            result,
            RouteResult::Error { reason, .. } if reason == "me_needs_tmux"
        ));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn send_dry_run_parser_and_output_include_resolved_target() {
        let args = parse_send_args(
            "hey",
            &send_acl_vec(&["me", "--dry-run", "test"]),
        )
        .expect("parse");
        assert!(args.dry_run);
        assert_eq!(args.target, "me");
        assert_eq!(args.text, "test");

        let output = send_dry_run_output(
            "hey",
            &args,
            &RouteResult::Local {
                target: "188-maw-rs:1".to_owned(),
            },
        );
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "dry-run: hey me -> local 188-maw-rs:1\n");
    }

    // #686: hey's success line named the target string as typed, so a
    // collision across nodes ("33-maw-rs" resolvable on m5 AND on two
    // different peers) could not be told apart from the confirmation alone —
    // it took a receiver-side `lastActivity` comparison to prove which node
    // actually received it. The node must be visible in the line itself.
    #[test]
    fn peer_success_line_names_the_node_it_resolved_to() {
        assert_eq!(send_peer_success_target("blackmachine", "33-maw-rs:0"), "blackmachine:33-maw-rs:0");
        // Callers that never track a node (notify/forward/talk_to today)
        // keep the bare target unchanged rather than gaining a bogus prefix.
        assert_eq!(send_peer_success_target("", "33-maw-rs:0"), "33-maw-rs:0");
    }

    #[test]
    fn hey_typed_inventory_routes_exact_and_asks_on_fuzzy() {
        let sessions = vec![send_route_session(
            "41-atlas",
            vec![send_route_window(1, "atlas-oracle")],
        )];
        let config = RouteConfig::default();

        assert_eq!(hey_picker_target("atlas-oracle", &config, &sessions).expect("exact"), "41-atlas:1");
        match typed_picker_plan("atla", &hey_typed_candidates(&config, &sessions), hey_kind_priority, hey_picker_row) {
            TypedPickerPlan::Pick { rows, .. } => {
                assert_eq!(rows.len(), 1);
                assert_eq!(rows[0].matched.candidate.name, "41-atlas:1");
                assert_eq!(rows[0].action, "maw hey 41-atlas:1 <message>");
            }
            plan @ TypedPickerPlan::Target(_) => panic!("expected fuzzy picker, got {plan:?}"),
        }
    }

    #[test]
    fn hey_help_prints_usage_to_stdout_zero() {
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(run_send_like_async_impl("hey", &send_acl_vec(&["--help"])));

        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("usage: maw hey <target> <message>"));
        assert!(output.stderr.is_empty());
        assert!(!wants_help_before_positionals(&send_acl_vec(&["bob", "hello", "--help"]), &["--from"]));
    }

    fn send_acl_write_scope(name: &str, members: &[&str]) {
        let dir = scope_native_dir();
        std::fs::create_dir_all(&dir).unwrap();
        let scope = ScopeNativeRecord { name: name.to_owned(), members: members.iter().map(|member| (*member).to_owned()).collect(), lead: None, created: "2026-06-26T00:00:00.000Z".to_owned(), ttl: None };
        std::fs::write(dir.join(format!("{name}.json")), serde_json::to_string_pretty(&scope).unwrap()).unwrap();
    }

    fn send_acl_assert_proceed(result: SendAclGateResult) -> String {
        match result {
            SendAclGateResult::Proceed { stderr_prefix } => stderr_prefix,
            other => panic!("expected proceed, got {other:?}"),
        }
    }

    #[test]
    fn send_identity_uses_invocation_oracle_for_wire_and_local_tags() {
        let _lock = env_test_lock();
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        std::env::remove_var("MAW_SENDER");
        let config = HeyConfig {
            node: Some("m5".to_owned()),
            oracle: Some("configured".to_owned()),
            route: RouteConfig::default(),
        };

        assert_eq!(
            resolve_hey_wire_from(None, &config, "maw-rs").expect("wire from"),
            "maw-rs:m5"
        );
        assert_eq!(
            format_local_hey_message("hello", &config, "maw-rs", None),
            "[m5:maw-rs] hello"
        );
        assert_eq!(
            format_local_hey_message("[pretagged] hello", &config, "maw-rs", None),
            "[m5:maw-rs] [pretagged] hello"
        );
    }

    #[test]
    fn send_identity_targets_callers_non_active_tmux_pane_and_marks_focused_fallback() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("sender-pane");
        let _pane = EnvVarRestore::capture("TMUX_PANE");
        let _session = EnvVarRestore::capture("MAW_SESSION_WINDOW");
        std::env::set_var("TMUX_PANE", "%42");
        std::env::remove_var("MAW_SESSION_WINDOW");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let _cwd = SendCwdRestore::enter(&repo);
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: None, route: RouteConfig::default() };
        let mut runner = SendFakeTmuxRunner {
            caller_window: Some(Ok("agora\n".to_owned())),
            focused_window: Some(Ok("nh\n".to_owned())),
            ..SendFakeTmuxRunner::default()
        };

        let sender = resolve_hey_sender_oracle_with(&config, std::env::var("TMUX_PANE").ok().as_deref(), true, &mut runner);

        assert_eq!(sender, "agora");
        assert_eq!(format_local_hey_message("hello", &config, &sender, None), "[m5:agora] hello");
        assert_eq!(resolve_hey_wire_from(None, &config, &sender).as_deref(), Ok("agora:m5"));
        assert_eq!(send_normalized_from(&config, &sender, None).as_deref(), Some("m5:agora"));
        assert_eq!(runner.calls, vec![("display-message".to_owned(), send_acl_vec(&["-t", "%42", "-p", "#{window_name}"]))]);

        let mut fallback = SendFakeTmuxRunner {
            focused_window: Some(Ok("nh\n".to_owned())),
            ..SendFakeTmuxRunner::default()
        };
        let sender = resolve_hey_sender_oracle_with(&config, None, true, &mut fallback);
        assert_eq!(sender, "pane/nh");
        assert_eq!(send_normalized_from(&config, &sender, None).as_deref(), Some("m5:pane/nh"));
        assert_eq!(fallback.calls, vec![("display-message".to_owned(), send_acl_vec(&["-p", "#{window_name}"]))]);
    }

    #[test]
    fn send_identity_headless_never_queries_focused_window() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("sender-headless");
        let _pane = EnvVarRestore::capture("TMUX_PANE");
        let _session = EnvVarRestore::capture("MAW_SESSION_WINDOW");
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        std::env::remove_var("TMUX_PANE");
        std::env::remove_var("MAW_SESSION_WINDOW");
        std::env::remove_var("MAW_SENDER");
        let plain = root.join("plain");
        std::fs::create_dir_all(&plain).unwrap();
        let _cwd = SendCwdRestore::enter(&plain);
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: None, route: RouteConfig::default() };
        let mut runner = SendFakeTmuxRunner {
            focused_window: Some(Ok("ai-party\n".to_owned())),
            ..SendFakeTmuxRunner::default()
        };

        let sender = resolve_hey_sender_oracle_with(&config, None, false, &mut runner);

        assert_eq!(sender, "pane/unknown");
        assert!(runner.calls.is_empty(), "headless sender must never query tmux: {:?}", runner.calls);
        assert_eq!(send_normalized_from(&config, &sender, None).as_deref(), Some("m5:pane/unknown"));
    }

    #[test]
    fn send_identity_headless_signs_job_repo_stem() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("sender-headless-repo");
        let _pane = EnvVarRestore::capture("TMUX_PANE");
        let _session = EnvVarRestore::capture("MAW_SESSION_WINDOW");
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        std::env::remove_var("TMUX_PANE");
        std::env::remove_var("MAW_SESSION_WINDOW");
        std::env::remove_var("MAW_SENDER");
        let repo = root.join("maw-rs");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let nested = repo.join("crates/maw-cli");
        std::fs::create_dir_all(&nested).unwrap();
        let _cwd = SendCwdRestore::enter(&nested);
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: None, route: RouteConfig::default() };
        let mut runner = SendFakeTmuxRunner {
            focused_window: Some(Ok("ai-party\n".to_owned())),
            ..SendFakeTmuxRunner::default()
        };

        let sender = resolve_hey_sender_oracle_with(&config, None, false, &mut runner);

        assert_eq!(sender, "job/maw-rs");
        assert!(runner.calls.is_empty(), "headless sender must never query tmux: {:?}", runner.calls);
        assert_eq!(format_local_hey_message("hello", &config, &sender, None), "[m5:job/maw-rs] hello");
        assert_eq!(send_normalized_from(&config, &sender, None).as_deref(), Some("m5:job/maw-rs"));
    }

    #[test]
    fn send_local_display_normalizes_explicit_wire_from() {
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: None, route: RouteConfig::default() };

        assert_eq!(send_display_from(Some("atlas:m5")).as_deref(), Some("m5:atlas"));
        assert_eq!(
            format_local_hey_message("hello", &config, "atlas", send_display_from(Some("atlas:m5")).as_deref()),
            "[m5:atlas] hello"
        );
        assert_eq!(send_display_from(Some("not-wire-shaped")).as_deref(), Some("not-wire-shaped"));
        assert_eq!(send_display_from(None), None);
    }

    #[test]
    fn send_success_writes_sane_audit_records() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("schema");
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        let args = send_audit_args("hey", &send_acl_vec(&["agent", "hello"]));

        send_record_success("hey", &args, &config, "atlas", None, "agent", "[m5:atlas] hello", "local", None);

        let audit: serde_json::Value = serde_json::from_str(std::fs::read_to_string(root.join("maw/audit.jsonl")).unwrap().trim()).unwrap();
        assert_eq!(audit["cmd"], "hey");
        assert_eq!(audit["args"], serde_json::json!(["hey", "agent", "hello"]));
        assert_eq!(audit["user"], "nat");
        assert!(audit["pid"].as_u64().is_some());

        let log: serde_json::Value = serde_json::from_str(std::fs::read_to_string(root.join("maw/maw-log.jsonl")).unwrap().trim()).unwrap();
        assert_eq!(log["from"], "m5:atlas");
        assert_eq!(log["to"], "agent");
        assert_eq!(log["msg"], "[m5:atlas] hello");
        assert_eq!(log["host"], "m5");
        assert_eq!(log["route"], "local");
    }

    #[test]
    fn message_sinks_normalize_explicit_wire_from_to_host_handle() {
        let _lock = env_test_lock();
        if std::process::Command::new("sqlite3").arg("-version").output().is_err() { return; }
        let (root, _restores) = send_audit_test_env("identity-order");
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: None, route: RouteConfig::default() };
        let args = send_audit_args("hey", &send_acl_vec(&["agent", "hello", "--from", "atlas:m5"]));

        assert_eq!(resolve_hey_sender_oracle_for_from(&config, Some("atlas:m5")), "atlas");
        assert_eq!(resolve_hey_wire_from(Some("atlas:m5"), &config, "atlas").unwrap(), "atlas:m5");
        assert!(send_message_signature(&config, "atlas", Some("atlas:m5"), "hello").is_ok());

        send_record_success("hey", &args, &config, "atlas", Some("atlas:m5"), "agent", "[atlas:m5] hello", "local", None);

        assert_message_sink_from(&root, "m5:atlas");
    }

    #[test]
    fn message_sinks_prefer_claude_handle_spelling_over_pane_label() {
        let _lock = env_test_lock();
        if std::process::Command::new("sqlite3").arg("-version").output().is_err() { return; }
        let (root, _restores) = send_audit_test_env("identity-spelling");
        let _pane = EnvVarRestore::capture("TMUX_PANE");
        let _session = EnvVarRestore::capture("MAW_SESSION_WINDOW");
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        std::env::remove_var("TMUX_PANE");
        std::env::remove_var("MAW_SENDER");
        std::env::set_var("MAW_SESSION_WINDOW", "41-arra:arraoraclev3");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::write(repo.join("CLAUDE.md"), "# arra-oracle-v3-oracle\n").unwrap();
        let _cwd = SendCwdRestore::enter(&repo);
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("configured".to_owned()), route: RouteConfig::default() };
        let sender = resolve_hey_sender_oracle(&config);

        send_record_success("hey", &send_audit_args("hey", &send_acl_vec(&["agent", "hello"])), &config, &sender, None, "agent", "hello", "local", None);

        assert_eq!(sender, "arra-oracle-v3");
        assert_message_sink_from(&root, "m5:arra-oracle-v3");
    }

    #[test]
    fn message_sinks_mark_unresolved_pane_fallback() {
        let _lock = env_test_lock();
        if std::process::Command::new("sqlite3").arg("-version").output().is_err() { return; }
        let (root, _restores) = send_audit_test_env("identity-pane-fallback");
        let _session = EnvVarRestore::capture("MAW_SESSION_WINDOW");
        let _sender = EnvVarRestore::capture("MAW_SENDER");
        std::env::remove_var("MAW_SESSION_WINDOW");
        std::env::remove_var("MAW_SENDER");
        let repo = root.join("repo");
        std::fs::create_dir_all(&repo).unwrap();
        let _cwd = SendCwdRestore::enter(&repo);
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: None, route: RouteConfig::default() };

        send_record_success("hey", &send_audit_args("hey", &send_acl_vec(&["agent", "hello"])), &config, "pane/window-arranger", None, "agent", "hello", "local", None);

        assert_message_sink_from(&root, "m5:pane/window-arranger");
    }

    #[test]
    fn sink_registry_preserves_audit_and_maw_log_bytes() {
        let _lock = env_test_lock();
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        let args = send_audit_args("hey", &send_acl_vec(&["agent", "hello"]));

        let (actual_root, _actual_restores) = send_audit_test_env("sink-actual");
        std::env::set_var("MAW_MESSAGE_LEDGER_DISABLE", "1");
        send_record_success("hey", &args, &config, "atlas", None, "agent", "[m5:atlas] hello", "local", None);
        let actual_audit = std::fs::read(actual_root.join("maw/audit.jsonl")).unwrap();
        let actual_log = std::fs::read(actual_root.join("maw/maw-log.jsonl")).unwrap();

        let (expected_root, _expected_restores) = send_audit_test_env("sink-expected");
        send_write_js_audit_record("hey", &args);
        send_write_js_maw_log_record("m5:atlas", "agent", "[m5:atlas] hello", "local");
        assert_eq!(actual_audit, std::fs::read(expected_root.join("maw/audit.jsonl")).unwrap());
        assert_eq!(actual_log, std::fs::read(expected_root.join("maw/maw-log.jsonl")).unwrap());
    }

    #[test]
    fn message_ledger_sink_writes_signed_column_default() {
        let _lock = env_test_lock();
        if std::process::Command::new("sqlite3").arg("-version").output().is_err() { return; }
        let (root, _restores) = send_audit_test_env("ledger");
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        let args = send_audit_args("hey", &send_acl_vec(&["agent", "hello"]));

        send_record_success("hey", &args, &config, "atlas", None, "agent", "[m5:atlas] hello", "local", None);

        let output = std::process::Command::new("sqlite3")
            .arg(root.join("maw/message-ledger.sqlite"))
            .arg("select from_id || '|' || to_id || '|' || text || '|' || route || '|' || signed from messages;")
            .output()
            .unwrap();
        assert!(output.status.success());
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "m5:atlas|agent|[m5:atlas] hello|local|0\n");
    }

    #[test]
    fn message_ledger_sink_marks_signed_records() {
        let _lock = env_test_lock();
        if std::process::Command::new("sqlite3").arg("-version").output().is_err() { return; }
        let (root, _restores) = send_audit_test_env("ledger-signed");
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        let args = send_audit_args("hey", &send_acl_vec(&["agent", "hello"]));
        send_record_success("hey", &args, &config, "atlas", None, "agent", "[m5:atlas] hello", "local", Some(&MessageSignature));
        let output = std::process::Command::new("sqlite3")
            .arg(root.join("maw/message-ledger.sqlite"))
            .arg("select signed from messages;")
            .output()
            .unwrap();
        assert_eq!(String::from_utf8(output.stdout).unwrap(), "1\n");
    }

    #[test]
    fn send_message_signature_rejects_forged_from_and_prefix_bypass() {
        let _lock = env_test_lock();
        let (_root, _restores) = send_audit_test_env("signature-forge");
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        assert!(send_message_signature(&config, "atlas", None, "hello").unwrap().is_some());
        assert!(send_message_signature(&config, "atlas", Some("other:m5"), "hello").unwrap_err().contains("does not match"));
        assert!(send_message_signature(&config, "atlas", None, "[fake] hello").unwrap_err().contains("bracket-prefixed"));
    }

    #[test]
    fn concurrent_send_audit_appends_remain_parseable_jsonl() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("concurrent");
        std::env::set_var("MAW_MESSAGE_LEDGER_DISABLE", "1");
        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        let workers = 64;

        std::thread::scope(|scope| {
            for index in 0..workers {
                let config = config.clone();
                scope.spawn(move || {
                    let raw_args = vec!["agent".to_owned(), format!("canary-{index}")];
                    let args = send_audit_args("hey", &raw_args);
                    send_record_success("hey", &args, &config, "atlas", None, "agent", &format!("[m5:atlas] canary-{index}"), "local", None);
                });
            }
        });

        assert_parseable_jsonl_count(&root.join("maw/audit.jsonl"), workers);
        assert_parseable_jsonl_count(&root.join("maw/maw-log.jsonl"), workers);
    }

    #[test]
    fn hey_log_correlates_fixture_jsonl_and_flags_suspicious_rows() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("hey-log");
        std::fs::write(root.join("maw/audit.jsonl"), include_str!("../../tests/fixtures/hey-log/audit.jsonl")).unwrap();
        std::fs::write(root.join("maw/maw-log.jsonl"), include_str!("../../tests/fixtures/hey-log/maw-log.jsonl")).unwrap();

        let output = hey_log_command(&send_acl_vec(&["--suspicious", "-n", "10"]));

        assert_eq!(output.code, 0);
        assert!(output.stderr.is_empty());
        assert!(!output.stdout.contains("2026-07-10T00:00:00.000Z"));
        assert!(output.stdout.contains("⚠ suspicious"));
        assert!(output.stdout.contains("from!=user"));
        assert!(output.stdout.contains("prefix-bypass"));
        assert!(output.stdout.contains("bad --from"));
    }

    #[test]
    fn hey_log_reader_missing_logs_returns_fast() {
        let _lock = env_test_lock();
        let (_root, _restores) = send_audit_test_env("hey-log-missing");
        let started = std::time::Instant::now();

        let output = hey_log_command(&send_acl_vec(&["--from", "nobody", "--since", "2026-07-10", "-n", "1"]));

        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "No hey log entries.\n");
        assert!(started.elapsed() < std::time::Duration::from_secs(1));
    }

    fn assert_parseable_jsonl_count(path: &std::path::Path, expected: usize) {
        let text = std::fs::read_to_string(path).expect("jsonl");
        let lines = text.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), expected, "{text}");
        for line in lines {
            let value: serde_json::Value = serde_json::from_str(line).unwrap_or_else(|error| panic!("invalid jsonl line: {error}: {line:?}"));
            assert!(value.as_object().is_some(), "{value}");
        }
    }

    #[test]
    fn send_acl_no_scope_same_scope_and_trusted_allow_peer_send() {
        let _lock = env_test_lock();
        let _env = SendAclEnvGuard::new("allow");
        let config = send_acl_config("alice");
        let sender = config.oracle.as_deref().expect("test oracle");
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer(
                "hey",
                "bob",
                &send_acl_args("remote-bob", "hello"),
                sender,
                false,
            )),
            ""
        );

        send_acl_write_scope("team", &["alice", "bob"]);
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer(
                "hey",
                "bob",
                &send_acl_args("remote-bob", "hello"),
                sender,
                false,
            )),
            ""
        );

        std::fs::remove_file(scope_native_path("team")).unwrap();
        scope_trust_add_to_path(&scope_trust_path(), "alice", "bob", "2026-06-26T00:00:00.000Z").unwrap();
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer(
                "hey",
                "bob",
                &send_acl_args("remote-bob", "hello"),
                sender,
                false,
            )),
            ""
        );
    }

    #[test]
    fn send_acl_cross_scope_queues_without_body_or_peer_key() {
        let _lock = env_test_lock();
        let env = SendAclEnvGuard::new("queue");
        send_acl_write_scope("team", &["alice", "carol"]);
        let args = send_acl_args("remote-bob", "SECRET_BODY token=abc123");
        let result = send_acl_gate_peer("hey", "bob", &args, "alice", false);
        let output = match result { SendAclGateResult::Queued(output) => output, other => panic!("expected queue, got {other:?}") };
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(output.stdout.contains("sender: alice"));
        assert!(output.stdout.contains("target: bob"));
        assert!(output.stdout.contains("maw inbox approve"));
        assert!(!output.stdout.contains("SECRET_BODY"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!env.root.join("state").join("peer-key").exists());
        let pending_dir = env.root.join("state").join("pending");
        let files = std::fs::read_dir(pending_dir).unwrap().collect::<Vec<_>>();
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn send_acl_approve_bypass_and_human_only_trust_rules() {
        let _lock = env_test_lock();
        let _env = SendAclEnvGuard::new("approve");
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");

        let sender = config.oracle.as_deref().expect("test oracle");
        let mut approve = send_acl_args("remote-bob", "hello");
        approve.approve = true;
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &approve, sender, false)),
            ""
        );
        assert!(!scope_trust_path().exists());

        approve.trust = true;
        assert_eq!(
            send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &approve, sender, false)),
            ""
        );
        let trusted = scope_trust_load_from_path(&scope_trust_path());
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].sender, "alice");
        assert_eq!(trusted[0].target, "bob");

        let err = parse_send_args("hey", &send_acl_vec(&["bob", "hello", "--trust"])).unwrap_err();
        assert!(err.contains("--trust requires --approve"));
    }

    #[test]
    fn send_acl_env_bypass_is_ignored_and_explicit_param_writes_no_trust() {
        let _lock = env_test_lock();
        let _env = SendAclEnvGuard::new("bypass");
        send_acl_write_scope("team", &["alice", "carol"]);
        std::env::set_var("MAW_ACL_BYPASS", "1");
        let queued = send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", false);
        assert!(
            matches!(queued, SendAclGateResult::Queued(_)),
            "env must not bypass ACL"
        );
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", true)), "");
        assert!(!scope_trust_path().exists());
        assert_eq!(std::env::var("MAW_ACL_BYPASS").as_deref(), Ok("1"));
    }

    #[test]
    fn send_acl_corrupt_acl_fails_open_with_loud_warning() {
        let _lock = env_test_lock();
        let _env = SendAclEnvGuard::new("corrupt");
        let dir = scope_native_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.json"), "{not json").unwrap();
        let stderr = send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", false));
        assert!(stderr.contains("warn: ACL check failed, allowing send"));
        assert!(stderr.contains("broken.json"));
        assert!(stderr.contains("fix"));

        std::fs::remove_file(dir.join("broken.json")).unwrap();
        std::fs::write(scope_trust_path(), "{not json").unwrap();
        let stderr = send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), "alice", false));
        assert!(stderr.contains("warn: ACL check failed, allowing send"));
        assert!(stderr.contains("scope-trust.json"));
    }

    #[test]
    fn send_acl_parser_accepts_approve_and_rejects_trust_alone() {
        let parsed = parse_send_args("hey", &send_acl_vec(&["bob", "hello", "--approve", "--trust"])).unwrap();
        assert!(parsed.approve);
        assert!(parsed.trust);
        let output = send_usage_error("hey", "hey: --trust requires --approve");
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("[--approve] [--trust]"));
    }

    #[test]
    fn hey_cli_matches_committed_maw_js_golden() {
        let fixture: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/hey-parity/maw-js-cli.json")).expect("valid maw-js hey fixture");
        let assert_output = |case: &serde_json::Value, output: CliOutput| {
            assert_eq!(output.code, i32::try_from(case["code"].as_i64().unwrap()).unwrap());
            assert_eq!(output.stdout, case["stdout"].as_str().unwrap());
            assert_eq!(output.stderr, case["stderr"].as_str().unwrap());
        };

        let no_args = tokio::runtime::Runtime::new().unwrap().block_on(run_send_like_async_impl("hey", &[]));
        assert_output(&fixture["noArgs"], no_args);

        let route = &fixture["routeError"];
        assert_output(route, CliOutput {
            code: send_error_code("hey"),
            stdout: String::new(),
            stderr: send_route_error("hey", route["target"].as_str().unwrap(), "", None),
        });

        let success = &fixture["localSuccess"];
        assert_output(success, CliOutput {
            code: 0,
            stdout: send_success_output("hey", success["target"].as_str().unwrap(), success["outbound"].as_str().unwrap()),
            stderr: String::new(),
        });
    }

    #[test]
    fn inbox_hey_send_args_keep_message_flags_opaque() {
        let args = send_args_for_inbox_hey(
            "bob",
            "hello --approve --from=mallory:edge --trust -leading",
        );

        assert_eq!(args.target, "bob");
        assert_eq!(
            args.text,
            "hello --approve --from=mallory:edge --trust -leading"
        );
        assert_eq!(args.inbox, None);
        assert_eq!(args.from, None);
        assert!(!args.approve);
        assert!(!args.trust);
    }


    #[test]
    fn send_acl_notify_cross_scope_queues_before_peer_transport() {
        let _lock = env_test_lock();
        let env = SendAclEnvGuard::new("notify-callsite");
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");
        let args = NotifyArgs {
            target: "remote-bob".to_owned(),
            text: "SECRET_NOTIFY token=abc123".to_owned(),
            from: None,
            approve: false,
            trust: false,
            force: false,
        };
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(notify_peer(
                "http://127.0.0.1:1",
                "bob",
                &args,
                &config,
                "alice",
            ));
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(!output.stdout.contains("SECRET_NOTIFY"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!env.root.join("state").join("peer-key").exists());
        assert_eq!(std::fs::read_dir(env.root.join("state").join("pending")).unwrap().count(), 1);
    }

    #[test]
    fn send_acl_talkto_cross_scope_queues_before_fake_or_real_transport() {
        let _lock = env_test_lock();
        let env = SendAclEnvGuard::new("talkto-callsite");
        let _fake = EnvVarRestore::capture("MAW_RS_TALKTO_FAKE_PEER_LOG");
        let fake_log = env.root.join("talkto-peer.jsonl");
        std::env::set_var("MAW_RS_TALKTO_FAKE_PEER_LOG", &fake_log);
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");
        let args = TalktoArgs { recipient: "remote-bob".to_owned(), message: "SECRET_TALK token=abc123".to_owned(), force: false };
        let output = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(talkto_peer("http://127.0.0.1:1", "bob", Some("remote"), &args, "SECRET_TALK token=abc123", &config, None));
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(!output.stdout.contains("SECRET_TALK"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!fake_log.exists(), "ACL queue must happen before fake/real peer transport");
        assert!(!env.root.join("state").join("peer-key").exists());
        assert_eq!(std::fs::read_dir(env.root.join("state").join("pending")).unwrap().count(), 1);
    }

    #[test]
    fn send_acl_queue_and_usage_match_committed_goldens() {
        assert_eq!(
            send_acl_format_queue_output("2026-06-26T00-00-00-000Z-a1b2c3", "alice", "bob"),
            include_str!("../../tests/fixtures/native-scope-acl/acl-queue.stdout")
        );
        let output = send_usage_error("hey", "hey: --trust requires --approve");
        assert_eq!(output.stderr, include_str!("../../tests/fixtures/native-scope-acl/send-usage.stderr"));
    }

    fn send_test_stdin(payload: &'static str) -> impl FnOnce() -> std::io::Cursor<&'static [u8]> {
        move || std::io::Cursor::new(payload.as_bytes())
    }

    fn send_no_stdin() -> impl FnOnce() -> std::io::Cursor<&'static [u8]> {
        || panic!("stdin must not be read for this invocation")
    }

    #[test]
    fn hey_file_source_delivers_shell_hostile_bytes_identical_to_sinks() {
        let _lock = env_test_lock();
        let (root, _restores) = send_audit_test_env("file-source");
        let payload = "review: `cargo test` ate $YESTERDAY and $(point-c)\nline2 'single' \"double\" \\backslash\n";
        let path = root.join("message.txt");
        std::fs::write(&path, payload).unwrap();
        let path_arg = path.to_str().unwrap();

        let args = parse_send_args_with_stdin("hey", &send_acl_vec(&["bob", "-f", path_arg]), send_no_stdin()).expect("parse -f");
        assert_eq!(args.target, "bob");
        assert_eq!(args.text, payload, "file bytes must pass through untouched");
        assert_eq!(args.from, None);
        assert!(!args.approve && !args.trust && !args.dry_run);

        let config = HeyConfig { node: Some("m5".to_owned()), oracle: Some("atlas".to_owned()), route: RouteConfig::default() };
        send_record_success("hey", &send_audit_args("hey", &send_acl_vec(&["bob", "-f", path_arg])), &config, "atlas", None, "bob", &args.text, "local", None);
        let log: serde_json::Value = serde_json::from_str(std::fs::read_to_string(root.join("maw/maw-log.jsonl")).unwrap().trim()).unwrap();
        assert_eq!(log["msg"], serde_json::json!(payload));
        if std::process::Command::new("sqlite3").arg("-version").output().is_ok() {
            let output = std::process::Command::new("sqlite3")
                .arg(root.join("maw/message-ledger.sqlite"))
                .arg("select text from messages;")
                .output()
                .unwrap();
            assert_eq!(String::from_utf8(output.stdout).unwrap(), format!("{payload}\n"));
        }
    }

    #[test]
    fn hey_stdin_dash_source_reads_shell_hostile_bytes_identical() {
        let payload = "a `b` $c\n$(never-runs) \"q\" 'w'";
        let args = parse_send_args_with_stdin("hey", &send_acl_vec(&["bob", "-"]), send_test_stdin(payload)).expect("parse -");
        assert_eq!(args.target, "bob");
        assert_eq!(args.text, payload, "stdin bytes must pass through untouched");

        let empty = parse_send_args_with_stdin("hey", &send_acl_vec(&["bob", "-"]), send_test_stdin("")).unwrap_err();
        assert_eq!(empty, "hey: missing message for 'bob'", "empty stdin must match today's empty-message error");
    }

    #[test]
    fn hey_rejects_positional_message_combined_with_file_or_stdin_source() {
        let parse = |argv: &[&str]| parse_send_args_with_stdin("hey", &send_acl_vec(argv), send_no_stdin()).unwrap_err();
        assert_eq!(parse(&["bob", "-f", "/tmp/x", "hello"]), "hey: message given both as argument and via -f <file>; use exactly one");
        assert_eq!(parse(&["bob", "-", "hello"]), "hey: message given both as argument and via '-' (stdin); use exactly one");
        assert_eq!(parse(&["bob", "-f", "/tmp/x", "-"]), "hey: message can come from only one of -f <file> or '-' (stdin)");
        assert_eq!(parse(&["bob", "-f"]), "hey: missing -f value (path to message file)");
    }

    #[test]
    fn hey_missing_and_empty_message_files_error_actionably() {
        let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let root = std::env::temp_dir().join(format!("maw-hey-file-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();

        let missing = root.join("nope.txt");
        let err = parse_send_args_with_stdin("hey", &send_acl_vec(&["bob", "-f", missing.to_str().unwrap()]), send_no_stdin()).unwrap_err();
        assert!(err.starts_with(&format!("hey: cannot read message file '{}':", missing.display())), "{err}");

        let empty = root.join("empty.txt");
        std::fs::write(&empty, "").unwrap();
        let err = parse_send_args_with_stdin("hey", &send_acl_vec(&["bob", "-f", empty.to_str().unwrap()]), send_no_stdin()).unwrap_err();
        assert_eq!(err, "hey: missing message for 'bob'", "empty file must match today's empty-message error");
        let output = send_usage_error("hey", &err);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("✗ missing message for target 'bob'"));
    }

    fn send_acl_vec(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
}
