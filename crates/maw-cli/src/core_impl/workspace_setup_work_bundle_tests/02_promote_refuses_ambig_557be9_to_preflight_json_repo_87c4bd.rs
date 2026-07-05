    #[test]
    fn promote_refuses_ambiguous_and_destination_exists_without_mutation() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.sessions.push(TmuxSession { name: "other".to_owned(), windows: vec![promote_test_window("test-cli")] });
        let ambiguous = promote_run_command_with(&work_args(&["test-cli"]), &mut tmux);
        assert_eq!(ambiguous.code, 1);
        assert_eq!(ambiguous.stderr, include_str!("../../../tests/fixtures/native-promote/promote-ambiguous.stderr"));
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.existing.insert("isolated".to_owned());
        let exists = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(exists.code, 1);
        assert_eq!(exists.stderr, include_str!("../../../tests/fixtures/native-promote/promote-dst-exists.stderr"));
        assert!(tmux.mutation_calls.is_empty());
    }

    #[test]
    fn promote_refuses_only_window_and_bad_inputs_before_mutation() {
        let mut tmux = PromoteFakeTmux::promote_fixture();
        let solo = promote_run_command_with(&work_args(&["scratch:scratch", "--as", "isolated"]), &mut tmux);
        assert_eq!(solo.code, 1);
        assert!(solo.stderr.contains("only window in session 'scratch'"));
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        let bad_as = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "-bad"]), &mut tmux);
        assert_eq!(bad_as.code, 1);
        assert!(bad_as.stderr.contains("not start with '-'") || bad_as.stderr.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        let old_shape = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--base", "alpha"]), &mut tmux);
        assert_eq!(old_shape.code, 1);
        assert!(old_shape.stderr.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
        assert!(tmux.mutation_calls.is_empty());
    }

    #[test]
    fn promote_force_merge_existing_destination_never_kills_existing_dst() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.sessions.push(TmuxSession { name: "isolated".to_owned(), windows: vec![promote_test_window("existing")] });
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated", "--force"]), &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, include_str!("../../../tests/fixtures/native-promote/promote-existing-force.stdout"));
        assert_eq!(tmux.mutation_calls, ["move-window -s 77-mawjs:test-cli -t isolated:"]);
        assert!(tmux.mutation_calls.iter().all(|call| !call.starts_with("kill-")));
    }

    #[test]
    fn promote_rolls_back_created_placeholder_but_not_foreign_windows() {
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.verify_missing = true;
        let verify = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(verify.code, 1);
        assert!(verify.stderr.contains("rolled back placeholder session"));
        assert!(tmux.mutation_calls.contains(&"kill-session -t isolated".to_owned()));

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.verify_missing = true;
        tmux.move_should_fail = false;
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("rolled back placeholder session"));
    }

    #[test]
    fn promote_q1_foreign_window_safe_and_q2_list_fail_conservative_no_kill_session() {
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.move_should_fail = true;
        tmux.foreign_before_rollback = true;
        let move_fail = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(move_fail.code, 1);
        assert!(move_fail.stderr.contains("move failed"));
        assert!(!tmux.mutation_calls.contains(&"kill-session -t isolated".to_owned()));
        assert!(tmux.mutation_calls.contains(&"kill-window -t isolated:__promote_placeholder__".to_owned()));

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.list_windows_fail_for.insert("isolated".to_owned());
        let list_fail = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(list_fail.code, 1);
        assert!(list_fail.stderr.contains("no session rollback performed because ownership cannot be verified"));
        assert!(!tmux.mutation_calls.contains(&"kill-session -t isolated".to_owned()));
        assert!(tmux.mutation_calls.contains(&"kill-window -t isolated:__promote_placeholder__".to_owned()));
    }

    #[test]
    fn snapshots_create_list_show_are_hermetic() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = WorkBundleEnvGuard::work_new();
        let create = snapshots_run_command(&work_args(&["create", "alpha_snap"]));
        assert_eq!(create.code, 0, "{}", create.stderr);
        let list = snapshots_run_command(&work_args(&["list"]));
        assert!(list.stdout.contains("alpha_snap"));
        let show = snapshots_run_command(&work_args(&["show", "alpha_snap", "--json"]));
        assert!(show.stdout.contains("\"name\":\"alpha_snap\""));
    }

    #[test]
    fn preflight_json_reports_temp_git_repo_clean() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let env = WorkBundleEnvGuard::work_new();
        std::process::Command::new("git").arg("init").arg(&env.root).output().expect("git init");
        let out = preflight_run_command(&work_args(&[env.root.to_str().expect("utf8"), "--json"]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("\"git\":true"));
    }
