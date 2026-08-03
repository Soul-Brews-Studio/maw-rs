// The wake suite, kept whole.
//
// Like serve's, these tests share one harness -- WakeMockTmux plus a hermetic
// fixture that stands up a fake fleet registry and ghq root. Splitting them by
// topic would mean exporting that harness across sibling modules for no real
// gain; moving them out of wake.rs is the part that mattered.

#[cfg(test)]
mod wake_tests {
    use super::*;

    fn rpro_ent_fleet_entry() -> NativeFleetEntry {
        // #711 repro: 7 windows in one session, all pointing at the same
        // repo (a codex fan-out) -- exactly the shape the issue's evidence
        // table says breaks every multi-window session on a shared repo.
        let window = |name: &str| NativeFleetWindow {
            name: name.to_owned(),
            repo: "switchaphon/rpro-ent-oracle".to_owned(),
            kind: None,
            kind_source: None,
        };
        NativeFleetEntry {
            file: "05-rpro-ent.json".to_owned(),
            path: std::path::PathBuf::from("05-rpro-ent.json"),
            session: NativeFleetSession {
                name: "05-rpro-ent".to_owned(),
                windows: vec![
                    window("rpro-ent-oracle"),
                    window("rpro-ent-codex-1"),
                    window("rpro-ent-codex-2"),
                    window("rpro-ent-maw-rs-migration"),
                    window("rpro-ent-nats-books"),
                    window("rpro-ent-nats-book3"),
                    window("rpro-ent-nats-books-4-7"),
                ],
                ..NativeFleetSession::default()
            },
        }
    }

    #[test]
    fn wake_resolve_registry_target_picks_the_named_window_among_repo_siblings() {
        // #711: the window literally NAMED rpro-ent-oracle must win over six
        // siblings that only share its repo -- if this regresses, #711
        // reopens verbatim (7-way "ambiguous registry target"). The repo
        // isn't actually cloned on the test machine, so resolution still
        // errors downstream -- the point is WHICH error: "repo not cloned"
        // proves the matcher already picked exactly one candidate and moved
        // past the ambiguity check; "ambiguous registry target" would mean
        // #711 is still live.
        let entries = vec![rpro_ent_fleet_entry()];
        let error = wake_resolve_registry_target("rpro-ent-oracle", &entries, &[])
            .expect_err("repo is not actually cloned in this test");
        assert!(!error.contains("ambiguous"), "{error}");
        assert!(error.contains("not cloned"), "{error}");
    }

    #[test]
    fn wake_resolve_registry_target_session_stem_alone_stays_genuinely_ambiguous() {
        // The bare session stem ("rpro-ent", no window-discriminating part)
        // matches all 7 windows equally -- correctly ambiguous, not a bug:
        // there is no way to tell which of 7 windows the caller means.
        let entries = vec![rpro_ent_fleet_entry()];
        let error = wake_resolve_registry_target("rpro-ent", &entries, &[])
            .expect_err("7 equally-plausible windows must not silently pick one");
        assert!(error.contains("ambiguous"), "{error}");
    }

    #[derive(Debug, Default)]
    #[allow(clippy::struct_excessive_bools)]
    struct WakeMockTmux {
        sessions: Vec<TmuxSession>,
        actions: Vec<String>,
        fail_select: bool,
        detached_delay_ms: u64,
        detached_finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
        /// Scripted `pane_current_command` replies; the last entry repeats
        /// forever. Empty script means an instantly-launched engine ("claude").
        pane_command_script: Vec<String>,
        /// Scripted fresh-pane, pre-send command replies. Empty script means
        /// shell init is already settled ("zsh").
        pre_send_pane_command_script: Vec<String>,
        pane_command_error: bool,
        target_pane_id: Option<String>,
        pane_polls: usize,
        pre_send_polls: usize,
        post_send_polls: usize,
        send_pane_polls: Vec<usize>,
        fresh_pane_unsent: bool,
        /// Scripted visible-screen captures; the last entry repeats forever.
        /// Empty script means an empty (healthy, prompt-free) screen.
        pane_capture_script: Vec<String>,
        pane_capture_error: bool,
        pane_captures: usize,
    }

