// The inbox suite, kept whole.
//
// Moved out of inbox.rs so the module reads as the thing it implements rather than
// the thing plus its scaffolding. Not split further: these tests share one
// fixture harness, and a private item in one `mod` is invisible to a sibling,
// so carving them up would mean exporting the harness for no real gain.

#[cfg(test)]
mod inbox_tests {
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

    // 2026-06-25T00:05:00.000Z -- five minutes after every fixture's
    // hardcoded `sent_at`/timestamp in this file. Every inbox test runs
    // against this frozen instant, not the real wall clock: pending items
    // are gated by a 30-day TTL (INBOX_PENDING_TTL_SECONDS) measured from
    // `inbox_now_ms()`, so a fixture dated 2026-06-25 silently "expires"
    // once real time drifts more than 30 days past it -- which is exactly
    // what made 5 of these tests fail starting sometime after 2026-07-25
    // (#700/#688), despite every path already being isolated via InboxEnv.
    const INBOX_TEST_NOW_MS: u64 = 1_782_345_900_000;

    fn inbox_run_test(
        argv: &[String],
        env: &InboxEnv,
        sender: &mut impl InboxSender,
    ) -> Result<String, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(inbox_run_at(argv, env, sender, INBOX_TEST_NOW_MS))
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
        inbox_write_fixture_at(
            env,
            filename,
            from,
            read,
            "2026-06-25T00:00:00.000Z",
            body,
        );
    }

    #[test]
    fn inbox_help_prints_usage_to_stdout_path() {
        let env = inbox_temp_env("help");
        let mut sender = InboxFakeSender::default();

        let output = inbox_run_test(&inbox_strings(&["--help"]), &env, &mut sender).unwrap();

        assert_eq!(output, format!("usage: {INBOX_USAGE}\n"));
    }

    fn inbox_write_fixture_at(
        env: &InboxEnv,
        filename: &str,
        from: &str,
        read: bool,
        timestamp: &str,
        body: &str,
    ) {
        std::fs::create_dir_all(&env.inbox_dir).unwrap();
        let text = format!(
            "---\nfrom: {from}\nto: nova\ntimestamp: {timestamp}\nread: {read}\n---\n\n{body}\n"
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
    fn inbox_list_prints_stable_row_ids() {
        let env = inbox_temp_env("list-ids");
        inbox_write_fixture_at(
            &env,
            "2026-06-25_00-00_alice_old.md",
            "alice",
            false,
            "2026-06-25T00:00:00.000Z",
            "older message",
        );
        inbox_write_fixture_at(
            &env,
            "2026-06-26_00-00_bob_new.md",
            "bob",
            true,
            "2026-06-26T00:00:00.000Z",
            "newer message",
        );
        let mut sender = InboxFakeSender::default();

        let list = inbox_run_test(&inbox_strings(&["list"]), &env, &mut sender).unwrap();

        assert!(list.contains("ID R FROM"), "{list}");
        assert!(list.contains("  1 \u{001b}[90m○\u{001b}[0m bob"), "{list}");
        assert!(list.contains("  2 \u{001b}[32m●\u{001b}[0m alice"), "{list}");
    }

    #[test]
    fn inbox_read_row_id_prints_message_and_marks_read() {
        let env = inbox_temp_env("read-row");
        inbox_write_fixture_at(
            &env,
            "2026-06-25_00-00_alice_old.md",
            "alice",
            false,
            "2026-06-25T00:00:00.000Z",
            "older message",
        );
        inbox_write_fixture_at(
            &env,
            "2026-06-26_00-00_bob_new.md",
            "bob",
            false,
            "2026-06-26T00:00:00.000Z",
            "first line\nsecond line with full body",
        );
        let mut sender = InboxFakeSender::default();

        let read = inbox_run_test(&inbox_strings(&["read", "1"]), &env, &mut sender).unwrap();

        assert!(read.contains("2026-06-26_00-00_bob_new.md"), "{read}");
        assert!(read.contains("from: bob"), "{read}");
        assert!(read.contains("when: 2026-06-26T00:00:00.000Z"), "{read}");
        assert!(read.contains("first line\nsecond line with full body"), "{read}");
        assert!(read.contains("marked read: 2026-06-26_00-00_bob_new.md"), "{read}");
        let stored = std::fs::read_to_string(env.inbox_dir.join("2026-06-26_00-00_bob_new.md")).unwrap();
        assert!(stored.contains("read: true"), "{stored}");
        assert!(stored.contains("readAt:"), "{stored}");
        let old = std::fs::read_to_string(env.inbox_dir.join("2026-06-25_00-00_alice_old.md")).unwrap();
        assert!(old.contains("read: false"), "{old}");
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
        assert_eq!(pending, include_str!("../../tests/fixtures/native-scope-acl/inbox-pending-list.stdout"));

        let detail = inbox_run_test(&inbox_strings(&["show-pending", "abc"]), &env, &mut sender).unwrap();
        assert_eq!(detail, include_str!("../../tests/fixtures/native-scope-acl/inbox-show-pending.stdout"));

        let approved = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();
        assert_eq!(approved, include_str!("../../tests/fixtures/native-scope-acl/inbox-approve.stdout"));
        assert_eq!(sender.sent, vec![("bob".to_owned(), "hello fleet".to_owned(), true)]);

        let rejected = inbox_run_test(&inbox_strings(&["reject", "def"]), &env, &mut sender).unwrap();
        assert_eq!(rejected, include_str!("../../tests/fixtures/native-scope-acl/inbox-reject.stdout"));
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

    #[test]
    fn inbox_approve_sends_flag_like_messages_as_opaque_text() {
        let cases = [
            ("approve", "hello --approve"),
            ("from", "hello --from=mallory:edge"),
            ("trust", "hello --trust"),
            ("leading", "-leading payload"),
        ];

        for (name, body) in cases {
            let env = inbox_temp_env(name);
            inbox_pending_fixture_with_message(&env, "abc123", "pending", body);
            let mut sender = InboxFakeSender::default();

            let approved =
                inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();

            assert!(approved.contains("approved: abc123"));
            assert_eq!(sender.sent, vec![("bob".to_owned(), body.to_owned(), true)]);
            assert!(std::env::var("MAW_ACL_BYPASS").is_err());
        }
    }

    #[test]
    fn inbox_pending_state_first_legacy_fallback_ttl_and_preview_only() {
        let env = inbox_temp_env("pending-state");
        let legacy = InboxPendingMessage {
            id: "same123".to_owned(),
            sender: "legacy".to_owned(),
            target: "bob".to_owned(),
            query: Some("bob".to_owned()),
            sent_at: "2026-06-25T00:00:00.000Z".to_owned(),
            status: "pending".to_owned(),
            message: "legacy full token SECRET_BODY".to_owned(),
        };
        inbox_write_pending(&env.pending_dir, &legacy).unwrap();
        let state = InboxPendingMessage {
            sender: "state".to_owned(),
            message: "state full token SECRET_BODY".to_owned(),
            ..legacy.clone()
        };
        inbox_write_pending(&inbox_state_pending_dir(&env), &state).unwrap();
        let expired = InboxPendingMessage {
            id: "old999".to_owned(),
            sent_at: "2026-05-01T00:00:00.000Z".to_owned(),
            ..state.clone()
        };
        inbox_write_pending(&inbox_state_pending_dir(&env), &expired).unwrap();

        let rows = inbox_load_pending_for_env(&env, inbox_parse_iso_ms("2026-06-26T00:00:00.000Z").unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sender, "state");
        assert!(!inbox_state_pending_dir(&env).join("old999.json").exists());

        let mut sender = InboxFakeSender::default();
        let list = inbox_run_test(&inbox_strings(&["queue"]), &env, &mut sender).unwrap();
        assert!(list.contains("same123"));
        assert!(list.contains("state"));
        assert!(!list.contains("SECRET_BODY"));
        let detail = inbox_run_test(&inbox_strings(&["show-pending", "same"]), &env, &mut sender).unwrap();
        assert!(detail.contains("SECRET_BODY"));
    }

    #[test]
    fn inbox_pending_approve_send_failure_keeps_file_for_retry() {
        let env = inbox_temp_env("pending-fail");
        inbox_pending_fixture(&env, "abc123", "pending");
        let mut sender = InboxFakeSender {
            fail: true,
            ..InboxFakeSender::default()
        };
        let err = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).expect_err("send failure");
        assert!(err.contains("fake send failed"));
        let path = inbox_state_pending_dir(&env).join("abc123.json");
        assert!(path.exists());
        let pending = inbox_load_pending_for_env(&env, INBOX_TEST_NOW_MS).unwrap();
        assert_eq!(pending[0].status, "pending");
    }

    #[test]
    fn inbox_pending_id_and_atomic_permissions_are_guarded() {
        let env = inbox_temp_env("pending-perms");
        inbox_pending_fixture(&env, "abc123", "pending");
        assert_eq!(
            inbox_pending_id(inbox_parse_iso_ms("2026-06-26T00:00:00.000Z").unwrap(), "A1B2c3").unwrap(),
            "2026-06-26T00-00-00-000Z-a1b2c3"
        );
        assert!(inbox_pending_id(0, "nope").is_err());
        let path = inbox_state_pending_dir(&env).join("abc123.json");
        assert!(path.exists());
        let siblings = std::fs::read_dir(inbox_state_pending_dir(&env))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!siblings
            .iter()
            .any(|name| std::path::Path::new(name).extension().is_some_and(|ext| ext == "tmp")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn inbox_guards_reject_leading_dash_and_paths() {
        let env = inbox_temp_env("guards");
        let mut sender = InboxFakeSender::default();
        assert!(inbox_run_test(&inbox_strings(&["--from", "-bad"]), &env, &mut sender).is_err());
        assert!(inbox_run_test(&inbox_strings(&["read", "../secret"]), &env, &mut sender).is_err());
        assert!(inbox_run_test(&inbox_strings(&["write", "-bad"]), &env, &mut sender).is_err());
        assert!(inbox_run_test(&inbox_strings(&["write", "--", "-ok"]), &env, &mut sender).is_ok());
    }

    #[test]
    fn inbox_dispatch_is_native() {
        assert_eq!(DISPATCH_62.len(), 1);
        assert_eq!(DISPATCH_62[0].command, "inbox");
    }

    #[test]
    fn inbox_path_has_no_self_spawn_or_acl_env_channel() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let part62 = std::fs::read_to_string(manifest_dir.join("src/core_impl/inbox.rs"))
            .expect("read part62");
        let part62_prod = part62
            .split_once("#[cfg(test)]")
            .map_or(part62.as_str(), |(prod, _tests)| prod);
        assert!(!part62_prod.contains("Command::new"));
        assert!(!part62_prod.contains("current_exe"));
        assert!(!part62_prod.contains("MAW_ACL_BYPASS"));

        let part29 = std::fs::read_to_string(manifest_dir.join("src/core_impl/send_federation.rs"))
            .expect("read part29");
        let part29_prod = part29
            .split_once("#[cfg(test)]")
            .map_or(part29.as_str(), |(prod, _tests)| prod);
        assert!(!part29_prod.contains("std::env::var(\"MAW_ACL_BYPASS\")"));
        assert!(!part29_prod.contains("std::env::var_os(\"MAW_ACL_BYPASS\")"));
    }
}
