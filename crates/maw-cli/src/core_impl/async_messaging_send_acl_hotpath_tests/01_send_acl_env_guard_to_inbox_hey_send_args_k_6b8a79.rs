    use super::*;

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

    fn send_acl_args(target: &str, text: &str) -> SendArgs {
        SendArgs { target: target.to_owned(), text: text.to_owned(), inbox: None, from: None, approve: false, trust: false }
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
    fn send_acl_no_scope_same_scope_and_trusted_allow_peer_send() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("allow");
        let config = send_acl_config("alice");
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &config, false)), "");

        send_acl_write_scope("team", &["alice", "bob"]);
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &config, false)), "");

        std::fs::remove_file(scope_native_path("team")).unwrap();
        scope_trust_add_to_path(&scope_trust_path(), "alice", "bob", "2026-06-26T00:00:00.000Z").unwrap();
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &config, false)), "");
    }

    #[test]
    fn send_acl_cross_scope_queues_without_body_or_peer_key() {
        let _lock = env_test_lock().lock().unwrap();
        let env = SendAclEnvGuard::new("queue");
        send_acl_write_scope("team", &["alice", "carol"]);
        let args = send_acl_args("remote-bob", "SECRET_BODY token=abc123");
        let result = send_acl_gate_peer("hey", "bob", &args, &send_acl_config("alice"), false);
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
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("approve");
        send_acl_write_scope("team", &["alice", "carol"]);
        let config = send_acl_config("alice");

        let mut approve = send_acl_args("remote-bob", "hello");
        approve.approve = true;
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &approve, &config, false)), "");
        assert!(!scope_trust_path().exists());

        approve.trust = true;
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &approve, &config, false)), "");
        let trusted = scope_trust_load_from_path(&scope_trust_path());
        assert_eq!(trusted.len(), 1);
        assert_eq!(trusted[0].sender, "alice");
        assert_eq!(trusted[0].target, "bob");

        let err = parse_send_args("hey", &send_acl_vec(&["bob", "hello", "--trust"])).unwrap_err();
        assert!(err.contains("--trust requires --approve"));
    }

    #[test]
    fn send_acl_env_bypass_is_ignored_and_explicit_param_writes_no_trust() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("bypass");
        send_acl_write_scope("team", &["alice", "carol"]);
        std::env::set_var("MAW_ACL_BYPASS", "1");
        let queued = send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &send_acl_config("alice"), false);
        assert!(
            matches!(queued, SendAclGateResult::Queued(_)),
            "env must not bypass ACL"
        );
        assert_eq!(send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &send_acl_config("alice"), true)), "");
        assert!(!scope_trust_path().exists());
        assert_eq!(std::env::var("MAW_ACL_BYPASS").as_deref(), Ok("1"));
    }

    #[test]
    fn send_acl_corrupt_acl_fails_open_with_loud_warning() {
        let _lock = env_test_lock().lock().unwrap();
        let _env = SendAclEnvGuard::new("corrupt");
        let dir = scope_native_dir();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("broken.json"), "{not json").unwrap();
        let stderr = send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &send_acl_config("alice"), false));
        assert!(stderr.contains("warn: ACL check failed, allowing send"));
        assert!(stderr.contains("broken.json"));
        assert!(stderr.contains("fix"));

        std::fs::remove_file(dir.join("broken.json")).unwrap();
        std::fs::write(scope_trust_path(), "{not json").unwrap();
        let stderr = send_acl_assert_proceed(send_acl_gate_peer("hey", "bob", &send_acl_args("remote-bob", "hello"), &send_acl_config("alice"), false));
        assert!(stderr.contains("warn: ACL check failed, allowing send"));
        assert!(stderr.contains("scope-trust.json"));
    }

    #[test]
    fn send_acl_parser_accepts_approve_and_rejects_trust_alone() {
        let parsed = parse_send_args("hey", &send_acl_vec(&["bob", "hello", "--approve", "--trust"])).unwrap();
        assert!(parsed.approve);
        assert!(parsed.trust);
        let output = send_usage_error("hey", "hey: --trust requires --approve");
        assert_eq!(output.code, 2);
        assert!(output.stderr.contains("[--approve] [--trust]"));
    }

    #[test]
    fn hey_pairing_diagnostic_reports_missing_identity_and_copyable_pair_commands_without_secrets() {
        let diagnostic = hey_pairing_diagnostic(
            "hey",
            "http://peer.example:31745",
            "nova:bigboy-vps",
            "remote /api/send returned HTTP 401: unauthorized (decision=refuse-missing-peer-key)",
        );

        assert!(diagnostic.contains("peer pairing is required and still fail-closed"));
        assert!(diagnostic.contains("missing from: nova:bigboy-vps"));
        assert!(diagnostic.contains("missing node: bigboy-vps"));
        assert!(diagnostic.contains("peer key: not paired (redacted)"));
        assert!(diagnostic.contains("maw pair generate --at http://peer.example:31745"));
        assert!(diagnostic.contains("maw pair http://peer.example:31745 <PAIR-CODE>"));
        assert!(!diagnostic.contains("feedface"));
        assert!(!diagnostic.contains("SECRET"));
        assert!(!diagnostic.contains("peer_key"));
        assert!(!diagnostic.contains("pubkey"));
    }

    #[test]
    fn hey_pairing_diagnostic_is_auth_decision_specific() {
        assert_eq!(
            hey_pairing_diagnostic(
                "hey",
                "http://peer.example:31745",
                "nova:bigboy-vps",
                "network error posting http://peer.example:31745/api/send",
            ),
            ""
        );
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


