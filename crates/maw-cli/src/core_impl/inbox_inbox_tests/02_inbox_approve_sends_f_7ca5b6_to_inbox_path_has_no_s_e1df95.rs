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
        let pending = inbox_load_pending_for_env(&env, inbox_now_ms()).unwrap();
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
        let part62_prod = read_core_impl_split_prod(manifest_dir, "inbox_");
        assert!(!part62_prod.contains("Command::new"));
        assert!(!part62_prod.contains("current_exe"));
        assert!(!part62_prod.contains("MAW_ACL_BYPASS"));

        let part29_prod = read_core_impl_split_prod(manifest_dir, "async_messaging_");
        assert!(!part29_prod.contains("std::env::var(\"MAW_ACL_BYPASS\")"));
        assert!(!part29_prod.contains("std::env::var_os(\"MAW_ACL_BYPASS\")"));
    }

    fn read_core_impl_split_prod(manifest_dir: &std::path::Path, prefix: &str) -> String {
        let core_impl = manifest_dir.join("src/core_impl");
        let order = std::fs::read_to_string(core_impl.join("parts.order")).expect("read parts.order");
        order
            .lines()
            .filter(|part| part.starts_with(prefix))
            .map(|part| {
                let text = std::fs::read_to_string(core_impl.join(part)).expect("read split part");
                if let Some((prod, _tests)) = text.split_once("#[cfg(test)]") {
                    prod.to_owned()
                } else {
                    text
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
