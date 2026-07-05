    use super::*;

    #[derive(Default)]
    struct InboxFakeSender {
        sent: Vec<(String, String, bool)>,
        fail: bool,
    }

    impl InboxSender for InboxFakeSender {
        fn inbox_send<'a>(
            &'a mut self,
            query: &'a str,
            message: &'a str,
            acl_bypass: bool,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                inbox_validate_target_arg(query, "query")?;
                if std::env::var("MAW_ACL_BYPASS").is_ok() {
                    return Err("test leak: MAW_ACL_BYPASS should not be global".to_owned());
                }
                if self.fail {
                    return Err("fake send failed".to_owned());
                }
                self.sent
                    .push((query.to_owned(), message.to_owned(), acl_bypass));
                Ok(())
            })
        }
    }

    fn inbox_run_test(
        argv: &[String],
        env: &InboxEnv,
        sender: &mut impl InboxSender,
    ) -> Result<String, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(inbox_run(argv, env, sender))
    }

    fn inbox_temp_env(name: &str) -> InboxEnv {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "maw-inbox-test-{name}-{}-{nanos}",
            std::process::id()
        ));
        InboxEnv {
            inbox_dir: root.join("psi").join("inbox"),
            pending_dir: root.join("config").join("pending"),
            state_dir: root.join("state"),
            oracle: "nova".to_owned(),
            node: "cli".to_owned(),
        }
    }

    fn inbox_write_fixture(env: &InboxEnv, filename: &str, from: &str, read: bool, body: &str) {
        std::fs::create_dir_all(&env.inbox_dir).unwrap();
        let text = format!(
            "---\nfrom: {from}\nto: nova\ntimestamp: 2026-06-25T00:00:00.000Z\nread: {read}\n---\n\n{body}\n"
        );
        std::fs::write(env.inbox_dir.join(filename), text).unwrap();
    }

    fn inbox_pending_fixture(env: &InboxEnv, id: &str, status: &str) {
        inbox_pending_fixture_with_message(env, id, status, "hello fleet");
    }

    fn inbox_pending_fixture_with_message(env: &InboxEnv, id: &str, status: &str, body: &str) {
        let message = InboxPendingMessage {
            id: id.to_owned(),
            sender: "alice".to_owned(),
            target: "bob".to_owned(),
            query: Some("bob".to_owned()),
            sent_at: "2026-06-25T00:00:00.000Z".to_owned(),
            status: status.to_owned(),
            message: body.to_owned(),
        };
        inbox_write_pending(&inbox_state_pending_dir(env), &message).unwrap();
    }

    fn inbox_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn inbox_list_show_read_and_write_are_hermetic() {
        let env = inbox_temp_env("list");
        inbox_write_fixture(
            &env,
            "2026-06-25_00-00_alice_ci.md",
            "alice",
            false,
            "[alice] ci green confirmed",
        );
        let mut sender = InboxFakeSender::default();
        let list = inbox_run_test(
            &inbox_strings(&["--unread", "--from", "alice", "--last", "1"]),
            &env,
            &mut sender,
        )
        .unwrap();
        assert!(list.contains("INBOX"));
        assert!(list.contains("alice"));
        let show = inbox_run_test(&inbox_strings(&["show", "ci"]), &env, &mut sender).unwrap();
        assert!(show.contains("ci green confirmed"));
        let read = inbox_run_test(&inbox_strings(&["read", "ci"]), &env, &mut sender).unwrap();
        assert!(read.contains("marked read"));
        let write =
            inbox_run_test(&inbox_strings(&["write", "new", "note"]), &env, &mut sender).unwrap();
        assert!(write.contains("wrote"));
    }

    #[test]
    fn inbox_drain_safe_dry_run_matches_golden_shape() {
        let env = inbox_temp_env("drain");
        inbox_write_fixture(
            &env,
            "2026-06-24_00-00_alice_ci.md",
            "alice",
            false,
            "[alice] ci green confirmed",
        );
        let mut sender = InboxFakeSender::default();
        let out = inbox_run_test(
            &inbox_strings(&["drain", "--safe", "--dry-run", "--older-than-hours", "0"]),
            &env,
            &mut sender,
        )
        .unwrap();
        assert!(out.contains("nova: would archive 1/1 safe stale inbox message"));
        assert!(out.contains("ci-green"));
        assert!(env.inbox_dir.join("2026-06-24_00-00_alice_ci.md").exists());
    }

    #[test]
    fn inbox_status_json_writes_temp_cursor_only() {
        let env = inbox_temp_env("status");
        inbox_write_fixture(
            &env,
            "2026-06-25_00-00_alice_ci.md",
            "alice",
            false,
            "hello",
        );
        let status = inbox_build_status("nova", &env.inbox_dir, &env, 1_766_620_800_000).unwrap();
        assert_eq!(status.unread, 1);
        assert!(env.state_dir.join("inbox-cursor.json").exists());
        let json = inbox_render_status(&status, true).unwrap();
        assert!(json.contains("\"oldest_age_seconds\""));
    }

    #[test]
    fn inbox_pending_acl_surfaces_match_committed_goldens() {
        let env = inbox_temp_env("pending-golden");
        inbox_pending_fixture(&env, "abc123", "pending");
        inbox_pending_fixture(&env, "def456", "pending");
        let mut sender = InboxFakeSender::default();

        let pending = inbox_run_test(&inbox_strings(&["pending"]), &env, &mut sender).unwrap();
        assert_eq!(pending, include_str!("../../../tests/fixtures/native-scope-acl/inbox-pending-list.stdout"));

        let detail = inbox_run_test(&inbox_strings(&["show-pending", "abc"]), &env, &mut sender).unwrap();
        assert_eq!(detail, include_str!("../../../tests/fixtures/native-scope-acl/inbox-show-pending.stdout"));

        let approved = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();
        assert_eq!(approved, include_str!("../../../tests/fixtures/native-scope-acl/inbox-approve.stdout"));
        assert_eq!(sender.sent, vec![("bob".to_owned(), "hello fleet".to_owned(), true)]);

        let rejected = inbox_run_test(&inbox_strings(&["reject", "def"]), &env, &mut sender).unwrap();
        assert_eq!(rejected, include_str!("../../../tests/fixtures/native-scope-acl/inbox-reject.stdout"));
    }

    #[test]
    fn inbox_pending_show_approve_reject_are_hermetic() {
        let env = inbox_temp_env("pending");
        inbox_pending_fixture(&env, "abc123", "pending");
        inbox_pending_fixture(&env, "def456", "pending");
        let mut sender = InboxFakeSender::default();
        let pending = inbox_run_test(&inbox_strings(&["pending"]), &env, &mut sender).unwrap();
        assert!(pending.contains("abc123"));
        let detail =
            inbox_run_test(&inbox_strings(&["show-pending", "abc"]), &env, &mut sender).unwrap();
        assert!(detail.contains("message:"));
        let approved = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();
        assert!(approved.contains("approved: abc123"));
        assert_eq!(
            sender.sent,
            vec![("bob".to_owned(), "hello fleet".to_owned(), true)]
        );
        assert!(std::env::var("MAW_ACL_BYPASS").is_err());
        assert!(!inbox_state_pending_dir(&env).join("abc123.json").exists());
        let rejected = inbox_run_test(&inbox_strings(&["reject", "def"]), &env, &mut sender).unwrap();
        assert!(rejected.contains("rejected: def456"));
        assert!(!inbox_state_pending_dir(&env).join("def456.json").exists());
    }

