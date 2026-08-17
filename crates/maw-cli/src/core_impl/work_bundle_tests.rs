// The workspace_scaffold_commands suite, kept whole.
//
// Moved out of workspace_scaffold_commands.rs so the module reads as the thing it implements rather than
// the thing plus its scaffolding. Not split further: these tests share one
// fixture harness, and a private item in one `mod` is invisible to a sibling,
// so carving them up would mean exporting the harness for no real gain.

#[cfg(test)]
mod work_bundle_tests {
    use super::*;

    struct WorkBundleEnvGuard {
        root: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl WorkBundleEnvGuard {
        fn work_new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-work-bundle-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home")).expect("home");
            std::fs::create_dir_all(root.join("state")).expect("state");
            std::env::set_var("HOME", root.join("home"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
            std::env::set_var("XDG_STATE_HOME", root.join("state"));
            std::env::set_var("XDG_DATA_HOME", root.join("data"));
            std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
            Self { root, saved }
        }
    }

    impl Drop for WorkBundleEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn work_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[derive(Default)]
    struct NewFakeTmux {
        existing: std::collections::BTreeSet<String>,
        options: std::collections::BTreeMap<(String, String), String>,
        calls: Vec<String>,
        current_session: String,
        current_window: String,
        next_pane: Option<String>,
        stdin_tty: bool,
        stdout_tty: bool,
        env_no_prompt: bool,
    }

    impl NewFakeTmux {
        fn with_skip_attach() -> Self {
            Self { env_no_prompt: true, current_session: "parent".to_owned(), current_window: "main".to_owned(), ..Self::default() }
        }

        fn seed_launch(&mut self, session: &str, cwd: &std::path::Path, command: &str, window: &str) {
            self.existing.insert(session.to_owned());
            self.options.insert((session.to_owned(), NEW_WORKSPACE_CWD_OPTION.to_owned()), cwd.display().to_string());
            self.options.insert((session.to_owned(), NEW_WORKSPACE_COMMAND_OPTION.to_owned()), command.to_owned());
            self.options.insert((session.to_owned(), NEW_WORKSPACE_WINDOW_OPTION.to_owned()), window.to_owned());
        }
    }

    impl NewWorkspaceTmuxNative for NewFakeTmux {
        fn new_has_session(&mut self, name: &str) -> bool {
            self.calls.push(format!("has-session {name}"));
            self.existing.contains(name)
        }

        fn new_session_create(&mut self, request: &NewSessionCreateNative) -> Result<String, String> {
            self.calls.push(format!(
                "new-session {} {} {} {:?} {:?}",
                request.name, request.window, request.cwd, request.command, request.print_format
            ));
            self.existing.insert(request.name.clone());
            Ok(self.next_pane.clone().unwrap_or_default())
        }

        fn new_first_pane_id(&mut self, target: &str) -> Option<String> {
            self.calls.push(format!("first-pane {target}"));
            self.next_pane.clone()
        }

        fn new_current_session_window(&mut self) -> Result<(String, String), String> {
            self.calls.push("display-current".to_owned());
            Ok((self.current_session.clone(), self.current_window.clone()))
        }

        fn new_split_window(&mut self, request: &NewSplitCreateNative) -> Result<String, String> {
            self.calls.push(format!(
                "split {:?} {} {:?} {:?}",
                request.direction, request.cwd, request.command, request.print_format
            ));
            Ok(self.next_pane.clone().unwrap_or_default())
        }

        fn new_select_pane_title(&mut self, pane: &str, title: &str) -> Result<(), String> {
            self.calls.push(format!("select-pane {pane} {title}"));
            Ok(())
        }

        fn new_read_session_option(&mut self, session: &str, option: &str) -> Result<Option<String>, String> {
            self.calls.push(format!("read-option {session} {option}"));
            Ok(self.options.get(&(session.to_owned(), option.to_owned())).cloned())
        }

        fn new_set_session_option(&mut self, session: &str, option: &str, value: &str) -> Result<(), String> {
            self.calls.push(format!("set-option {session} {option} {value}"));
            self.options.insert((session.to_owned(), option.to_owned()), value.to_owned());
            Ok(())
        }

        fn new_attach_session(&mut self, session: &str) -> Result<(), String> {
            self.calls.push(format!("attach {session}"));
            Ok(())
        }

        fn new_stdin_is_terminal(&self) -> bool { self.stdin_tty }
        fn new_stdout_is_terminal(&self) -> bool { self.stdout_tty }
        fn new_env_no_prompt(&self) -> bool { self.env_no_prompt }
    }

    #[derive(Default)]
    #[allow(clippy::struct_excessive_bools)]
    struct PromoteFakeTmux {
        sessions: Vec<TmuxSession>,
        existing: std::collections::BTreeSet<String>,
        caller_in_tmux: bool,
        calls: Vec<String>,
        mutation_calls: Vec<String>,
        move_should_fail: bool,
        kill_session_should_fail: bool,
        verify_missing: bool,
        list_windows_fail_for: std::collections::BTreeSet<String>,
        foreign_before_rollback: bool,
    }

    impl PromoteFakeTmux {
        fn promote_fixture() -> Self {
            Self {
                sessions: vec![
                    TmuxSession {
                        name: "77-mawjs".to_owned(),
                        windows: vec![promote_test_window("mawjs-oracle"), promote_test_window("test-cli")],
                    },
                    TmuxSession { name: "scratch".to_owned(), windows: vec![promote_test_window("scratch")] },
                ],
                caller_in_tmux: true,
                ..Self::default()
            }
        }

        fn promote_session_mut(&mut self, name: &str) -> Option<&mut TmuxSession> {
            self.sessions.iter_mut().find(|item| item.name == name)
        }
    }

    impl PromoteTmuxNative for PromoteFakeTmux {
        fn promote_list_all(&mut self) -> Result<Vec<TmuxSession>, String> {
            self.calls.push("list-all".to_owned());
            Ok(self.sessions.clone())
        }

        fn promote_list_windows(&mut self, session: &str) -> Result<Vec<maw_tmux::TmuxWindow>, String> {
            self.calls.push(format!("list-windows {session}"));
            if self.list_windows_fail_for.contains(session) {
                return Err("tmux list failed".to_owned());
            }
            self.sessions.iter().find(|item| item.name == session).map(|item| item.windows.clone()).ok_or_else(|| "no such session".to_owned())
        }

        fn promote_has_session(&mut self, name: &str) -> bool {
            self.calls.push(format!("has-session {name}"));
            self.existing.contains(name) || self.sessions.iter().any(|item| item.name == name)
        }

        fn promote_caller_in_tmux(&self) -> bool { self.caller_in_tmux }

        fn promote_new_session(&mut self, name: &str, window: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("new-session -d -s {name} -n {window}"));
            self.existing.insert(name.to_owned());
            self.sessions.push(TmuxSession { name: name.to_owned(), windows: vec![promote_test_window(window)] });
            Ok(())
        }

        fn promote_move_window(&mut self, src: &str, dst: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("move-window -s {src} -t {dst}"));
            let (src_session, src_window) = src.split_once(':').ok_or_else(|| "bad source".to_owned())?;
            let dst_session = dst.trim_end_matches(':');
            if self.move_should_fail {
                if self.foreign_before_rollback {
                    if let Some(dst) = self.promote_session_mut(dst_session) {
                        dst.windows.push(promote_test_window("foreign"));
                    }
                }
                return Err("move failed".to_owned());
            }
            if let Some(src_session_item) = self.promote_session_mut(src_session) {
                src_session_item.windows.retain(|window| window.name != src_window);
            }
            if !self.verify_missing {
                if let Some(dst_session_item) = self.promote_session_mut(dst_session) {
                    dst_session_item.windows.push(promote_test_window(src_window));
                }
            }
            Ok(())
        }

