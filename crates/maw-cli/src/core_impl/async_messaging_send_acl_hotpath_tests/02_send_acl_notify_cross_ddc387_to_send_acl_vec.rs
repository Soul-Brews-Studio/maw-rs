    #[test]
    fn send_acl_notify_cross_scope_queues_before_peer_transport() {
        let _lock = env_test_lock().lock().unwrap();
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
            .block_on(notify_peer("http://127.0.0.1:1", "bob", &args, &config));
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("queued pending ACL approval"));
        assert!(!output.stdout.contains("SECRET_NOTIFY"));
        assert!(!output.stdout.contains("abc123"));
        assert!(!env.root.join("state").join("peer-key").exists());
        assert_eq!(std::fs::read_dir(env.root.join("state").join("pending")).unwrap().count(), 1);
    }

    #[test]
    fn send_acl_talkto_cross_scope_queues_before_fake_or_real_transport() {
        let _lock = env_test_lock().lock().unwrap();
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
            include_str!("../../../tests/fixtures/native-scope-acl/acl-queue.stdout")
        );
        let output = send_usage_error("hey", "hey: --trust requires --approve");
        assert_eq!(output.stderr, include_str!("../../../tests/fixtures/native-scope-acl/send-usage.stderr"));
    }

    fn send_acl_vec(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
