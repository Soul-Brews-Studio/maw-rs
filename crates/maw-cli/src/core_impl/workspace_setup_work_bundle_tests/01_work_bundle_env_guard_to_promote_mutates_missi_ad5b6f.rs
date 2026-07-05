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
        fn promote_list_all(&mut self) -> Vec<TmuxSession> {
            self.calls.push("list-all".to_owned());
            self.sessions.clone()
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
    fn scaffold_and_new_create_hermetic_plugins() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let env = WorkBundleEnvGuard::work_new();
        let rust_dest = env.root.join("hello-rust");
        let args = work_args(&["hello-rust", "--dest", rust_dest.to_str().expect("utf8")]);
        let out = scaffold_run_command(&args);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(rust_dest.join("plugin.json").exists());
        let as_dest = env.root.join("hello-as");
        let args = work_args(&["hello-as", "--as", "--dest", as_dest.to_str().expect("utf8")]);
        let out = new_run_command(&args);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(as_dest.join("assembly/index.ts").exists());
    }

    #[test]
    fn work_guards_reject_separator_and_leading_dash_values() {
        assert!(work_run_command(&work_args(&["--"])).stderr.contains("separator"));
        assert!(awake_run_command(&work_args(&["--"])).stderr.contains("separator"));
        assert!(scaffold_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(new_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(promote_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(preflight_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(snapshots_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
    }

    #[test]
    fn promote_mutates_missing_destination_with_golden_and_exact_argv() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated", "--attach"]), &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, include_str!("../../../tests/fixtures/native-promote/promote-success.stdout"));
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