        fn promote_kill_session(&mut self, name: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("kill-session -t {name}"));
            if self.kill_session_should_fail {
                return Err("kill session failed".to_owned());
            }
            self.existing.remove(name);
            self.sessions.retain(|session| session.name != name);
            Ok(())
        }

        fn promote_kill_window(&mut self, target: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("kill-window -t {target}"));
            let Some((session, window)) = target.split_once(':') else { return Err("bad target".to_owned()); };
            if let Some(session_item) = self.promote_session_mut(session) {
                session_item.windows.retain(|item| item.name != window);
            }
            Ok(())
        }

        fn promote_switch_client(&mut self, session: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("switch-client -t {session}"));
            Ok(())
        }
    }

    fn promote_test_window(name: &str) -> maw_tmux::TmuxWindow {
        maw_tmux::TmuxWindow { index: 0, name: name.to_owned(), active: false, cwd: None }
    }

    #[test]
    fn work_dispatch_registers_seven_commands() {
        assert_eq!(DISPATCH_93.len(), 7);
        let commands = DISPATCH_93.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, ["work", "awake", "scaffold", "new", "promote", "preflight", "snapshots"]);
    }

    #[test]
    fn scaffold_creates_hermetic_plugin_and_new_creates_workspace_session() {
        let _lock = super::env_test_lock();
        let env = WorkBundleEnvGuard::work_new();
        let rust_dest = env.root.join("hello-rust");
        let args = work_args(&["hello-rust", "--dest", rust_dest.to_str().expect("utf8")]);
        let out = scaffold_run_command(&args);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(rust_dest.join("plugin.json").exists());

        let workspace = env.root.join("workspace");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let mut tmux = NewFakeTmux::with_skip_attach();
        tmux.next_pane = Some("%9".to_owned());
        let args = work_args(&[
            "team-room",
            "--path",
            workspace.to_str().expect("utf8"),
            "--window",
            "lead",
            "--cmd",
            "echo hi",
            "--print",
            "--no-fleet",
        ]);
        let out = new_run_command_with(&args, &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        let value: serde_json::Value = serde_json::from_str(&out.stdout).expect("json");
        assert_eq!(value["session"], "team-room");
        assert_eq!(value["window"], "lead");
        assert_eq!(value["pane_id"], "%9");
        assert_eq!(value["cwd"], workspace.display().to_string());
        assert_eq!(value["command"], "echo hi");
        assert_eq!(value["reused"], false);
        assert!(tmux.calls.iter().any(|call| call.starts_with("new-session team-room lead")));
        assert!(tmux.calls.iter().any(|call| call == &format!("set-option team-room {NEW_WORKSPACE_CWD_OPTION} {}", workspace.display())));
        assert!(!workspace.join("plugin.json").exists());
    }

    #[test]
    fn new_rejects_plugin_scaffold_flags_with_explicit_redirect() {
        let mut tmux = NewFakeTmux::with_skip_attach();
        let out = new_run_command_with(&work_args(&["hello-rust", "--rust"]), &mut tmux);
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("plugin-scaffold option"), "{}", out.stderr);
        assert!(out.stderr.contains("maw plugin create"), "{}", out.stderr);
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn new_dry_run_is_session_create_not_plugin_scaffold() {
        let _lock = super::env_test_lock();
        let env = WorkBundleEnvGuard::work_new();
        let workspace = env.root.join("workspace-dry");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let mut tmux = NewFakeTmux::with_skip_attach();
        let out = new_run_command_with(
            &work_args(&["dry-room", "--path", workspace.to_str().expect("utf8"), "--dry-run"]),
            &mut tmux,
        );
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("[dry-run] would create workspace session 'dry-room'"), "{}", out.stdout);
        assert!(!out.stdout.contains("plugin"), "{}", out.stdout);
        assert!(!tmux.calls.iter().any(|call| call.starts_with("new-session")));
    }

    #[test]
    fn new_auto_suffixes_colliding_auto_names_by_launch_context() {
        let _lock = super::env_test_lock();
        let env = WorkBundleEnvGuard::work_new();
        let workspace = env.root.join("auto-room");
        std::fs::create_dir_all(&workspace).expect("workspace");
        let mut tmux = NewFakeTmux::with_skip_attach();
        tmux.seed_launch("auto-room-echo-hi", &workspace, "different", "lead");
        let out = new_run_command_with(
            &work_args(&["--path", workspace.to_str().expect("utf8"), "--cmd", "echo hi", "--dry-run"]),
            &mut tmux,
        );
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("using 'auto-room-echo-hi-2'"), "{}", out.stdout);
    }

    #[test]
    fn work_guards_reject_separator_and_leading_dash_values() {
        assert!(work_run_command(&work_args(&["--"])).stderr.contains("separator"));
        assert!(awake_run_command(&work_args(&["--"])).stderr.contains("separator"));
        assert!(scaffold_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(new_run_command(&work_args(&["-bad"])).stderr.contains("invalid session name"));
        assert!(promote_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(preflight_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(snapshots_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
    }

    #[test]
    fn promote_mutates_missing_destination_with_golden_and_exact_argv() {
        let _lock = super::env_test_lock();
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated", "--attach"]), &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, include_str!("../../tests/fixtures/native-promote/promote-success.stdout"));
        assert_eq!(
            tmux.calls,
            ["list-all", "list-windows 77-mawjs", "list-all", "list-windows 77-mawjs", "has-session isolated", "list-windows isolated"]
        );
        assert_eq!(
            tmux.mutation_calls,
            [
                "new-session -d -s isolated -n __promote_placeholder__",
                "move-window -s 77-mawjs:test-cli -t isolated:",
                "kill-window -t isolated:__promote_placeholder__",
            ]
        );
    }

    #[test]
    fn promote_refuses_ambiguous_and_destination_exists_without_mutation() {
        let _lock = super::env_test_lock();
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.sessions.push(TmuxSession { name: "other".to_owned(), windows: vec![promote_test_window("test-cli")] });
        let ambiguous = promote_run_command_with(&work_args(&["test-cli"]), &mut tmux);
        assert_eq!(ambiguous.code, 1);
        assert_eq!(ambiguous.stderr, include_str!("../../tests/fixtures/native-promote/promote-ambiguous.stderr"));
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.existing.insert("isolated".to_owned());
        let exists = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(exists.code, 1);
        assert_eq!(exists.stderr, include_str!("../../tests/fixtures/native-promote/promote-dst-exists.stderr"));
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
        let _lock = super::env_test_lock();
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.sessions.push(TmuxSession { name: "isolated".to_owned(), windows: vec![promote_test_window("existing")] });
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated", "--force"]), &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, include_str!("../../tests/fixtures/native-promote/promote-existing-force.stdout"));
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
        let _lock = super::env_test_lock();
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
        let _lock = super::env_test_lock();
        let env = WorkBundleEnvGuard::work_new();
        std::process::Command::new("git").arg("init").arg(&env.root).output().expect("git init");
        let out = preflight_run_command(&work_args(&[env.root.to_str().expect("utf8"), "--json"]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("\"git\":true"));
    }
}
