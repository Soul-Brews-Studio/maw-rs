// The fleet suite, kept whole.
//
// Moved out of fleet.rs so the module reads as the thing it implements rather than
// the thing plus its scaffolding. Not split further: these tests share one
// fixture harness, and a private item in one `mod` is invisible to a sibling,
// so carving them up would mean exporting the harness for no real gain.

#[cfg(test)]
mod fleet_tests {
    use super::*;

    fn fleet_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn fleet_parse_agents_skips_invalid_manifest_names() {
        let value = serde_json::json!({"agents": {
            "--help": "m5",
            ".bak202605130508discord-oracle": "m5",
            "a b": "m5",
            "digger": "m5:window",
            "nova": {"node": "edge"}
        }});
        let agents = fleet_parse_agents(&value);
        assert_eq!(agents.len(), 2, "{agents:?}");
        assert_eq!(agents.get("digger"), Some(&"m5".to_owned()));
        assert_eq!(agents.get("nova"), Some(&"edge".to_owned()));
    }

    #[derive(Default)]
    struct FleetMockTmux {
        sessions: String,
    }

    impl maw_tmux::TmuxRunner for FleetMockTmux {
        fn run(&mut self, subcommand: &str, _args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            if subcommand == "list-sessions" {
                Ok(self.sessions.clone())
            } else {
                Err(maw_tmux::TmuxError::new(format!("unexpected tmux command {subcommand}")))
            }
        }
    }

    #[derive(Default)]
    struct FleetFakeRuntime {
        ghq_root: Option<String>,
        commands: Vec<(String, Vec<String>)>,
        sessions: Vec<TmuxSession>,
    }

    impl FleetRuntime for FleetFakeRuntime {
        fn fleet_run_command(&mut self, program: &str, args: &[String]) -> Result<String, String> {
            self.commands.push((program.to_owned(), args.to_vec()));
            if program == "ghq" && args == ["root".to_owned()] {
                self.ghq_root.clone().ok_or_else(|| "fake ghq root failed".to_owned())
            } else if program == "tmux" && args.first().is_some_and(|arg| arg == "rename-session") {
                Ok(String::new())
            } else {
                Err(format!("unexpected command {program} {args:?}"))
            }
        }

        fn fleet_list_all(&mut self) -> Vec<TmuxSession> {
            self.sessions.clone()
        }
    }

    fn fleet_live_session(name: &str, windows: &[&str]) -> TmuxSession {
        TmuxSession {
            name: name.to_owned(),
            windows: windows
                .iter()
                .enumerate()
                .map(|(index, window)| maw_tmux::TmuxWindow {
                    index: u32::try_from(index).expect("window index"),
                    name: (*window).to_owned(),
                    active: index == 0,
                    cwd: None,
                })
                .collect(),
        }
    }

    fn fleet_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-fleet-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn fleet_fixture() -> std::path::PathBuf {
        let root = fleet_temp_root("fixture");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::fs::create_dir_all(root.join("ghq/github.com/acme/maw-rs")).expect("repo");
        std::fs::write(root.join("config/maw.config.json"), fleet_config_json()).expect("config");
        std::fs::write(root.join("config/fleet/03-alpha.json"), fleet_session_json()).expect("session");
        std::fs::write(root.join("config/fleet/22-dormant.disabled"), "{}\n").expect("disabled");
        root
    }