    impl WakeTmuxNative for WakeMockTmux {
        fn wake_list(&mut self) -> Vec<TmuxSession> { self.sessions.clone() }
        fn wake_has_session(&mut self, name: &str) -> bool { self.sessions.iter().any(|session| session.name == name) }
        fn wake_new_session(&mut self, name: &str, window: &str, cwd: &std::path::Path) -> Result<(), String> {
            self.actions.push(format!("new-session {name} {window} {}", cwd.display()));
            self.sessions.push(TmuxSession { name: name.to_owned(), windows: vec![maw_tmux::TmuxWindow { index: 0, name: window.to_owned(), active: true, cwd: Some(cwd.display().to_string()) }] });
            self.fresh_pane_unsent = true;
            Ok(())
        }
        fn wake_new_window(&mut self, session: &str, window: &str, cwd: &std::path::Path) -> Result<(), String> {
            self.actions.push(format!("new-window {session} {window} {}", cwd.display()));
            if let Some(existing) = self.sessions.iter_mut().find(|item| item.name == session) {
                existing.windows.push(maw_tmux::TmuxWindow {
                    index: u32::try_from(existing.windows.len()).unwrap_or(u32::MAX),
                    name: window.to_owned(),
                    active: false,
                    cwd: Some(cwd.display().to_string()),
                });
            }
            self.fresh_pane_unsent = true;
            Ok(())
        }
        fn wake_send_text(&mut self, target: &str, text: &str) -> Result<(), String> {
            self.send_pane_polls.push(self.pane_polls);
            self.fresh_pane_unsent = false;
            self.actions.push(format!("send {target} {text}"));
            Ok(())
        }
        fn wake_send_text_detached(&mut self, target: String, text: String) -> Result<Option<std::thread::JoinHandle<()>>, String> {
            self.send_pane_polls.push(self.pane_polls);
            self.fresh_pane_unsent = false;
            self.actions.push(format!("send-detached {target} {text}"));
            let delay_ms = self.detached_delay_ms;
            let finished = std::sync::Arc::clone(&self.detached_finished);
            Ok(Some(std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                finished.store(true, std::sync::atomic::Ordering::SeqCst);
            })))
        }
        fn wake_select_window(&mut self, target: &str) -> Result<(), String> {
            self.actions.push(format!("select {target}"));
            if self.fail_select { Err("mock attach failed".to_owned()) } else { Ok(()) }
        }
        fn wake_pane_current_command(&mut self, _target: &str) -> Result<String, String> {
            self.pane_polls += 1;
            if self.pane_command_error { return Err("mock pane query failed".to_owned()); }
            if self.fresh_pane_unsent {
                self.pre_send_polls += 1;
                if self.pre_send_pane_command_script.is_empty() { return Ok("zsh".to_owned()); }
                let index = (self.pre_send_polls - 1).min(self.pre_send_pane_command_script.len() - 1);
                return Ok(self.pre_send_pane_command_script[index].clone());
            }
            self.post_send_polls += 1;
            if self.pane_command_script.is_empty() { return Ok("claude".to_owned()); }
            let index = (self.post_send_polls - 1).min(self.pane_command_script.len() - 1);
            Ok(self.pane_command_script[index].clone())
        }
        fn wake_target_pane_id(&mut self, _target: &str) -> Result<String, String> {
            self.target_pane_id.clone().ok_or_else(|| "mock target pane missing".to_owned())
        }
        fn wake_pane_capture(&mut self, _target: &str) -> Result<String, String> {
            self.pane_captures += 1;
            if self.pane_capture_error { return Err("mock pane capture failed".to_owned()); }
            if self.pane_capture_script.is_empty() { return Ok(String::new()); }
            let index = (self.pane_captures - 1).min(self.pane_capture_script.len() - 1);
            Ok(self.pane_capture_script[index].clone())
        }
        fn wake_confirm_poll_sleep(&mut self, _delay: std::time::Duration) {}
    }

    fn wake_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn wake_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-wake-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    struct CwdRestore {
        previous: std::path::PathBuf,
    }

    impl CwdRestore {
        fn enter(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().expect("current dir before test");
            std::env::set_current_dir(path).expect("set test cwd");
            Self { previous }
        }
    }

    impl Drop for CwdRestore {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.previous).expect("restore test cwd");
        }
    }

    fn wake_with_fixture<F>(test: F)
    where
        F: FnOnce(&std::path::Path),
    {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _tmux = EnvVarRestore::capture("TMUX");
        let _tmux_pane = EnvVarRestore::capture("TMUX_PANE");
        let root = wake_temp_root("fixture");
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle")).expect("repo");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        std::env::remove_var("TMUX");
        std::env::remove_var("TMUX_PANE");
        test(&root);
    }

    fn wake_mock_tmux_with_existing_window(session: &str, window: &str) -> WakeMockTmux {
        WakeMockTmux {
            sessions: vec![TmuxSession {
                name: session.to_owned(),
                windows: vec![maw_tmux::TmuxWindow {
                    index: 0,
                    name: window.to_owned(),
                    active: true,
                    cwd: None,
                }],
            }],
            ..WakeMockTmux::default()
        }
    }

    #[test]
    fn wake_parse_flags_and_guard_option_injection() {
        let options = wake_parse_args(&wake_strings(&["neo", "--task", "issue-134", "--dry-run", "--no-attach", "--layout=legacy", "--fresh"])).expect("parse");
        assert_eq!(options.target, "neo");
        assert_eq!(options.task.as_deref(), Some("issue-134"));
        assert!(options.dry_run && options.no_attach && options.fresh);
        assert!(wake_parse_args(&wake_strings(&["neo", "-a"])).expect("parse -a").attach);
        assert!(wake_parse_args(&wake_strings(&["neo", "--yes"])).expect("parse yes").yes);
        assert!(wake_parse_args(&wake_strings(&["--", "neo"])).expect_err("separator guard").contains("unknown argument"));
        assert!(wake_parse_args(&wake_strings(&["neo", "--task", "-bad"])).expect_err("value guard").contains("must not start"));
    }

    #[test]
    fn wake_post_wake_hooks_write_marker_env() {
        wake_with_fixture(|root| {
            let session = wake_session_name("neo", &[]);
            let expected = format!("neo|{session}|neo");
            let cli_marker = root.join("cli-ready.txt");
            let cli_hook = format!(
                "printf '%s|%s|%s' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" > {}",
                wake_shell_quote(&cli_marker.display().to_string())
            );
            let mut tmux = WakeMockTmux::default();
            let (code, _stdout) = wake_run(
                &wake_strings(&["neo", "--no-attach", "--on-ready", "false", "--on-ready", &cli_hook]),
                &mut tmux,
            )
            .expect("wake with cli hooks");
            assert_eq!(code, 0);
            assert_eq!(std::fs::read_to_string(&cli_marker).expect("cli marker"), expected);

            let config_marker = root.join("config-ready.txt");
            let config_hook = format!(
                "printf '%s|%s|%s' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" > {}",
                wake_shell_quote(&config_marker.display().to_string())
            );
            std::fs::write(
                root.join("config/maw.config.50.json"),
                serde_json::to_string(&serde_json::json!({"hooks":{"postWake":[config_hook]}})).expect("json"),
            )
            .expect("write config hook");
            let mut tmux = WakeMockTmux { sessions: tmux.sessions, ..WakeMockTmux::default() };
            let (code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("wake with config hook");
            assert_eq!(code, 0);
            assert_eq!(std::fs::read_to_string(&config_marker).expect("config marker"), expected);
        });
    }

    #[test]
    fn wake_short_e_flag_and_config_commands_engine_resolution() {
        // short `-e` is accepted as an alias of `--engine`
        let options = wake_parse_args(&wake_strings(&["neo", "-e", "omx-1"])).expect("parse -e");
        assert_eq!(options.engine.as_deref(), Some("omx-1"));

        // custom engines resolve to their full command from merged config `commands`;
        // real binaries not in the map fall through to the literal name.
        wake_with_fixture(|root| {
            let dir = active_config_dir();
            std::fs::create_dir_all(&dir).expect("config dir");
            std::fs::write(
                dir.join("maw.config.50.json"),
                r#"{"commands":{"omx-1":"bun codex-setup.ts 1 && CODEX_HOME=$PWD/.codex omx --direct --madmax","default":"claude"}}"#,
            )
            .expect("write config");
            assert!(!dir.join("maw.config.json").exists());
            assert_eq!(
                wake_resolve_engine_command("omx-1", root),
                "bun codex-setup.ts 1 && CODEX_HOME=$PWD/.codex omx --direct --madmax"
            );
            assert_eq!(wake_resolve_engine_command("codex", root), "codex");
        });
    }

    #[test]
    fn wake_fresh_default_uses_config_default_and_resume_follows_the_resolved_engine() {
        wake_with_fixture(|_| {
            let dir = active_config_dir();
            std::fs::create_dir_all(&dir).expect("config dir");
            std::fs::write(dir.join("maw.config.50.json"), r#"{"commands":{"default":"claude"}}"#)
                .expect("write config");

            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("fresh");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo claude"), "{send}");
            assert!(!send.contains("codex"), "{send}");

            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "-e", "codex"]), &mut tmux).expect("explicit");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo codex"), "{send}");

            // --resume no longer hijacks the engine to codex (#615): the
            // repo's commands.default engine resumes with its own form.
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "--resume"]), &mut tmux).expect("resume");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo claude --continue"), "{send}");
        });
    }

    #[test]
    fn wake_resume_without_engine_config_still_lands_on_codex_with_subcommand_first() {
        wake_with_fixture(|_| {
            // No commands/wake config at all: the final fallback engine stays
            // codex, and the resume subcommand goes right after the binary.
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "--resume"]), &mut tmux).expect("resume");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo codex resume"), "{send}");
        });
    }

    #[test]
    fn wake_resume_config_entry_beats_engine_command_and_codex_fallback_injects_subcommand() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"omx-1":"OMX_POOL=1 omx --direct","omx-1-resume":"OMX_AUTO_UPDATE=0 omx --direct resume --last"},"wake":{"engine":"omx-1","resume":true}}"#,
            )
            .expect("repo config");

            // (a) commands.<engine>-resume is a COMPLETE replacement line and
            // wins over commands.<engine> + any fallback.
            let mut tmux = WakeMockTmux::default();
            let (_code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("config resume entry");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo OMX_AUTO_UPDATE=0 omx --direct resume --last"), "{send}");
            assert!(!stdout.contains("warning:"), "{stdout}");

            // (c) codex-family fallback (no <engine>-resume entry): `resume`
            // is injected as the subcommand right after the binary token, not
            // appended after the flags.
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"codex":"codex --search --dangerously-bypass-approvals-and-sandbox"},"wake":{"resume":true}}"#,
            )
            .expect("repo config codex");
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("codex fallback");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(
                send.ends_with("MAW_SESSION_WINDOW=neo codex resume --search --dangerously-bypass-approvals-and-sandbox"),
                "{send}"
            );
        });
    }

    #[test]
    fn wake_resume_claude_fallback_skips_env_and_command_builtin_then_appends_continue() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"c48":"ANTHROPIC_MODEL=claude-opus-4-8 command claude --dangerously-skip-permissions"},"wake":{"engine":"c48","resume":true}}"#,
            )
            .expect("repo config");

            // (b) claude-family fallback: binary detection skips VAR=VAL env
            // prefixes and the shell `command` builtin, then appends --continue.
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("claude fallback");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(
                send.ends_with("MAW_SESSION_WINDOW=neo ANTHROPIC_MODEL=claude-opus-4-8 command claude --dangerously-skip-permissions --continue"),
                "{send}"
            );
        });
    }

    #[test]
    fn wake_resume_unknown_binary_keeps_naive_append_but_warns() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"mystery":"mystery-bin --flag"},"wake":{"engine":"mystery","resume":true}}"#,
            )
            .expect("repo config");

            let mut tmux = WakeMockTmux::default();
            let (_code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("unknown binary");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo mystery-bin --flag resume"), "{send}");
            assert!(stdout.contains("warning:"), "{stdout}");
            assert!(stdout.contains("commands.mystery-resume"), "{stdout}");
        });
    }

    #[test]
    fn wake_engine_resolution_reads_repo_layer_config_for_the_resolved_repo() {
        // The test process cwd sits outside the fixture repo, so a passing
        // repo-layer lookup proves resolution is keyed on the resolved repo
        // path, not the invoking shell's cwd (#600).
        wake_with_fixture(|root| {
            let dir = active_config_dir();
            std::fs::create_dir_all(&dir).expect("config dir");
            std::fs::write(dir.join("maw.config.40.json"), r#"{"commands":{"omx-1":"user-omx"}}"#)
                .expect("user config");
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"omx-1":"CODEX_HOME=$PWD/.codex omx --direct"}}"#,
            )
            .expect("repo config");

            // Repo layer beats user config at equal weight (Project scope
            // outranks User); a dir outside the repo sees only the user layer.
            assert_eq!(wake_resolve_engine_command("omx-1", &repo), "CODEX_HOME=$PWD/.codex omx --direct");
            assert_eq!(wake_resolve_engine_command("omx-1", root), "user-omx");

            // End-to-end: waking the repo by name threads its resolved path
            // into engine resolution.
            let mut tmux = WakeMockTmux::default();
            let (code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "-e", "omx-1"]), &mut tmux).expect("wake");
            assert_eq!(code, 0);
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo CODEX_HOME=$PWD/.codex omx --direct"), "{send}");
        });
    }

    #[test]
    fn wake_defaults_block_fills_engine_channels_prompt_and_cli_flags_win() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"omx-1":"OMX_POOL=1 omx --direct"},"wake":{"engine":"omx-1","channels":true,"prompt":"read AGENTS.md first"}}"#,
            )
            .expect("repo config");

            // No flags: wake.engine resolves through the commands map and
            // wake.prompt fills in. wake.channels does NOT hand the
            // claude-only flag to a non-claude engine (#615) — it warns.
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("config defaults");
            assert_eq!(code, 0);
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo OMX_POOL=1 omx --direct 'read AGENTS.md first'"), "{send}");
            assert!(!send.contains("--channels"), "{send}");
            assert!(stdout.contains("warning:"), "{stdout}");
            assert!(stdout.contains("commands.omx-1-channels"), "{stdout}");

            // Explicit CLI flags beat the config defaults (`--channels` has no
            // negative flag, so the config value still applies there) — codex
            // is not claude-family either, so no claude flag.
            let mut tmux = WakeMockTmux::default();
            let (code, _stdout) = wake_run(
                &wake_strings(&["neo", "--no-attach", "-e", "codex", "--prompt", "hi"]),
                &mut tmux,
            )
            .expect("cli wins");
            assert_eq!(code, 0);
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo codex hi"), "{send}");
            assert!(!send.contains("--channels"), "{send}");

            // claude-family engines still get the channels flag, and a
            // commands.<engine>-channels entry is honored as a full
            // replacement line.
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"wake":{"engine":"claude","channels":true}}"#,
            )
            .expect("repo config claude");
            let mut tmux = WakeMockTmux::default();
            let (code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("claude channels");
            assert_eq!(code, 0);
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo claude --channels plugin:discord@claude-plugins-official"), "{send}");

            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                r#"{"commands":{"omx-1":"omx --direct","omx-1-channels":"omx --direct --with-channels"},"wake":{"engine":"omx-1","channels":true}}"#,
            )
            .expect("repo config channels entry");
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("channels entry");
            assert_eq!(code, 0);
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo omx --direct --with-channels"), "{send}");
            assert!(!stdout.contains("warning:"), "{stdout}");
        });
    }

    #[test]
    fn wake_defaults_block_resume_resumes_the_configured_engine_and_fresh_opts_out() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(repo.join(".maw/maw.config.40.json"), r#"{"wake":{"engine":"claude","resume":true}}"#)
                .expect("repo config");

            // (e) wake.resume no longer pins codex (#615): the configured
            // wake.engine resumes with its own family form.
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("config resume");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo claude --continue"), "{send}");

            // (f) --fresh opts out of the configured resume.
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "--fresh"]), &mut tmux).expect("fresh");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo claude"), "{send}");
            assert!(!send.contains(" resume"), "{send}");
            assert!(!send.contains("--continue"), "{send}");

            // Explicit -e still resumes that engine with its family form.
            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "-e", "claude"]), &mut tmux).expect("explicit engine");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=neo claude --continue"), "{send}");
        });
    }

    #[test]
    fn wake_post_wake_hooks_read_repo_layer_config() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/acme/neo-oracle");
            let marker = root.join("repo-hook.txt");
            let hook = format!(
                "printf '%s|%s|%s' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" > {}",
                wake_shell_quote(&marker.display().to_string())
            );
            std::fs::create_dir_all(repo.join(".maw")).expect("repo .maw");
            std::fs::write(
                repo.join(".maw/maw.config.40.json"),
                serde_json::to_string(&serde_json::json!({"hooks":{"postWake":[hook]}})).expect("json"),
            )
            .expect("repo config");

            let session = wake_session_name("neo", &[]);
            let mut tmux = WakeMockTmux::default();
            let (code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("wake with repo hook");
            assert_eq!(code, 0);
            // The process cwd is outside the repo — the hook can only come
            // from the repo-layer config resolved against repo_path.
            assert_eq!(std::fs::read_to_string(&marker).expect("repo marker"), format!("neo|{session}|neo"));
        });
    }

    #[test]
    fn wake_errors_when_pane_never_leaves_the_shell() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux { pane_command_script: vec!["zsh".to_owned()], ..WakeMockTmux::default() };
            let err = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect_err("shell-stuck pane must fail");
            assert!(err.contains("wake: engine did not start in"), "{err}");
            assert!(err.contains("pane still running 'zsh'"), "{err}");
            assert!(err.contains("— sent: "), "{err}");
            // Poll budget is bounded: initial check + one per backoff step.
            assert_eq!(tmux.pre_send_polls, 1);
            assert_eq!(tmux.post_send_polls, WAKE_LAUNCH_CONFIRM_BACKOFF_MS.len() + 1);
        });
    }

    #[test]
    fn wake_confirms_launch_without_exhausting_poll_for_engine_and_version_string() {
        wake_with_fixture(|_| {
            // Pane leaves the shell on the second poll — success, poll exits early.
            let mut tmux = WakeMockTmux {
                pane_command_script: vec!["zsh".to_owned(), "claude".to_owned()],
                ..WakeMockTmux::default()
            };
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("wake");
            assert_eq!(code, 0);
            assert!(stdout.contains("created session"), "{stdout}");
            assert_eq!(tmux.pre_send_polls, 1);
            assert_eq!(tmux.post_send_polls, 2);

            // A running claude engine can report a bare version string (#520);
            // "left the shell" must treat it as a healthy launch on poll one.
            let mut tmux = WakeMockTmux { pane_command_script: vec!["2.1.207".to_owned()], ..WakeMockTmux::default() };
            let (code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("wake version-string");
            assert_eq!(code, 0);
            assert_eq!(tmux.pre_send_polls, 1);
            assert_eq!(tmux.post_send_polls, 1);
        });
    }

    #[test]
    fn wake_keeps_legacy_success_when_pane_state_is_unreadable() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux { pane_command_error: true, ..WakeMockTmux::default() };
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("wake");
            assert_eq!(code, 0);
            assert!(stdout.contains("created session"), "{stdout}");
        });
    }

    #[test]
    fn wake_fails_fast_when_engine_is_stuck_at_trust_prompt() {
        // Engine left the shell but sits at the first-run directory-trust
        // dialog (#616) — the wake must report failure, not success.
        for prompt in [
            "codex\n\nDo you trust the contents of this directory?\n1. Yes, continue\n2. No, quit",
            "claude\n\nDo you trust the files in this folder?\n\n/opt/repo",
        ] {
            let mut tmux = WakeMockTmux { pane_capture_script: vec![prompt.to_owned()], ..WakeMockTmux::default() };
            let err = wake_confirm_engine_launch(&mut tmux, "neo:main", "codex --yolo")
                .expect_err("trust-prompt pane must fail");
            assert!(err.contains("directory-trust prompt in neo:main"), "{err}");
            assert!(err.contains("maw a neo"), "{err}");
            assert!(err.contains("sent: codex --yolo"), "{err}");
            // Marker found on the first capture — no settle re-captures needed.
            assert_eq!(tmux.pane_captures, 1);
        }
    }

    #[test]
    fn wake_fails_fast_when_trust_prompt_renders_after_a_settle_poll() {
        // The prompt can render slightly after the engine process appears;
        // the settle window must catch it on a re-capture.
        let mut tmux = WakeMockTmux {
            pane_capture_script: vec![
                "$ codex --yolo".to_owned(),
                "Do you trust the contents of this directory?".to_owned(),
            ],
            ..WakeMockTmux::default()
        };
        let err = wake_confirm_engine_launch(&mut tmux, "neo:main", "codex --yolo")
            .expect_err("late-rendering trust prompt must fail");
        assert!(err.contains("directory-trust prompt"), "{err}");
        assert_eq!(tmux.pane_captures, 2);
    }

    #[test]
    fn wake_confirms_launch_when_screen_shows_a_normal_banner() {
        let mut tmux = WakeMockTmux {
            pane_capture_script: vec!["✻ Welcome to Claude Code!\n\n> ".to_owned()],
            ..WakeMockTmux::default()
        };
        wake_confirm_engine_launch(&mut tmux, "neo:main", "claude").expect("healthy banner must confirm");
        // Settle window is bounded: immediate capture + the extra polls.
        assert_eq!(tmux.pane_captures, WAKE_TRUST_PROMPT_SETTLE_POLLS + 1);
    }

    #[test]
    fn wake_keeps_legacy_success_when_pane_capture_is_unreadable() {
        // Same principle as #580: an unreadable readback never fails an
        // otherwise healthy wake.
        let mut tmux = WakeMockTmux { pane_capture_error: true, ..WakeMockTmux::default() };
        wake_confirm_engine_launch(&mut tmux, "neo:main", "claude").expect("unreadable capture keeps legacy success");
        assert_eq!(tmux.pane_captures, 1);
    }

    #[test]
    fn wake_shell_stuck_pane_error_is_unchanged_and_never_captures() {
        // Pane never leaves the shell — the existing #580 error stands and the
        // trust-prompt capture path is never entered.
        let mut tmux = WakeMockTmux { pane_command_script: vec!["zsh".to_owned()], ..WakeMockTmux::default() };
        let err = wake_confirm_engine_launch(&mut tmux, "neo:main", "claude").expect_err("shell-stuck pane must fail");
        assert!(err.contains("wake: engine did not start in"), "{err}");
        assert_eq!(tmux.pane_captures, 0);
    }

    #[test]
    fn wake_pane_command_is_shell_matches_shells_not_engines() {
        for shell in ["zsh", "-zsh", "bash", "/bin/sh", "fish", ""] {
            assert!(wake_pane_command_is_shell(shell), "{shell:?} should read as a shell");
        }
        for engine in ["claude", "2.1.207", "codex", "node", "bun"] {
            assert!(!wake_pane_command_is_shell(engine), "{engine:?} should read as left-the-shell");
        }
    }

    #[test]
    fn wake_repo_path_flag_overrides_repo_resolution() {
        // `team up` passes `--repo-path <worktree>`; wake must accept it and use it
        // directly, bypassing ghq/fleet lookup.
        let options = wake_parse_args(&wake_strings(&[
            "coder-1", "--repo-path", "/tmp/wt/coder-1", "-e", "codex", "--no-attach",
        ]))
        .expect("parse --repo-path");
        assert_eq!(options.repo_path.as_deref(), Some(std::path::Path::new("/tmp/wt/coder-1")));
        assert_eq!(
            wake_repo_path(&options, "coder-1", &fleet_load_entries()).expect("resolve").path,
            std::path::PathBuf::from("/tmp/wt/coder-1")
        );
    }

    #[test]
    fn wake_reuses_workon_github_url_resolver_without_double_prefix_or_peer_route() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/Soul-Brews-Studio/maw-fleetpad");
            std::fs::create_dir_all(&repo).expect("repo");
            let args = wake_strings(&[
                "https://github.com/Soul-Brews-Studio/maw-fleetpad",
                "--dry-run",
                "--no-attach",
            ]);
            let options = wake_parse_args(&args).expect("parse");

            assert!(!wake_should_use_peer_target(&options));
            assert_eq!(wake_oracle(&options).expect("oracle"), "maw-fleetpad");
            assert_eq!(wake_repo_path(&options, "maw-fleetpad", &fleet_load_entries()).expect("resolve").path, repo);

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&args, &mut tmux).expect("run");
            assert_eq!(code, 0);
            assert!(stdout.contains("Soul-Brews-Studio/maw-fleetpad"), "{stdout}");
            assert!(!stdout.contains("github.com/github.com"), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_host_colon_target_and_peer_flag_route_to_peer_target() {
        // Issue #600 done-criterion 3: `host:target` (and `--peer <node>`) keep
        // routing through `run_wake_async` — the dir-aware local pipeline (and
        // its repo-layer config reads) never runs for peer wakes.
        let host_target = wake_parse_args(&wake_strings(&["mba:neo"])).expect("parse host:target");
        assert!(wake_should_use_peer_target(&host_target));

        let peer_flag = wake_parse_args(&wake_strings(&["neo", "--peer", "mba"])).expect("parse --peer");
        assert!(wake_should_use_peer_target(&peer_flag));

        // Local escape hatches still beat the colon heuristic.
        let dry_run = wake_parse_args(&wake_strings(&["mba:neo", "--dry-run"])).expect("parse dry-run");
        assert!(!wake_should_use_peer_target(&dry_run));
    }

    #[test]
    fn wake_reuses_workon_github_host_slug_resolver_without_double_prefix() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/Soul-Brews-Studio/maw-fleetpad");
            std::fs::create_dir_all(&repo).expect("repo");
            let options = wake_parse_args(&wake_strings(&[
                "github.com/Soul-Brews-Studio/maw-fleetpad",
                "--dry-run",
            ]))
            .expect("parse");

            assert_eq!(wake_repo_path(&options, "maw-fleetpad", &fleet_load_entries()).expect("resolve").path, repo);
        });
    }

    #[test]
    fn wake_fuzzy_resolves_middle_repo_segment_and_reports_match() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/laris-co/DustBoy-Phd-Oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let mut tmux = WakeMockTmux::default();

            let (code, stdout) = wake_run(&wake_strings(&["phd-oracle", "--dry-run"]), &mut tmux)
                .expect("fuzzy wake");

            assert_eq!(code, 0);
            assert!(stdout.contains("fuzzy match: DustBoy-Phd-Oracle"), "{stdout}");
            assert!(stdout.contains(&repo.display().to_string()), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_relative_repo_path_is_absolute_at_creation_and_send_is_bare_launch() {
        wake_with_fixture(|root| {
            let cwd = root.join("workspace");
            let repo = cwd.join("agents/1-codex-1");
            std::fs::create_dir_all(&repo).expect("worktree");
            let _cwd = CwdRestore::enter(&cwd);

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(
                &wake_strings(&[
                    "coder-1",
                    "--repo-path",
                    "agents/1-codex-1",
                    "-e",
                    "codex",
                    "--no-attach",
                ]),
                &mut tmux,
            )
            .expect("wake");
            assert_eq!(code, 0);
            assert!(stdout.contains("created session"));

            // The relative path is absolute by creation time: the pane starts
            // inside the worktree via tmux `-c`, not via an in-pane `cd`.
            let expected = repo.canonicalize().expect("canonical worktree");
            let new_session = tmux.actions.iter().find(|action| action.starts_with("new-session")).expect("new-session action");
            assert!(new_session.contains(&expected.display().to_string()), "{new_session}");

            // Work-parity launch line (#601): bare engine behind the env
            // prefix — no cd wrapper, no in-pane printf reporters. Failure
            // detection is #580's Rust-side pane poll.
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.ends_with("MAW_SESSION_WINDOW=coder-1 codex"), "{send}");
            assert!(!send.contains("cd "), "{send}");
            assert!(!send.contains("maw wake:"), "{send}");
        });
    }

    #[test]
    fn wake_missing_repo_path_fails_rust_side_before_any_tmux_action() {
        // #601 removed the in-pane `cd DIR || printf` guard; a bad repo path
        // must surface from the Rust side before anything is created, instead
        // of silently opening a pane in $HOME.
        wake_with_fixture(|root| {
            let missing = root.join("workspace/does-not-exist");
            let missing_arg = missing.display().to_string();
            let mut tmux = WakeMockTmux::default();
            let err = wake_run(
                &wake_strings(&["coder-1", "--repo-path", &missing_arg, "--no-attach"]),
                &mut tmux,
            )
            .expect_err("missing repo path must fail");
            assert!(err.contains("wake: repo path missing"), "{err}");
            assert!(err.contains(&missing_arg), "{err}");
            assert!(tmux.actions.is_empty(), "{:?}", tmux.actions);
        });
    }

    #[test]
    fn wake_reuses_registry_session_name_after_reboot() {
        wake_with_fixture(|root| {
            let session = "99-mother";
            let repo = root.join("ghq/github.com/laris-co/mother-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join(format!("{session}.json")),
                r#"{"name":"99-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with(&format!("new-session {session}"))), "{stdout}");
            assert!(stdout.contains(&format!("created session '{session}'")));
        });
    }

    #[test]
    fn wake_full_numeric_registry_name_resolves_via_typed_resolver() {
        wake_with_fixture(|root| {
            let session = "41-arra-oracle-v3";
            let repo = root.join("ghq/github.com/laris-co/arra-oracle-v3");
            std::fs::create_dir_all(&repo).expect("repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"41-arra-oracle-v3","windows":[{"name":"arra-oracle-v3","repo":"github.com/laris-co/arra-oracle-v3"}]}"#,
            )
            .expect("write registry");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&[session, "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains(&format!("created session '{session}'")), "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with(&format!("new-session {session} arra-oracle-v3"))), "{tmux:?}");
            assert!(tmux.actions.iter().any(|action| action.contains(&repo.display().to_string())), "{tmux:?}");
        });
    }

    #[test]
    fn wake_exact_session_name_with_multiple_windows_is_not_ambiguous() {
        wake_with_fixture(|root| {
            let session = "41-arra-oracle-v3";
            let main_repo = root.join("ghq/github.com/laris-co/arra-oracle-v3");
            let task_repo = root.join("ghq/github.com/laris-co/arra-oracle-v3-task");
            std::fs::create_dir_all(&main_repo).expect("main repo");
            std::fs::create_dir_all(&task_repo).expect("task repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"41-arra-oracle-v3","windows":[{"name":"arra-oracle-v3","repo":"github.com/laris-co/arra-oracle-v3"},{"name":"arra-oracle-v3-task","repo":"github.com/laris-co/arra-oracle-v3-task"}]}"#,
            )
            .expect("write registry");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&[session, "--dry-run"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("found"), "{stdout}");
            assert!(stdout.contains("arra-oracle-v3"), "{stdout}");
            assert!(stdout.contains(&main_repo.display().to_string()), "{stdout}");
            assert!(stdout.contains(&format!("would wake window 'arra-oracle-v3' in session '{session}'")), "{stdout}");
            assert!(!stdout.contains("ambiguous registry target"), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_names_and_reuses_the_matched_sibling_not_the_generic_oracle() {
        // #711: sibling windows on one repo all derive the same oracle
        // ("rpro-ent"), so resolving to the RIGHT candidate isn't enough --
        // if the final window name collapses back to that shared oracle,
        // wake silently wakes the wrong thing. Two things are checked
        // together on purpose (not two separate tests): the naming bug and
        // wake_create_or_reuse_window's live-window comparison are on the
        // same path, and testing them apart could pass with the live path
        // still broken.
        wake_with_fixture(|root| {
            let session = "05-rpro-ent";
            let repo = root.join("ghq/github.com/switchaphon/rpro-ent-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"05-rpro-ent","windows":[{"name":"rpro-ent-oracle","repo":"switchaphon/rpro-ent-oracle"},{"name":"rpro-ent-codex-1","repo":"switchaphon/rpro-ent-oracle"}]}"#,
            )
            .expect("write registry");

            // dry-run: naming the sibling by its own name must produce a
            // plan for THAT sibling, not the shared, generic oracle name.
            let mut dry_tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["rpro-ent-codex-1", "--dry-run"]), &mut dry_tmux).expect("dry run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("would wake window 'rpro-ent-codex-1' in session '05-rpro-ent'"), "{stdout}");
            assert!(!stdout.contains("'rpro-ent'"), "collapsed to the generic oracle name: {stdout}");

            // live: the sibling already exists as its own tmux window --
            // must be reused, never re-created under the generic name.
            let mut tmux = WakeMockTmux {
                sessions: vec![TmuxSession {
                    name: session.to_owned(),
                    windows: vec![
                        maw_tmux::TmuxWindow { index: 0, name: "rpro-ent-oracle".to_owned(), active: true, cwd: None },
                        maw_tmux::TmuxWindow { index: 1, name: "rpro-ent-codex-1".to_owned(), active: false, cwd: None },
                    ],
                }],
                ..WakeMockTmux::default()
            };
            let (code, stdout) = wake_run(&wake_strings(&["rpro-ent-codex-1", "--no-attach"]), &mut tmux).expect("apply");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("rpro-ent-codex-1"), "{stdout}");
            assert!(
                !tmux.actions.iter().any(|action| action.starts_with("new-window")),
                "reused window should not be re-created: {:?}",
                tmux.actions
            );
        });
    }

    fn rpro_ent_registry_fixture(root: &std::path::Path) {
        std::fs::create_dir_all(root.join("ghq/github.com/switchaphon/rpro-ent-oracle")).expect("repo");
        std::fs::write(
            root.join("config/fleet/05-rpro-ent.json"),
            r#"{"name":"05-rpro-ent","windows":[{"name":"rpro-ent-oracle","repo":"switchaphon/rpro-ent-oracle"},{"name":"rpro-ent-codex-1","repo":"switchaphon/rpro-ent-oracle"}]}"#,
        )
        .expect("write registry");
    }

    fn rpro_ent_live_session(live_window: &str) -> WakeMockTmux {
        WakeMockTmux {
            sessions: vec![TmuxSession {
                name: "05-rpro-ent".to_owned(),
                windows: vec![maw_tmux::TmuxWindow {
                    index: 0,
                    name: live_window.to_owned(),
                    active: true,
                    cwd: None,
                }],
            }],
            ..WakeMockTmux::default()
        }
    }

    #[test]
    fn wake_prefers_the_one_live_sibling_when_the_shared_alias_ties() {
        // #711 part 2, Nat's chosen policy: when an alias genuinely ties
        // multiple windows and exactly one of them is live, prefer it --
        // never guess among several live candidates, never guess among
        // several sleeping ones. "rpro-ent" (the oracle both siblings
        // derive from their shared repo) used to always report ambiguous
        // here regardless of live state, because wake_typed_registry_
        // candidates tagged every window SleepingRegistry unconditionally --
        // maw-matcher's live_tiebreak (#719) had no live candidate to ever
        // prefer. Threading `sessions` through lets it see the real one.
        wake_with_fixture(|root| {
            rpro_ent_registry_fixture(root);
            let mut tmux = rpro_ent_live_session("rpro-ent-codex-1");
            let (code, stdout) = wake_run(&wake_strings(&["rpro-ent", "--dry-run"]), &mut tmux).expect("resolves");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("would wake window 'rpro-ent-codex-1' in session '05-rpro-ent'"), "{stdout}");
        });
    }

    #[test]
    fn wake_still_refuses_to_guess_when_multiple_siblings_are_live() {
        // The other half of the same policy: liveness only disambiguates
        // when it picks out exactly one candidate. Two live siblings sharing
        // the tied alias must still error -- silently picking one of two
        // equally-live windows is exactly the "succeeds at the wrong thing"
        // shape #711's whole family was about.
        wake_with_fixture(|root| {
            rpro_ent_registry_fixture(root);
            let mut tmux = WakeMockTmux {
                sessions: vec![TmuxSession {
                    name: "05-rpro-ent".to_owned(),
                    windows: vec![
                        maw_tmux::TmuxWindow { index: 0, name: "rpro-ent-oracle".to_owned(), active: true, cwd: None },
                        maw_tmux::TmuxWindow { index: 1, name: "rpro-ent-codex-1".to_owned(), active: false, cwd: None },
                    ],
                }],
                ..WakeMockTmux::default()
            };
            let error = wake_run(&wake_strings(&["rpro-ent", "--dry-run"]), &mut tmux).expect_err("still ambiguous");
            assert!(error.contains("ambiguous registry target for rpro-ent"), "{error}");
        });
    }

    #[test]
    fn wake_shared_derived_oracle_stays_ambiguous_same_as_before_the_identity_split() {
        // The matched_window split changes what a *resolved* candidate is
        // named -- it must not change *whether* a query resolves at all.
        // "rpro-ent" is the oracle both siblings derive from their shared
        // repo, so it still hits the same two-way tie in resolve_typed_target
        // before wake ever reaches window naming. Confirmed byte-for-byte
        // against a checkout of this file predating the split: identical
        // error text, not just "still an error" -- checked, not assumed
        // (the #703 lesson: an unverified "X is unaffected" is how that
        // regression got approved and shipped).
        wake_with_fixture(|root| {
            let session = "05-rpro-ent";
            let repo = root.join("ghq/github.com/switchaphon/rpro-ent-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"05-rpro-ent","windows":[{"name":"rpro-ent-oracle","repo":"switchaphon/rpro-ent-oracle"},{"name":"rpro-ent-codex-1","repo":"switchaphon/rpro-ent-oracle"}]}"#,
            )
            .expect("write registry");
            let mut tmux = WakeMockTmux::default();
            let error = wake_run(&wake_strings(&["rpro-ent", "--dry-run"]), &mut tmux)
                .expect_err("shared oracle across two real siblings must stay ambiguous");
            assert_eq!(
                error,
                "wake: ambiguous registry target for rpro-ent: 05-rpro-ent:rpro-ent-oracle, 05-rpro-ent:rpro-ent-codex-1"
            );
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_accepts_the_session_window_syntax_its_own_ambiguous_error_prints() {
        // #711 fix 5: wake's own ambiguous-registry-target error prints
        // candidates as "session:window" (see the assertion above) but
        // rejected that exact syntax as input -- "wake: invalid oracle" --
        // because wake_oracle's degenerate fallback identity choked on the
        // colon before the typed resolver, which already handles this shape
        // via an exact name match, ever got a chance to run.
        wake_with_fixture(|root| {
            let session = "05-rpro-ent";
            let repo = root.join("ghq/github.com/switchaphon/rpro-ent-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"05-rpro-ent","windows":[{"name":"rpro-ent-oracle","repo":"switchaphon/rpro-ent-oracle"},{"name":"rpro-ent-codex-1","repo":"switchaphon/rpro-ent-oracle"}]}"#,
            )
            .expect("write registry");
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) =
                wake_run(&wake_strings(&["05-rpro-ent:rpro-ent-codex-1", "--dry-run"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("would wake window 'rpro-ent-codex-1' in session '05-rpro-ent'"), "{stdout}");
            assert!(!stdout.contains("'rpro-ent'"), "collapsed to the generic oracle name: {stdout}");
        });
    }

    #[test]
    fn wake_rejects_node_qualified_targets_even_when_the_last_segment_is_a_real_local_repo() {
        // m5's review of #716: `hey`/`ls -v` print and pass around
        // node:session:window strings, and people copy those into other
        // commands. Before this guard, stripping to the LAST colon segment
        // for the genuine "session:window" case ALSO stripped a
        // node-qualified target down to its bare window name -- and if that
        // last segment happens to name a real local repo (independent of
        // the actual node/session the caller meant), it resolved locally
        // and silently discarded the node the caller explicitly named. Same
        // family as #715: succeeding at the wrong thing, not erroring.
        // `black:33-maw-rs:maw-rs` reproduces that exactly: "maw-rs" is a
        // real local repo here, but the caller asked for node "black".
        wake_with_fixture(|root| {
            std::fs::create_dir_all(root.join("ghq/github.com/acme/maw-rs")).expect("real local repo");
            let mut tmux = WakeMockTmux::default();
            let error = wake_run(&wake_strings(&["black:33-maw-rs:maw-rs", "--dry-run"]), &mut tmux)
                .expect_err("node-qualified target must not resolve locally");
            assert_eq!(error, "wake: invalid oracle");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_unknown_name_reports_not_found_without_tmux_mutation() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&["does-not-exist", "--no-attach"]), &mut tmux).expect_err("not found");
            assert!(err.contains("wake: repo not found for does-not-exist"), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_typo_near_miss_reports_did_you_mean_and_next_steps() {
        wake_with_fixture(|root| {
            std::fs::create_dir_all(root.join("ghq/github.com/acme/mascot-oracle")).expect("repo");
            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&["mascott", "--no-attach"]), &mut tmux).expect_err("not found");
            assert!(err.contains("wake: repo not found for mascott"), "{err}");
            assert!(err.contains("Did you mean"), "{err}");
            assert!(err.contains("mascot"), "{err}");
            assert!(err.contains("maw oracle scan"), "{err}");
            assert!(err.contains("maw ls -a"), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_exact_fleet_squad_uses_universal_non_tty_picker() {
        wake_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/01-3e.json"),
                r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"},{"handle":"drift"}]}"#,
            )
            .expect("squad registry");

            let output = run_wake_command(&wake_strings(&["3e"]));

            assert_eq!(output.code, 1, "{}{}", output.stdout, output.stderr);
            assert!(output.stdout.contains("fleet squad 3e (2 members)"), "{}", output.stdout);
            assert!(output.stdout.contains("maw fleet wake 3e"), "{}", output.stdout);
            assert!(output.stderr.is_empty(), "{}", output.stderr);
        });
    }

    #[test]
    fn wake_fleet_squad_yes_and_dry_run_execute_in_process_bridge() {
        wake_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/01-3e.json"),
                r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"}]}"#,
            )
            .expect("squad registry");
            let mut tmux = WakeMockTmux::default();
            let mut calls = Vec::<Vec<String>>::new();
            let mut fleet_wake = |args: &[String]| {
                calls.push(args.to_vec());
                CliOutput { code: 0, stdout: "fleet bridge\n".to_owned(), stderr: String::new() }
            };

            let yes = run_wake_command_with(&wake_strings(&["3e", "--yes"]), &mut tmux, &mut fleet_wake);
            let dry_run = run_wake_command_with(&wake_strings(&["3e", "--dry-run"]), &mut tmux, &mut fleet_wake);

            assert_eq!(yes.code, 0, "{}{}", yes.stdout, yes.stderr);
            assert_eq!(dry_run.code, 0, "{}{}", dry_run.stdout, dry_run.stderr);
            assert_eq!(calls, vec![wake_strings(&["wake", "3e"]), wake_strings(&["wake", "3e", "--dry-run"])]);
            assert!(tmux.actions.is_empty(), "{:?}", tmux.actions);
        });
    }

    #[test]
    fn wake_near_fleet_squad_uses_same_picker_without_auto_action() {
        wake_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/01-3e.json"),
                r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"}]}"#,
            )
            .expect("squad registry");
            let mut tmux = WakeMockTmux::default();
            let mut called = false;
            let mut fleet_wake = |_: &[String]| {
                called = true;
                CliOutput { code: 0, stdout: String::new(), stderr: String::new() }
            };

            let output = run_wake_command_with(&wake_strings(&["3f"]), &mut tmux, &mut fleet_wake);

            assert_eq!(output.code, 1, "{}{}", output.stdout, output.stderr);
            assert!(output.stdout.contains("fleet squad 3e"), "{}", output.stdout);
            assert!(output.stdout.contains("maw fleet wake 3e"), "{}", output.stdout);
            assert!(!called);
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_revived_session_reregisters_into_its_own_registry_entry() {
        // #312 revive + #299 upsert guard interaction: the entry that named
        // the revived session lives in the config fleet dir, not the default
        // ~/.maw/fleet write dir. Re-registration after the wake must update
        // that entry in place instead of minting a duplicate file.
        wake_with_fixture(|root| {
            let session = "99-mother";
            let repo = root.join("ghq/github.com/laris-co/mother-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let entry = root.join("config/fleet").join(format!("{session}.json"));
            std::fs::write(
                &entry,
                r#"{"name":"99-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains(&format!("created session '{session}'")));
            assert!(!root.join("home/.maw/fleet").join(format!("{session}.json")).exists(), "duplicate entry minted: {stdout}");
            let value = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&entry).expect("entry")).expect("json");
            assert_eq!(value["name"], "99-mother");
            assert_eq!(value["created_by"], "maw wake");
        });
    }

    #[test]
    fn wake_session_name_avoids_slot_collision_with_live_session() {
        wake_with_fixture(|root| {
            let oracle = "turso";
            let _ = std::fs::create_dir_all(root.join("ghq/github.com/acme/turso-oracle"));
            let occupied_slot = wake_slot(oracle);
            let mut tmux = WakeMockTmux {
                sessions: vec![TmuxSession {
                    name: format!("{occupied_slot:02}-esp32"),
                    windows: vec![maw_tmux::TmuxWindow { index: 0, name: "esp32".to_owned(), active: true, cwd: None }],
                }],
                ..WakeMockTmux::default()
            };
            let (code, stdout) = wake_run(&wake_strings(&[oracle, "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with("new-session")));
            assert!(
                !tmux.actions.iter().any(|action| action.starts_with(&format!("new-session {occupied_slot:02}-{oracle}"))),
                "{stdout}"
            );
        });
    }

    #[test]
    fn wake_repo_not_found_reports_registry_gap() {
        wake_with_fixture(|root| {
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join("88-mother.json"),
                r#"{"name":"88-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");

            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect_err("not found");
            assert!(err.contains("registry entry for 88-mother exists"), "{err}");
            assert!(err.contains("not cloned under"), "{err}");
            assert!(err.contains("github.com/laris-co/mother-oracle"), "{err}");
            assert!(
                err.contains("maw work https://github.com/laris-co/mother-oracle"),
                "{err}"
            );
            assert!(err.contains("probed"), "{err}");
            assert!(err.contains(&wake_ghq_root().display().to_string()), "{err}");
            assert!(err.contains(&root.join("ghq/github.com/laris-co/mother-oracle").display().to_string()), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_attach_live_registry_session_skips_missing_repo_probe() {
        wake_with_fixture(|root| {
            let session = "88-mother";
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join(format!("{session}.json")),
                r#"{"name":"88-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");
            let mut tmux = wake_mock_tmux_with_existing_window(session, "mother");
            let mut fleet_wake = |_: &[String]| panic!("fleet wake must not run");

            let output = run_wake_command_with(
                &wake_strings(&[session, "--attach", "--session", session]),
                &mut tmux,
                &mut fleet_wake,
            );

            assert_eq!(output.code, 0, "{}", output.stderr);
            assert_eq!(tmux.actions, vec!["select 88-mother:mother"]);
        });
    }

    #[test]
    fn wake_cloned_registry_repo_still_wakes_normally() {
        wake_with_fixture(|root| {
            let session = "88-mother";
            let repo = root.join("ghq/github.com/laris-co/mother-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join(format!("{session}.json")),
                r#"{"name":"88-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");
            let mut tmux = WakeMockTmux::default();

            let (code, stdout) =
                wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect("wake");

            assert_eq!(code, 0, "{stdout}");
            assert!(tmux.actions.iter().any(|action| {
                action.starts_with("new-session 88-mother mother ")
                    && action.contains(&repo.display().to_string())
            }));
        });
    }

    #[test]
    fn wake_registry_wrong_org_uses_disk_basename_match() {
        wake_with_fixture(|root| {
            let session = "88-mother";
            let repo = root.join("ghq/github.com/laris-co/mother-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join(format!("{session}.json")),
                r#"{"name":"88-mother","windows":[{"name":"mother","repo":"github.com/Soul-Brews-Studio/mother-oracle"}]}"#,
            )
            .expect("write");
            let mut tmux = WakeMockTmux::default();

            let (code, stdout) =
                wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect("wake");

            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("using disk basename match"), "{stdout}");
            assert!(tmux.actions.iter().any(|action| {
                action.starts_with("new-session 88-mother mother ")
                    && action.contains(&repo.display().to_string())
            }));
        });
    }

    #[test]
    fn wake_stale_registry_repo_falls_back_to_oracles_local_path() {
        wake_with_fixture(|root| {
            let canonical = root.join("repos/token-oracle");
            std::fs::create_dir_all(&canonical).expect("canonical repo");
            let canonical = canonical.canonicalize().expect("canonical path");
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join("59-token.json"),
                r#"{"name":"59-token","windows":[{"name":"token","repo":"github.com/Soul-Brews-Studio/token-oracle-oracle"}]}"#,
            )
            .expect("stale registry");
            let cache = serde_json::json!({
                "schema": 1,
                "oracles": [{
                    "org": "laris-co",
                    "repo": "token-oracle",
                    "name": "token",
                    "local_path": canonical,
                    "has_psi": true,
                    "has_fleet_config": true
                }]
            });
            std::fs::write(root.join("home/.maw/oracles.json"), cache.to_string()).expect("oracles cache");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["token", "--dry-run"]), &mut tmux).expect("fallback");

            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains(&format!("registry repo stale, using oracles.json: {}", canonical.display())), "{stdout}");
            assert_eq!(stdout.matches(&canonical.display().to_string()).count(), 2, "{stdout}");
            assert!(!stdout.contains("token-oracle-oracle"), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_full_registry_name_reports_missing_clone_path() {
        wake_with_fixture(|root| {
            let session = "41-arra-oracle-v3";
            let probed = root.join("ghq/github.com/laris-co/arra-oracle-v3");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"41-arra-oracle-v3","windows":[{"name":"arra-oracle-v3","repo":"github.com/laris-co/arra-oracle-v3"}]}"#,
            )
            .expect("write registry");

            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&[session, "--no-attach"]), &mut tmux).expect_err("missing clone");
            assert!(err.contains(&format!("registry entry for {session} exists")), "{err}");
            assert!(err.contains(&format!("probed {}", probed.display())), "{err}");
            assert!(!err.contains("repo not found for"), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_dry_run_is_hermetic_and_matches_golden() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--dry-run", "--task", "issue-134"]), &mut tmux).expect("run");
            assert_eq!(code, 0);
            assert!(stdout.contains("dry-run — no tmux sessions/windows will be changed"));
            assert!(stdout.contains("would wake window 'neo-issue-134'"));
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_apply_uses_seeded_repo_and_mock_tmux_only() {
        wake_with_fixture(|root| {
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0);
            assert!(stdout.contains("created session"));
            assert!(stdout.contains("attach: maw a"));
            assert!(tmux.actions.iter().any(|action| action.starts_with("new-session")));
            assert!(tmux.actions.iter().any(|action| action.contains(&root.join("ghq/github.com/acme/neo-oracle").display().to_string())));
            assert!(!tmux.actions.iter().any(|action| action.starts_with("select")));
        });
    }

    #[test]
    fn wake_fresh_session_waits_for_shell_ready_before_send() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux {
                pre_send_pane_command_script: vec!["direnv".to_owned(), "python3".to_owned(), "zsh".to_owned()],
                pane_command_script: vec!["claude".to_owned()],
                ..WakeMockTmux::default()
            };

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");

            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("created session"), "{stdout}");
            assert_eq!(tmux.pre_send_polls, 3);
            assert_eq!(tmux.post_send_polls, 1);
            assert_eq!(tmux.send_pane_polls, vec![3]);
        });
    }

    #[test]
    fn wake_fresh_session_sends_after_shell_ready_timeout() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux {
                pre_send_pane_command_script: vec!["direnv".to_owned()],
                pane_command_script: vec!["claude".to_owned()],
                ..WakeMockTmux::default()
            };

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");

            let expected = WAKE_LAUNCH_CONFIRM_BACKOFF_MS.len() + 1;
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("created session"), "{stdout}");
            assert_eq!(tmux.pre_send_polls, expected);
            assert_eq!(tmux.post_send_polls, 1);
            assert_eq!(tmux.send_pane_polls, vec![expected]);
        });
    }

    #[test]
    fn wake_reused_shell_window_resends_instead_of_already_running() {
        wake_with_fixture(|_| {
            let session = wake_session_name("neo", &[]);
            let mut tmux = wake_mock_tmux_with_existing_window(&session, "neo");
            tmux.pane_command_script = vec!["zsh".to_owned(), "claude".to_owned()];

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");

            assert_eq!(code, 0, "{stdout}");
            assert!(!stdout.contains('⚡'), "{stdout}");
            assert!(stdout.contains("woke 'neo'"), "{stdout}");
            assert!(!tmux.actions.iter().any(|action| action.starts_with("new-window")), "{:?}", tmux.actions);
            assert!(tmux.actions.iter().any(|action| action.starts_with(&format!("send {session}:neo "))), "{:?}", tmux.actions);
            assert_eq!(tmux.pre_send_polls, 0);
            assert_eq!(tmux.send_pane_polls, vec![1]);
            assert_eq!(tmux.pane_polls, 2);
        });
    }

    #[test]
    fn wake_self_pane_reuse_queues_send_instead_of_already_running() {
        wake_with_fixture(|_| {
            let session = wake_session_name("neo", &[]);
            let mut tmux = wake_mock_tmux_with_existing_window(&session, "neo");
            tmux.target_pane_id = Some("%42".to_owned());
            tmux.pane_command_script = vec!["maw".to_owned()];
            std::env::set_var("TMUX_PANE", "%42");

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");

            assert_eq!(code, 0, "{stdout}");
            assert!(!stdout.contains('⚡'), "{stdout}");
            assert!(stdout.contains("woke 'neo'"), "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with(&format!("send {session}:neo "))), "{:?}", tmux.actions);
            assert_eq!(tmux.pane_polls, 0, "self-pane launcher must not be mistaken for a launched engine");
        });
    }

    #[test]
    fn wake_reused_non_shell_window_keeps_already_running_without_send() {
        wake_with_fixture(|_| {
            let session = wake_session_name("neo", &[]);
            let mut tmux = wake_mock_tmux_with_existing_window(&session, "neo");

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");

            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("running in"), "{stdout}");
            assert_eq!(tmux.pre_send_polls, 0);
            assert!(!tmux.actions.iter().any(|action| action.starts_with("send ")), "{:?}", tmux.actions);
            assert_eq!(tmux.pane_polls, 1);
        });
    }

    #[test]
    fn wake_reused_unreadable_window_keeps_already_running_without_send() {
        wake_with_fixture(|_| {
            let session = wake_session_name("neo", &[]);
            let mut tmux = wake_mock_tmux_with_existing_window(&session, "neo");
            tmux.pane_command_error = true;

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");

            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("running in"), "{stdout}");
            assert_eq!(tmux.pre_send_polls, 0);
            assert!(!tmux.actions.iter().any(|action| action.starts_with("send ")), "{:?}", tmux.actions);
            assert_eq!(tmux.pane_polls, 1);
        });
    }

    #[test]
    fn wake_attach_selects_before_post_attach_work_and_audits_phases() {
        wake_with_fixture(|root| {
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert_eq!(tmux.actions[0].split_whitespace().next(), Some("new-session"));
            assert_eq!(tmux.actions[1].split_whitespace().next(), Some("send-detached"));
            assert_eq!(tmux.actions[2].split_whitespace().next(), Some("select"));
            let audit = std::fs::read_to_string(root.join("state/audit.jsonl")).expect("audit");
            assert!(audit.contains(r#""event":"wake.phase""#), "{audit}");
            assert!(audit.contains(r#""phase":"first-window""#), "{audit}");
            let first = audit.find(r#""phase":"first-window""#).expect("first-window phase");
            let attach = audit.find(r#""phase":"attach""#).expect("attach phase");
            let fleet = audit.find(r#""phase":"fleet-upsert""#).expect("fleet phase");
            assert!(first < attach && attach < fleet, "{audit}");
            assert!(audit.contains(r#""phase":"fleet-upsert""#), "{audit}");
        });
    }

    #[test]
    fn wake_fast_attach_failure_waits_for_detached_engine_send() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux {
                fail_select: true,
                detached_delay_ms: 50,
                ..WakeMockTmux::default()
            };

            let error = wake_run(&wake_strings(&["neo", "--attach"]), &mut tmux).expect_err("attach failure");

            assert!(error.contains("mock attach failed"), "{error}");
            assert!(tmux.detached_finished.load(std::sync::atomic::Ordering::SeqCst));
        });
    }

    #[test]
    fn wake_auto_registers_fleet_json_and_merges_new_windows() {
        wake_with_fixture(|root| {
            let _now = EnvVarRestore::capture("MAW_RS_FLEET_REGISTRY_NOW");
            std::env::set_var("MAW_RS_FLEET_REGISTRY_NOW", "2026-07-03T02:03:04.000Z");
            let mut tmux = WakeMockTmux::default();

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("first wake");
            assert_eq!(code, 0, "{stdout}");
            let session = tmux.sessions.first().expect("session").name.clone();
            let path = root.join("home/.maw/fleet").join(format!("{session}.json"));
            let first: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("registry")).expect("json");
            assert_eq!(first["name"], session);
            assert_eq!(first["created_at"], "2026-07-03T02:03:04.000Z");
            assert_eq!(first["created_by"], "maw wake");
            assert_eq!(first["auto_registered"], true);
            assert_eq!(first["windows"].as_array().expect("windows").len(), 1);
            assert_eq!(first["windows"][0]["name"], "neo");
            assert_eq!(first["windows"][0]["repo"], "acme/neo-oracle");
            assert_eq!(first["windows"][0]["kind"], "project");

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--task", "issue-90", "--no-attach"]), &mut tmux).expect("task wake");
            assert_eq!(code, 0, "{stdout}");
            let updated: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("updated registry")).expect("json");
            let windows = updated["windows"].as_array().expect("windows");
            assert_eq!(windows.len(), 2);
            assert!(windows.iter().any(|window| window["name"] == "neo"));
            assert!(windows.iter().any(|window| window["name"] == "neo-issue-90"));
            assert!(windows.iter().all(|window| window["kind"] == "project"));
            assert_eq!(updated["created_at"], "2026-07-03T02:03:04.000Z");
        });
    }

    #[test]
    fn wake_list_reads_mock_sessions_without_real_tmux() {
        let mut tmux = WakeMockTmux { sessions: vec![TmuxSession { name: "12-neo".to_owned(), windows: vec![maw_tmux::TmuxWindow { index: 0, name: "neo".to_owned(), active: true, cwd: None }] }], ..WakeMockTmux::default() };
        let (code, stdout) = wake_run(&wake_strings(&["neo", "--list"]), &mut tmux).expect("run");
        assert_eq!(code, 0);
        assert!(stdout.contains("12-neo (1 windows)"));
        assert!(tmux.actions.is_empty());
    }
}