    fn fleet_config_json() -> &'static str {
        r#"{"node":"alpha","namedPeers":[{"name":"beta","url":"http://127.0.0.1:4111"}],"agents":{"nova":"alpha:nova","wish":{"node":"beta"}}}"#
    }

    fn fleet_session_json() -> &'static str {
        r#"{"name":"03-alpha","windows":[{"name":"maw","repo":"acme/maw-rs"},{"name":"ghost","repo":"acme/missing"}]}"#
    }

    fn fleet_with_fixture<F>(test: F)
    where
        F: FnOnce(&std::path::Path),
    {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _tmux = EnvVarRestore::capture("TMUX");
        let root = fleet_fixture();
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        std::env::remove_var("TMUX");
        test(&root);
    }

    #[test]
    fn fleet_parse_flags_and_guard_option_injection() {
        let parsed = fleet_parse_args(&fleet_strings(&["wake", "--json", "--dry-run", "--all", "--kill", "--resume"])).expect("parse");
        assert_eq!(parsed.command, FleetCommand::Wake);
        assert!(parsed.json && parsed.dry_run && parsed.all && parsed.kill && parsed.resume);
        let renumber = fleet_parse_args(&fleet_strings(&["renumber", "--include-99", "--dry-run"])).expect("renumber parse");
        assert_eq!(renumber.command, FleetCommand::Renumber);
        assert!(renumber.include_99 && renumber.dry_run);
        let only_99 = fleet_parse_args(&fleet_strings(&["renumber", "--only-99", "--dry-run"])).expect("only 99 parse");
        assert!(only_99.only_99 && only_99.dry_run);
        assert!(fleet_parse_args(&fleet_strings(&["--", "wake"])).expect_err("separator guard").contains("unknown argument"));
        assert!(fleet_parse_args(&fleet_strings(&["-oProxyCommand=bad"])).expect_err("leading dash").contains("unknown argument"));
        let scoped = fleet_parse_args(&fleet_strings(&["wake", "3e"])).expect("group target");
        assert_eq!((scoped.command, scoped.target.as_deref()), (FleetCommand::Wake, Some("3e")));
        let groups = fleet_parse_args(&fleet_strings(&["ls", "--squads", "3e,drift"])).expect("squad filter");
        assert_eq!(groups.squads, vec!["3e".to_owned(), "drift".to_owned()]);
        let alias = fleet_parse_args(&fleet_strings(&["wake-all"])).expect("alias");
        assert!(alias.all, "wake-all implies --all");
        let bare = fleet_parse_args(&fleet_strings(&["wake"])).expect_err("bare wake");
        assert!(bare.contains("specify a squad, or --all to wake every registered session on this node"), "{bare}");
        let sleep = fleet_parse_args(&fleet_strings(&["sleep", "--json"])).expect_err("bare sleep");
        assert!(sleep.contains("fleet sleep: specify a squad"), "{sleep}");
    }

    #[test]
    fn fleet_census_is_hermetic_and_golden() {
        fleet_with_fixture(|_| {
            let output = run_fleet_command(&fleet_strings(&["ls"]));
            assert_eq!(output.code, 0);
            assert!(output.stderr.is_empty());
            assert_eq!(
                output.stdout,
                "\u{1b}[36mfleet\u{1b}[0m node alpha\n  sessions: 1 (2 windows, 1 disabled)\n  peers: 1\n  agents: 2\n  session list:\n  - 03-alpha (2 windows)\n  squads: 0\n"
            );
        });
    }

    #[test]
    fn fleet_census_lists_squads_and_filters_membership() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let unfiltered = run_fleet_command(&fleet_strings(&["ls", "--json"]));
            assert_eq!(unfiltered.code, 0, "{}", unfiltered.stderr);
            let raw: serde_json::Value = serde_json::from_str(&unfiltered.stdout).expect("json");
            assert_eq!(raw["squads"].as_array().expect("squads").len(), 1);
            assert_eq!(raw["squads"][0]["name"], serde_json::json!("3e"));
            assert_eq!(raw["sessionCount"], 1); // rosters are excluded from sessions
            assert_eq!(raw["sessions"][0]["name"], serde_json::json!("03-alpha"));

            let filtered = run_fleet_command(&fleet_strings(&["ls", "--squads", "3e", "--json"]));
            assert_eq!(filtered.code, 0, "{}", filtered.stderr);
            let filtered_json: serde_json::Value = serde_json::from_str(&filtered.stdout).expect("json");
            assert_eq!(filtered_json["squads"][0]["name"], serde_json::json!("3e"));
            assert_eq!(filtered_json["sessionCount"], 1);
            assert_eq!(filtered_json["sessions"][0]["name"], serde_json::json!("03-alpha"));
            let muted = run_fleet_command(&fleet_strings(&["ls", "--squads", "nope", "--json"]));
            assert_eq!(muted.code, 0, "{}", muted.stderr);
            let muted_json: serde_json::Value = serde_json::from_str(&muted.stdout).expect("json");
            assert_eq!(muted_json["sessionCount"], 0);
            assert_eq!(muted_json["sessions"], serde_json::json!([]));
            assert_eq!(muted_json["squads"], serde_json::json!([]));
        });
    }

    #[test]
    fn fleet_doctor_json_reports_seeded_missing_repo_only() {
        fleet_with_fixture(|root| {
            let output = run_fleet_command(&fleet_strings(&["doctor", "--json"]));
            assert_eq!(output.code, 1);
            assert!(output.stderr.is_empty());
            assert!(output.stdout.contains("\"node\": \"alpha\""));
            assert!(output.stdout.contains("\"code\": \"missing-repo\""));
            assert!(output.stdout.contains(&root.join("ghq/github.com/acme/missing").display().to_string()));
        });
    }

    #[test]
    fn fleet_doctor_flags_oracle_names_that_resolve_to_multiple_registry_windows() {
        // #711: two windows sharing one repo, neither literally named the
        // derived oracle -- so unlike the issue's original repro (fixed by
        // #665's literal_name_tiebreak, a window named exactly "rpro-ent-
        // oracle" wins the tie), there is no name match to break the tie.
        // `maw wake twin` would be permanently ambiguous; doctor must say so.
        fleet_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/09-twin.json"),
                r#"{"name":"09-twin","windows":[{"name":"twin-codex-1","repo":"acme/twin-oracle"},{"name":"twin-codex-2","repo":"acme/twin-oracle"}]}"#,
            )
            .expect("twin registry");
            let mut runtime = FleetFakeRuntime::default();
            let (_, dry_run) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("dry run");
            let dry_json: serde_json::Value = serde_json::from_str(&dry_run).expect("dry json");
            let ambiguous = dry_json["findings"]
                .as_array()
                .expect("findings")
                .iter()
                .filter(|finding| finding["code"] == "ambiguous-oracle")
                .collect::<Vec<_>>();
            assert_eq!(ambiguous.len(), 1, "{dry_json}");
            assert_eq!(ambiguous[0]["subject"], "twin");
            let detail = ambiguous[0]["detail"].as_str().expect("detail");
            assert!(detail.contains("twin-codex-1"), "{detail}");
            assert!(detail.contains("twin-codex-2"), "{detail}");
        });
    }

    #[test]
    fn fleet_doctor_detects_and_fixes_aliases_for_the_same_live_window() {
        fleet_with_fixture(|root| {
            let fleet_dir = root.join("config/fleet");
            for (file, session) in [("04-agora.json", "04-agora"), ("05-bud.json", "05-bud")] {
                std::fs::write(
                    fleet_dir.join(file),
                    format!(
                        r#"{{"name":"{session}","windows":[{{"name":"{session}-oracle","repo":"github.com/acme/maw-rs","legacy":true}},{{"name":"{session}","repo":"acme/maw-rs","kind":"project","preferred":true}}]}}"#,
                    ),
                )
                .expect("duplicate registry");
            }
            let mut runtime = FleetFakeRuntime {
                sessions: vec![
                    fleet_live_session("04-agora", &["04-agora"]),
                    fleet_live_session("05-bud", &["05-bud"]),
                ],
                ..Default::default()
            };

            let (_, dry_run) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("dry run");
            let dry_json: serde_json::Value = serde_json::from_str(&dry_run).expect("dry json");
            assert_eq!(
                dry_json["findings"]
                    .as_array()
                    .expect("findings")
                    .iter()
                    .filter(|finding| finding["code"] == "duplicate-window-repo")
                    .count(),
                2
            );
            let unchanged: serde_json::Value = serde_json::from_str(
                &std::fs::read_to_string(fleet_dir.join("04-agora.json")).expect("dry-run registry"),
            )
            .expect("dry-run json");
            assert_eq!(unchanged["windows"].as_array().expect("windows").len(), 2);

            let (_, fixed) = fleet_run_with(&fleet_strings(&["doctor", "--fix", "--json"]), &mut runtime).expect("fix");
            let fixed_json: serde_json::Value = serde_json::from_str(&fixed).expect("fixed json");
            assert_eq!(fixed_json["repairs"].as_array().expect("repairs").len(), 2);
            for file in ["04-agora.json", "05-bud.json"] {
                let registry: serde_json::Value = serde_json::from_str(
                    &std::fs::read_to_string(fleet_dir.join(file)).expect("fixed registry"),
                )
                .expect("fixed registry json");
                let windows = registry["windows"].as_array().expect("windows");
                assert_eq!(windows.len(), 1);
                assert_eq!(windows[0]["kind"], "project");
                assert_eq!(windows[0]["preferred"], true);
                assert!(windows[0].get("legacy").is_none());
            }
        });
    }

    #[test]
    fn fleet_doctor_preserves_distinct_live_windows_sharing_one_repo() {
        fleet_with_fixture(|root| {
            std::fs::create_dir_all(root.join("ghq/github.com/acme/missing")).expect("seed missing repo");
            let path = root.join("config/fleet/41-team.json");
            std::fs::write(
                &path,
                r#"{"name":"41-team","windows":[{"name":"coder-one","repo":"acme/maw-rs"},{"name":"coder-two","repo":"github.com/acme/maw-rs"},{"name":"coder-three","repo":"acme/maw-rs"}]}"#,
            )
            .expect("team registry");
            let mut runtime = FleetFakeRuntime {
                sessions: vec![fleet_live_session("41-team", &["coder-one", "coder-two", "coder-three"])],
                ..Default::default()
            };

            let (_, dry_run) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("dry run");
            let dry_json: serde_json::Value = serde_json::from_str(&dry_run).expect("dry json");
            let findings = dry_json["findings"].as_array().expect("findings");
            // The original alias-based dedup check must stay silent: these
            // are genuinely distinct windows, not the same window under two
            // names. It's a *different* claim from "wake maw-rs resolves" --
            // none of the three is literally named maw-rs, so that oracle
            // identity IS genuinely ambiguous (#711) and doctor should say so.
            assert!(!findings.iter().any(|finding| finding["code"] == "duplicate-window-repo"), "{dry_json}");
            assert!(
                findings.iter().any(|finding| finding["code"] == "ambiguous-oracle" && finding["subject"] == "maw-rs"),
                "{dry_json}"
            );

            let (_, fixed) = fleet_run_with(&fleet_strings(&["doctor", "--fix", "--json"]), &mut runtime).expect("fix");
            let fixed_json: serde_json::Value = serde_json::from_str(&fixed).expect("fixed json");
            assert_eq!(fixed_json["repairs"], serde_json::json!([]));
            let registry: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("registry")).expect("json");
            assert_eq!(registry["windows"].as_array().expect("windows").len(), 3);
        });
    }

    #[test]
    fn fleet_doctor_uses_ghq_root_once_for_host_prefixed_repo_slugs() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = fleet_temp_root("doctor-ghq-root");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet dir");
        std::fs::write(
            root.join("config/fleet/188-maw-rs.json"),
            r#"{"name":"188-maw-rs","windows":[{"name":"maw-rs-oracle","repo":"github.com/Soul-Brews-Studio/missing"}]}"#,
        )
        .expect("fleet json");
        std::env::set_var("HOME", root.join("wrong-home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("GHQ_ROOT");
        let mut runtime = FleetFakeRuntime {
            ghq_root: Some(root.join("real-ghq").display().to_string()),
            ..Default::default()
        };

        let (code, stdout) = fleet_run_with(&fleet_strings(&["doctor", "--json"]), &mut runtime).expect("doctor");

        assert_eq!(code, 1);
        assert!(runtime.commands.iter().any(|(program, args)| program == "ghq" && args == &["root".to_owned()]));
        let single = root.join("real-ghq/github.com/Soul-Brews-Studio/missing").display().to_string();
        assert!(stdout.contains(&single), "{stdout}");
        assert!(!stdout.contains("github.com/github.com"), "{stdout}");
        assert!(!stdout.contains("wrong-home"), "{stdout}");
    }

    #[test]
    fn fleet_add_registers_live_session_windows_from_fake_tmux() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _now = EnvVarRestore::capture("MAW_RS_FLEET_REGISTRY_NOW");
        let root = fleet_temp_root("add");
        std::fs::create_dir_all(root.join("config")).expect("config dir");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("GHQ_ROOT");
        std::env::set_var("MAW_RS_FLEET_REGISTRY_NOW", "2026-07-03T01:02:03.000Z");
        let repo = root.join("real-ghq/github.com/Soul-Brews-Studio/maw-rs");
        let mut runtime = FleetFakeRuntime {
            ghq_root: Some(root.join("real-ghq").display().to_string()),
            sessions: vec![TmuxSession {
                name: "188-maw-rs".to_owned(),
                windows: vec![
                    maw_tmux::TmuxWindow {
                        index: 0,
                        name: "maw-rs-oracle".to_owned(),
                        active: true,
                        cwd: Some(repo.join("agents/fleet-register").display().to_string()),
                    },
                    maw_tmux::TmuxWindow {
                        index: 1,
                        name: "scratch".to_owned(),
                        active: false,
                        cwd: Some("/tmp/scratch".to_owned()),
                    },
                ],
            }],
            ..Default::default()
        };

        let (code, stdout) = fleet_run_with(&fleet_strings(&["add", "188-maw-rs"]), &mut runtime).expect("add");

        assert_eq!(code, 0);
        assert!(stdout.contains("fleet add 188-maw-rs: created"), "{stdout}");
        let path = root.join("home/.maw/fleet/188-maw-rs.json");
        let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("registry")).expect("json");
        assert_eq!(json["name"], "188-maw-rs");
        assert_eq!(json["created_at"], "2026-07-03T01:02:03.000Z");
        assert_eq!(json["created_by"], "maw fleet add");
        assert_eq!(json["auto_registered"], false);
        assert_eq!(json["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(json["windows"][0]["name"], "maw-rs-oracle");
        assert_eq!(json["windows"][0]["repo"], "Soul-Brews-Studio/maw-rs");
        assert_eq!(json["windows"][0]["kind"], "oracle");
    }

    const FLEET_SQUADRON_JSON: &str =
        r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"},{"handle":"drift"}]}"#;

    #[test]
    fn fleet_renumber_dry_run_skips_99_by_default() {
        fleet_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/99-bud.json"),
                r#"{"name":"99-bud","windows":[],"mystery":true}"#,
            )
            .expect("bud");
            let mut runtime = FleetFakeRuntime::default();
            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--dry-run", "--json"]), &mut runtime).expect("renumber");
            assert_eq!(code, 0);
            let value: serde_json::Value = serde_json::from_str(&stdout).expect("json");
            assert_eq!(value["dryRun"], true);
            assert_eq!(value["include99"], false);
            assert_eq!(value["configs"].as_array().expect("configs").len(), 1);
            assert_eq!(value["configs"][0]["oldName"], "03-alpha");
            assert_eq!(value["configs"][0]["newName"], "01-alpha");
            assert!(root.join("config/fleet/03-alpha.json").exists());
            assert!(root.join("config/fleet/99-bud.json").exists());
        });
    }

    #[test]
    fn fleet_renumber_include_99_rewrites_configs_and_renames_tmux() {
        fleet_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/99-bud.json"),
                r#"{"name":"99-bud","windows":[],"mystery":true}"#,
            )
            .expect("bud");
            let mut runtime = FleetFakeRuntime {
                sessions: vec![
                    TmuxSession { name: "03-alpha".to_owned(), windows: Vec::new() },
                    TmuxSession { name: "99-bud".to_owned(), windows: Vec::new() },
                ],
                ..Default::default()
            };

            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--include-99"]), &mut runtime).expect("renumber");

            assert_eq!(code, 0);
            assert!(stdout.contains("renamed 03-alpha.json -> 01-alpha.json"), "{stdout}");
            assert!(stdout.contains("renamed 99-bud.json -> 02-bud.json"), "{stdout}");
            assert!(!root.join("config/fleet/03-alpha.json").exists());
            assert!(!root.join("config/fleet/99-bud.json").exists());
            let alpha: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("config/fleet/01-alpha.json")).expect("alpha")).expect("alpha json");
            let bud: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("config/fleet/02-bud.json")).expect("bud")).expect("bud json");
            assert_eq!(alpha["name"], "01-alpha");
            assert_eq!(bud["name"], "02-bud");
            assert_eq!(bud["mystery"], true);
            assert!(runtime.commands.iter().any(|(program, args)| program == "tmux" && args == &fleet_strings(&["rename-session", "-t", "03-alpha", "01-alpha"])));
            assert!(runtime.commands.iter().any(|(program, args)| program == "tmux" && args == &fleet_strings(&["rename-session", "-t", "99-bud", "02-bud"])));
        });
    }


    #[test]
    fn fleet_renumber_only_99_dry_run_fills_gaps_without_touching_existing() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-root.json"), r#"{"name":"01-root","windows":[]}"#).expect("root");
            std::fs::write(root.join("config/fleet/99-bud.json"), r#"{"name":"99-bud","windows":[],"mystery":true}"#).expect("bud");
            std::fs::write(root.join("config/fleet/99-cat.json"), r#"{"name":"99-cat","windows":[]}"#).expect("cat");
            std::fs::write(root.join("config/fleet/99-overview.json"), r#"{"name":"99-overview","windows":[]}"#).expect("overview");
            let mut runtime = FleetFakeRuntime::default();

            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--only-99", "--dry-run", "--json"]), &mut runtime).expect("renumber");

            assert_eq!(code, 0);
            let value: serde_json::Value = serde_json::from_str(&stdout).expect("json");
            assert_eq!(value["only99"], true);
            assert_eq!(value["configs"].as_array().expect("configs").len(), 2);
            assert_eq!(value["configs"][0]["oldName"], "99-bud");
            assert_eq!(value["configs"][0]["newName"], "02-bud");
            assert_eq!(value["configs"][1]["newName"], "04-cat");
            assert!(root.join("config/fleet/01-root.json").exists());
            assert!(root.join("config/fleet/03-alpha.json").exists());
            assert!(root.join("config/fleet/99-bud.json").exists());
            assert!(runtime.commands.is_empty());
        });
    }

    #[test]
    fn fleet_renumber_only_99_rewrites_only_99_and_renames_tmux() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-root.json"), r#"{"name":"01-root","windows":[]}"#).expect("root");
            std::fs::write(root.join("config/fleet/99-bud.json"), r#"{"name":"99-bud","windows":[],"mystery":true}"#).expect("bud");
            let mut runtime = FleetFakeRuntime { sessions: vec![TmuxSession { name: "99-bud".to_owned(), windows: Vec::new() }], ..Default::default() };

            let (code, stdout) = fleet_run_with(&fleet_strings(&["renumber", "--only-99"]), &mut runtime).expect("renumber");

            assert_eq!(code, 0);
            assert!(stdout.contains("only-99: true"), "{stdout}");
            assert!(stdout.contains("renamed 99-bud.json -> 02-bud.json"), "{stdout}");
            assert!(root.join("config/fleet/01-root.json").exists());
            assert!(root.join("config/fleet/03-alpha.json").exists());
            assert!(!root.join("config/fleet/99-bud.json").exists());
            let bud: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(root.join("config/fleet/02-bud.json")).expect("bud")).expect("json");
            assert_eq!(bud["name"], "02-bud");
            assert_eq!(bud["mystery"], true);
            assert!(runtime.commands.iter().any(|(program, args)| program == "tmux" && args == &fleet_strings(&["rename-session", "-t", "99-bud", "02-bud"])));
        });
    }

    #[test]
    fn fleet_wake_bare_errors_and_all_sweep_excludes_roster_files() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let bare = run_fleet_command(&fleet_strings(&["wake"]));
            assert_eq!(bare.code, 1);
            assert!(bare.stderr.contains("specify a squad, or --all"), "{}", bare.stderr);
            let all = run_fleet_command(&fleet_strings(&["wake", "--all", "--json", "--dry-run"]));
            assert_eq!(all.code, 0, "{}", all.stderr);
            assert!(all.stdout.contains("\"action\": \"wake\"") && all.stdout.contains("\"sessionCount\": 1"), "{}", all.stdout);
            assert!(all.stdout.contains("03-alpha") && !all.stdout.contains("01-3e"), "{}", all.stdout);
            assert!(!all.stdout.contains("22-dormant"), "disabled entries stay skipped");
            let alias = run_fleet_command(&fleet_strings(&["wake-all", "--dry-run"]));
            assert_eq!(alias.code, 0, "{}", alias.stderr);
            assert!(alias.stdout.contains("  - 03-alpha") && !alias.stdout.contains("01-3e"), "{}", alias.stdout);
            let sleep = run_fleet_command(&fleet_strings(&["sleep", "--all", "--dry-run"]));
            assert_eq!(sleep.code, 0, "{}", sleep.stderr);
            assert!(sleep.stdout.contains("  - 03-alpha") && !sleep.stdout.contains("01-3e"), "{}", sleep.stdout);
        });
    }

    #[test]
    fn fleet_wake_group_scopes_plan_to_squadron_members() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let plan = run_fleet_command(&fleet_strings(&["wake", "3e", "--dry-run"]));
            assert_eq!(plan.code, 0, "{}", plan.stderr);
            assert!(plan.stdout.contains("squad: 3e · members: 2 · sessions: 1 · skipped: 1"), "{}", plan.stdout);
            assert!(plan.stdout.contains("  - alpha -> 03-alpha"), "{}", plan.stdout);
            assert!(plan.stdout.contains("  - drift skipped: no session"), "{}", plan.stdout);
            let json = run_fleet_command(&fleet_strings(&["sleep", "3e", "--json", "--dry-run"]));
            assert_eq!(json.code, 0, "{}", json.stderr);
            let value: serde_json::Value = serde_json::from_str(&json.stdout).expect("json");
            assert_eq!(value["action"], "sleep");
            assert_eq!(value["squad"], "3e");
            assert_eq!(value["dryRun"], true);
            assert_eq!(value["sessions"], serde_json::json!(["03-alpha"]));
            assert_eq!(value["members"][0]["handle"], "alpha");
            assert_eq!(value["skipped"][0], serde_json::json!({"handle": "drift", "reason": "no session"}));
        });
    }


    #[test]
    fn fleet_gather_dry_run_plans_live_and_asleep_members() {
        fleet_with_fixture(|root| {
            std::fs::write(root.join("config/fleet/01-3e.json"), FLEET_SQUADRON_JSON).expect("roster");
            let mut runtime = FleetFakeRuntime {
                sessions: vec![TmuxSession { name: "03-alpha".to_owned(), windows: Vec::new() }],
                ..FleetFakeRuntime::default()
            };
            let (code, stdout) = fleet_run_with(&fleet_strings(&["gather", "3e", "--dry-run"]), &mut runtime).expect("gather");
            assert_eq!(code, 0);
            assert!(stdout.contains("fleet gather plan node: alpha"), "{stdout}");
            assert!(stdout.contains("  - alpha live: join 03-alpha:maw"), "{stdout}");
            assert!(stdout.contains("  - drift asleep: skipped (no auto-wake in v1)"), "{stdout}");
            assert!(stdout.contains("  - layout: main-vertical"), "{stdout}");
            let (code, stdout) = fleet_run_with(&fleet_strings(&["gather", "3e", "--scatter", "--dry-run"]), &mut runtime).expect("scatter");
            assert_eq!(code, 0);
            assert!(stdout.contains("fleet scatter plan node: alpha"), "{stdout}");
            assert!(stdout.contains("  - alpha live: break 03-alpha:maw"), "{stdout}");
            assert!(!stdout.contains("layout:"), "{stdout}");
        });
    }

    #[test]
    fn fleet_wake_group_runs_config_post_wake_hook_per_member() {
        fleet_with_fixture(|root| {
            let marker = root.join("fleet-ready.txt");
            let hook = format!(
                "printf '%s|%s|%s\\n' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" >> {}",
                wake_shell_quote(&marker.display().to_string())
            );
            std::fs::write(
                root.join("config/maw.config.json"),
                serde_json::to_string(&serde_json::json!({"node":"alpha","hooks":{"postWake":[hook]}})).expect("json"),
            )
            .expect("write config hook");
            std::fs::write(
                root.join("config/fleet/01-hooks.json"),
                r#"{"name":"01-hooks","squadName":"hooks","windows":[],"members":[{"handle":"maw"},{"handle":"ghost"}]}"#,
            )
            .expect("roster");

            let output = run_fleet_command(&fleet_strings(&["wake", "hooks"]));

            assert_eq!(output.code, 0, "{}", output.stderr);
            let lines = std::fs::read_to_string(&marker).expect("marker");
            assert_eq!(lines.lines().collect::<Vec<_>>(), vec!["maw|03-alpha|maw", "ghost|03-alpha|ghost"]);
        });
    }

    #[test]
    fn fleet_wake_group_errors_for_missing_or_empty_squadron() {
        fleet_with_fixture(|root| {
            let missing = run_fleet_command(&fleet_strings(&["wake", "nope"]));
            assert_eq!(missing.code, 1);
            assert!(missing.stderr.contains("fleet wake: no squad named nope"), "{}", missing.stderr);
            std::fs::write(
                root.join("config/fleet/02-empty.json"),
                r#"{"name":"02-empty","squadName":"empty","windows":[],"members":[]}"#,
            )
            .expect("roster");
            let empty = run_fleet_command(&fleet_strings(&["wake", "empty"]));
            assert_eq!(empty.code, 1);
            assert!(empty.stderr.contains("fleet wake: squad empty has no members"), "{}", empty.stderr);
            let both = run_fleet_command(&fleet_strings(&["wake", "empty", "--all"]));
            assert_eq!(both.code, 1);
            assert!(both.stderr.contains("pass a squad or --all, not both"), "{}", both.stderr);
        });
    }

    #[test]
    fn fleet_gc_dry_run_composes_auto_legacy_and_manual_entry_rules() {
        fleet_with_fixture(|root| {
            let ghost = root.join("config/fleet/04-auto-ghost.json");
            std::fs::create_dir_all(root.join("ghq/github.com/acme/live-repo"))
                .expect("live repo");
            std::fs::write(
                &ghost,
                r#"{"name":"04-auto-ghost","auto_registered":true,"windows":[{"name":"ghost","repo":"acme/live-repo"}]}"#,
            )
            .expect("ghost");
            let manual = root.join("config/fleet/05-manual-ghost.json");
            std::fs::write(
                &manual,
                r#"{"name":"05-manual-ghost","auto_registered":false,"windows":[{"name":"manual","repo":"acme/missing"}]}"#,
            )
            .expect("manual ghost");
            let legacy = root.join("config/fleet/06-legacy-ghost.json");
            std::fs::write(
                &legacy,
                r#"{"name":"06-legacy-ghost","windows":[{"name":"legacy","repo":"acme/missing"}]}"#,
            )
            .expect("legacy ghost");
            let mut runtime = FleetFakeRuntime::default();
            let state = fleet_load_state_with(&mut runtime).expect("state");
            let options = fleet_parse_args(&fleet_strings(&["gc", "--dry-run"])).expect("parse");
            let mut tmux = FleetMockTmux { sessions: String::new() };
            let (code, stdout) = fleet_run_gc(&state, &options, &mut tmux).expect("gc");

            assert_eq!(code, 0);
            assert!(stdout.contains("[dry-run] would disable"));
            assert!(stdout.contains("04-auto-ghost.json"));
            assert!(stdout.contains("06-legacy-ghost.json"));
            assert!(!stdout.contains("05-manual-ghost.json"));
            assert!(!stdout.contains("03-alpha.json"));
            assert!(ghost.exists());
            assert!(manual.exists());
            assert!(legacy.exists());
            assert!(!ghost
                .with_file_name("04-auto-ghost.json.disabled")
                .exists());
        });
    }

    #[test]
    fn fleet_session_consumers_ignore_squad_rosters() {
        fleet_with_fixture(|root| {
            let roster = root.join("config/fleet/squads/01-3e/squad.json");
            std::fs::create_dir_all(roster.parent().expect("roster parent")).expect("roster dir");
            std::fs::write(
                &roster,
                r#"{"name":"01-3e","squadName":"3e","windows":[{"name":"durable","repo":"acme/missing-squad"}],"members":[]}"#,
            )
            .expect("roster");
            let mut runtime = FleetFakeRuntime::default();
            let state = fleet_load_state_with(&mut runtime).expect("state");
            assert!(state.sessions.iter().all(|session| session.name != "01-3e"));
            assert!(fleet_gc_candidates(&state, &BTreeSet::new())
                .iter()
                .all(|candidate| candidate.path != roster));
        });
    }

    #[test]
    fn fleet_upsert_never_writes_a_session_snapshot_into_a_squad_folder() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let root = fleet_temp_root("upsert-squad-boundary");
        let roster = root.join("state/fleet/squads/01-3e/squad.json");
        std::fs::create_dir_all(roster.parent().expect("roster parent")).expect("roster dir");
        let roster_body = r#"{"name":"01-3e","squadName":"3e","unknown":"keep","windows":[{"name":"durable","repo":"acme/roster"}],"members":[]}"#;
        std::fs::write(&roster, roster_body).expect("roster");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        let windows = vec![FleetWindowSummary {
            name: "live".to_owned(),
            repo: "acme/live".to_owned(),
            kind: None,
        }];

        let written = fleet_registry_upsert_session_for_env(
            &current_xdg_env(),
            "01-3e",
            &windows,
            "maw fleet add",
        )
        .expect("upsert");

        assert_eq!(written.path, root.join("home/.maw/fleet/01-3e.json"));
        assert_eq!(std::fs::read_to_string(roster).expect("unchanged roster"), roster_body);
    }

    #[test]
    fn fleet_upsert_session_follows_stem_matches_and_repo_overlap_across_state_and_home_dirs() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-cross-dir");
        std::fs::create_dir_all(root.join("config/fleet")).expect("config fleet dir");
        std::fs::create_dir_all(root.join("state/fleet")).expect("state fleet dir");
        std::fs::write(root.join("config/fleet/63-homekeeper.json"), r#"{"name":"63-homekeeper","windows":[{"name":"main","repo":"github.com/acme/homekeeper-oracle","kind":"oracle"}]}"#)
            .expect("state fixture");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let windows = vec![FleetWindowSummary {
            name: "main".to_owned(),
            repo: "github.com/acme/homekeeper-oracle".to_owned(),
            kind: None,
        }];
        let written = fleet_registry_upsert_session_for_env(&current_xdg_env(), "158-homekeeper", &windows, "maw fleet add").expect("upsert");

        assert_eq!(written.path, root.join("config/fleet/63-homekeeper.json"));
        let merged = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&written.path).expect("registry")).expect("json");
        assert_eq!(merged["name"], "158-homekeeper");
        assert_eq!(merged["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(merged["windows"][0]["repo"], "acme/homekeeper-oracle");
    }

    #[test]
    fn fleet_upsert_uses_canonical_repo_overlap_to_merge_symlinked_paths() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-symlink-canonical");
        std::fs::create_dir_all(root.join("state/fleet")).expect("state fleet dir");
        let real = root.join("ghq/github.com/acme/homekeeper-oracle");
        let linked = root.join("ghq/github.com/acme/homelab");
        std::fs::create_dir_all(&real).expect("repo");
        #[cfg(unix)] {
            use std::os::unix::fs::symlink;
            symlink(&real, &linked).expect("symlink repo");
        }

        std::fs::write(
            root.join("state/fleet/63-homelab.json"),
            r#"{"name":"63-homelab","windows":[{"name":"main","repo":"github.com/acme/homekeeper-oracle","kind":"oracle"}] }"#,
        )
        .expect("state fixture");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let windows = vec![FleetWindowSummary {
            name: "main".to_owned(),
            repo: "github.com/acme/homelab".to_owned(),
            kind: None,
        }];
        let written = fleet_registry_upsert_session_for_env(&current_xdg_env(), "158-homelab", &windows, "maw fleet add").expect("upsert");

        assert_eq!(written.path, root.join("state/fleet/63-homelab.json"));
        let merged = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&written.path).expect("registry")).expect("json");
        assert_eq!(merged["name"], "158-homelab");
        assert_eq!(merged["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(merged["windows"][0]["repo"], "acme/homelab");

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn fleet_upsert_deduplicates_bud_and_wake_names_for_one_live_window() {
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-bud-wake-dedup");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet dir");
        let path = root.join("config/fleet/10-oracle-dig-ui.json");
        let bud = BudContext {
            stem: "oracle-dig-ui".to_owned(),
            org: "Soul-Brews-Studio".to_owned(),
            parent: None,
            repo_name: "oracle-dig-ui-oracle".to_owned(),
            slug: "Soul-Brews-Studio/oracle-dig-ui-oracle".to_owned(),
            repo_path: root.join("ghq/github.com/Soul-Brews-Studio/oracle-dig-ui-oracle"),
        };
        let mut registered = serde_json::json!({
            "name": "10-oracle-dig-ui",
            "windows": [],
        });
        bud_fleet_ensure_window(&mut registered, &bud).expect("bud registration");
        std::fs::write(&path, serde_json::to_string_pretty(&registered).expect("bud json"))
            .expect("bud fleet file");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("GHQ_ROOT", root.join("ghq"));

        let live_windows = vec![FleetWindowSummary {
            name: "oracle-dig-ui".to_owned(),
            repo: "github.com/Soul-Brews-Studio/oracle-dig-ui-oracle".to_owned(),
            kind: Some(NativeRepoKind::Project),
        }];
        fleet_registry_upsert_session_for_env(
            &current_xdg_env(),
            "10-oracle-dig-ui",
            &live_windows,
            "maw wake",
        )
        .expect("wake registration");

        let merged: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(path).expect("registry")).expect("json");
        assert_eq!(merged["windows"].as_array().expect("windows").len(), 1);
        assert_eq!(merged["windows"][0]["name"], "oracle-dig-ui");
        assert_eq!(merged["windows"][0]["repo"], "Soul-Brews-Studio/oracle-dig-ui-oracle");
    }

    #[test]
    fn fleet_upsert_prefers_exact_name_entry_over_stem_sibling_for_revived_session() {
        // #312 revives session names from the registry; when that session
        // re-registers itself the upsert must update its own entry in place —
        // not get treated as a duplicate of an earlier-sorting same-stem
        // sibling (which would mint a second entry with the same name).
        let _guard = env_test_lock();
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");

        let root = fleet_temp_root("upsert-revive-exact");
        std::env::remove_var("MAW_HOME");
        std::fs::create_dir_all(root.join("config/fleet")).expect("config fleet dir");
        std::fs::write(
            root.join("config/fleet/63-mother.json"),
            r#"{"name":"63-mother","windows":[{"name":"main","repo":"github.com/laris-co/mother-oracle","kind":"oracle"}]}"#,
        )
        .expect("stale sibling fixture");
        std::fs::write(
            root.join("config/fleet/99-mother.json"),
            r#"{"name":"99-mother","windows":[{"name":"main","repo":"github.com/laris-co/mother-oracle","kind":"oracle"}]}"#,
        )
        .expect("revived fixture");

        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));

        let windows = vec![FleetWindowSummary {
            name: "main".to_owned(),
            repo: "github.com/laris-co/mother-oracle".to_owned(),
            kind: None,
        }];
        let written = fleet_registry_upsert_session_for_env(&current_xdg_env(), "99-mother", &windows, "maw wake").expect("upsert");

        assert_eq!(written.path, root.join("config/fleet/99-mother.json"));
        assert!(!written.created);
        let revived = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&written.path).expect("registry")).expect("json");
        assert_eq!(revived["name"], "99-mother");
        assert_eq!(revived["windows"].as_array().expect("windows").len(), 1);
        let sibling = serde_json::from_str::<serde_json::Value>(
            &std::fs::read_to_string(root.join("config/fleet/63-mother.json")).expect("sibling"),
        )
        .expect("sibling json");
        assert_eq!(sibling["name"], "63-mother");
        assert!(!root.join("home/.maw/fleet/99-mother.json").exists());

        let _ = std::fs::remove_dir_all(root);
    }
}
