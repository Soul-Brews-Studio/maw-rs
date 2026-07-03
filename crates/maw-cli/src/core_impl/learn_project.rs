const DISPATCH_291: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "learn",
        handler: Handler::Sync(run_learn_command),
    },
    DispatcherEntry {
        command: "project",
        handler: Handler::Sync(run_project_command),
    },
    DispatcherEntry {
        command: "park",
        handler: Handler::Sync(run_park_command),
    },
    DispatcherEntry {
        command: "cleanup",
        handler: Handler::Sync(run_cleanup_fail_closed_command),
    },
];

const LEARN_USAGE: &str = "usage: maw learn <repo> [--fast|--deep]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LearnMode {
    Default,
    Fast,
    Deep,
}

impl LearnMode {
    fn label(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Fast => "fast",
            Self::Deep => "deep",
        }
    }

    fn agents(self) -> u8 {
        match self {
            Self::Default => 3,
            Self::Fast => 1,
            Self::Deep => 5,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LearnOptions {
    repo: String,
    mode: LearnMode,
}

fn run_learn_command(argv: &[String]) -> CliOutput {
    match learn_parse(argv) {
        Ok(options) => CliOutput {
            code: 0,
            stdout: learn_render_stub(&options),
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

// ── project ──────────────────────────────────────────────────────────────────

const PROJECT_TRACK_URL: &str = "https://github.com/Soul-Brews-Studio/maw-js/issues/523";

fn project_help() -> String {
    format!(
        "usage: maw project <learn|incubate|find|list> [args...]\n\
         \x20 learn    <url>   \u{2014} clone repo for study (symlink in \u{03c8}/learn/)\n\
         \x20 incubate <url>   \u{2014} clone repo for development (symlink in \u{03c8}/incubate/)\n\
         \x20 find     <query> \u{2014} search tracked repos (alias: search)\n\
         \x20 list             \u{2014} list all tracked repos\n\
         \n\
         see Oracle skill /project for the full implementation (scaffold tracks {PROJECT_TRACK_URL})."
    )
}

fn project_stub_line(action: &str, detail: &str) -> String {
    format!(
        "project {action}: {detail} \u{2014} not yet implemented in core plugin; \
         use Oracle skill /project for full behavior.\n\
         \x20 track: {PROJECT_TRACK_URL}"
    )
}

fn run_project_command(argv: &[String]) -> CliOutput {
    // Filter out every `--`-prefixed token first.
    let positional: Vec<&str> = argv
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(String::as_str)
        .collect();

    let sub = positional.first().copied().unwrap_or("");
    match sub {
        "" => CliOutput {
            code: 0,
            stdout: project_help(),
            stderr: String::new(),
        },
        "learn" | "incubate" => {
            let url = positional.get(1).copied().unwrap_or("");
            if url.is_empty() {
                CliOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: format!("usage: maw project {sub} <url>"),
                }
            } else {
                let dir = if sub == "learn" {
                    "\u{03c8}/learn"
                } else {
                    "\u{03c8}/incubate"
                };
                CliOutput {
                    code: 0,
                    stdout: project_stub_line(
                        sub,
                        &format!(
                            "would clone \"{url}\" and symlink into {dir}/<owner>/<repo>"
                        ),
                    ),
                    stderr: String::new(),
                }
            }
        }
        "find" | "search" => {
            let query = positional.get(1).copied().unwrap_or("");
            if query.is_empty() {
                CliOutput {
                    code: 1,
                    stdout: String::new(),
                    stderr: "usage: maw project find <query>".to_owned(),
                }
            } else {
                CliOutput {
                    code: 0,
                    stdout: project_stub_line(
                        "find",
                        &format!(
                            "would search tracked repos for \"{query}\" \
                             across \u{03c8}/learn and \u{03c8}/incubate"
                        ),
                    ),
                    stderr: String::new(),
                }
            }
        }
        "list" => CliOutput {
            code: 0,
            stdout: project_stub_line(
                "list",
                "would list all tracked repos from \u{03c8}/learn and \u{03c8}/incubate",
            ),
            stderr: String::new(),
        },
        other => CliOutput {
            code: 1,
            stdout: project_help(),
            stderr: format!(
                "maw project: unknown subcommand \"{other}\" (expected learn|incubate|find|list)"
            ),
        },
    }
}

// park implementation lives in part292.rs (run_park_command, resolve_park,
// time_ago_ms, and helpers). All are in scope via include!.

fn run_cleanup_fail_closed_command(_: &[String]) -> CliOutput {
    missing_cmd_fail_closed("cleanup")
}

fn missing_cmd_fail_closed(command: &str) -> CliOutput {
    CliOutput {
        code: 1,
        stdout: String::new(),
        stderr: format!(
            "{command} not yet native in maw-rs — port pending; use maw-js for now\n"
        ),
    }
}

fn learn_parse(argv: &[String]) -> Result<LearnOptions, String> {
    let mut repo = None;
    let mut fast = false;
    let mut deep = false;
    let mut unknown = Vec::new();

    for arg in argv {
        match arg.as_str() {
            "--fast" => fast = true,
            "--deep" => deep = true,
            value if value.starts_with("--") => unknown.push(value.to_owned()),
            value if repo.is_none() => repo = Some(value.to_owned()),
            _ => {}
        }
    }

    if fast && deep {
        return Err("maw learn: --fast and --deep are mutually exclusive".to_owned());
    }
    if !unknown.is_empty() {
        return Err(format!(
            "maw learn: unknown flag(s) {} (accepts --fast, --deep)",
            unknown.join(", ")
        ));
    }
    let repo = repo.ok_or_else(|| LEARN_USAGE.to_owned())?;
    learn_validate_repo(&repo)?;
    let mode = if fast {
        LearnMode::Fast
    } else if deep {
        LearnMode::Deep
    } else {
        LearnMode::Default
    };
    Ok(LearnOptions { repo, mode })
}

fn learn_validate_repo(repo: &str) -> Result<(), String> {
    if repo.is_empty() || repo.trim() != repo || repo == "--" || repo.starts_with('-') {
        return Err(
            "maw learn: repo must be non-empty, unpadded, not '--', and not start with '-'"
                .to_owned(),
        );
    }
    if repo.chars().any(char::is_control) {
        return Err("maw learn: repo must not contain control characters".to_owned());
    }
    Ok(())
}

fn learn_render_stub(options: &LearnOptions) -> String {
    format!(
        "learn: {} mode on \"{}\" — not yet implemented in core plugin; use Oracle skill /learn for full behavior.\n  planned: {} parallel agent(s), write docs to ψ/learn/<owner>/<repo>/YYYY-MM-DD/HHMM_*.md\n  track:   https://github.com/Soul-Brews-Studio/maw-js/issues/521\n",
        options.mode.label(),
        options.repo,
        options.mode.agents()
    )
}

#[cfg(test)]
mod missing_cmds_tests291 {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn part291_registers_only_missing_cmds_as_native() {
        let commands: Vec<&str> = DISPATCH_291.iter().map(|entry| entry.command).collect();
        assert_eq!(commands, ["learn", "project", "park", "cleanup"]);
        for command in commands {
            assert_eq!(dispatcher_status(command), DispatchKind::Native, "{command}");
        }
    }

    #[test]
    fn learn_stub_default_fast_and_deep_match_current_maw_js_contract() {
        let default = run_learn_command(&args(&["owner/repo"]));
        assert_eq!(default.code, 0);
        assert_eq!(
            default.stdout,
            "learn: default mode on \"owner/repo\" — not yet implemented in core plugin; use Oracle skill /learn for full behavior.\n  planned: 3 parallel agent(s), write docs to ψ/learn/<owner>/<repo>/YYYY-MM-DD/HHMM_*.md\n  track:   https://github.com/Soul-Brews-Studio/maw-js/issues/521\n"
        );
        assert!(default.stderr.is_empty());

        let fast = run_learn_command(&args(&["owner/repo", "--fast"]));
        assert_eq!(fast.code, 0);
        assert!(fast.stdout.contains("learn: fast mode"));
        assert!(fast.stdout.contains("planned: 1 parallel agent(s)"));

        let deep = run_learn_command(&args(&["--deep", "owner/repo"]));
        assert_eq!(deep.code, 0);
        assert!(deep.stdout.contains("learn: deep mode"));
        assert!(deep.stdout.contains("planned: 5 parallel agent(s)"));
    }

    #[test]
    fn learn_stub_parser_errors_are_fail_closed_before_any_fallback() {
        let missing = run_learn_command(&[]);
        assert_eq!(missing.code, 2);
        assert_eq!(missing.stderr, format!("{LEARN_USAGE}\n"));

        let conflict = run_learn_command(&args(&["owner/repo", "--fast", "--deep"]));
        assert_eq!(conflict.code, 2);
        assert_eq!(
            conflict.stderr,
            "maw learn: --fast and --deep are mutually exclusive\n"
        );

        let unknown = run_learn_command(&args(&["owner/repo", "--wide", "--json"]));
        assert_eq!(unknown.code, 2);
        assert_eq!(
            unknown.stderr,
            "maw learn: unknown flag(s) --wide, --json (accepts --fast, --deep)\n"
        );

        let injected = run_learn_command(&args(&["-oProxyCommand=bad"]));
        assert_eq!(injected.code, 2);
        assert!(injected.stderr.contains("not start with '-'"));

        let control = run_learn_command(&args(&["bad\nrepo"]));
        assert_eq!(control.code, 2);
        assert!(control.stderr.contains("control characters"));
    }

    #[test]
    fn cleanup_refuses_nonzero_without_delegation_text() {
        // cleanup is still fail-closed; project and park are now native.
        let output = run_cli(&args(&["cleanup", "--anything"]));
        assert_eq!(output.code, 1, "cleanup");
        assert!(output.stdout.is_empty(), "cleanup: stdout={}", output.stdout);
        assert_eq!(
            output.stderr,
            "cleanup not yet native in maw-rs — port pending; use maw-js for now\n",
        );
        assert!(!output.stdout.contains("DELEGATED-MAW"));
        assert!(!output.stderr.contains("DELEGATED-MAW"));
        assert!(!output.stdout.contains("bun"));
        assert!(!output.stderr.contains("bun"));
    }

    #[test]
    fn project_no_args_returns_help_exit0() {
        let out = run_project_command(&[]);
        assert_eq!(out.code, 0, "stdout={} stderr={}", out.stdout, out.stderr);
        assert!(out.stderr.is_empty());
        assert!(out.stdout.contains("usage: maw project"));
        assert!(out.stdout.contains("learn"));
        assert!(out.stdout.contains("incubate"));
        assert!(out.stdout.contains("find"));
        assert!(out.stdout.contains("list"));
        assert!(out.stdout.contains(PROJECT_TRACK_URL));
        // No trailing newline (goldens authoritative).
        assert!(!out.stdout.ends_with('\n'), "stdout must not end with newline");
    }

    #[test]
    fn project_list_returns_stub_exit0() {
        let out = run_project_command(&args(&["list"]));
        assert_eq!(out.code, 0);
        assert!(out.stderr.is_empty());
        assert!(out.stdout.contains("project list: would list all tracked repos"));
        assert!(out.stdout.contains(PROJECT_TRACK_URL));
        assert!(!out.stdout.ends_with('\n'));
    }

    #[test]
    fn project_learn_with_url_returns_stub_exit0() {
        let out = run_project_command(&args(&[
            "learn",
            "https://github.com/Soul-Brews-Studio/maw-js",
        ]));
        assert_eq!(out.code, 0);
        assert!(out.stderr.is_empty());
        assert!(out.stdout.contains(
            "project learn: would clone \"https://github.com/Soul-Brews-Studio/maw-js\""
        ));
        assert!(out.stdout.contains("\u{03c8}/learn/<owner>/<repo>"));
        assert!(!out.stdout.ends_with('\n'));
    }

    #[test]
    fn project_incubate_with_url_returns_stub_exit0() {
        let out = run_project_command(&args(&[
            "incubate",
            "https://github.com/Soul-Brews-Studio/maw-rs",
        ]));
        assert_eq!(out.code, 0);
        assert!(out.stderr.is_empty());
        assert!(out.stdout.contains(
            "project incubate: would clone \"https://github.com/Soul-Brews-Studio/maw-rs\""
        ));
        assert!(out.stdout.contains("\u{03c8}/incubate/<owner>/<repo>"));
        assert!(!out.stdout.ends_with('\n'));
    }

    #[test]
    fn project_find_with_query_returns_stub_exit0() {
        let out = run_project_command(&args(&["find", "oracle"]));
        assert_eq!(out.code, 0);
        assert!(out.stderr.is_empty());
        assert!(out.stdout.contains(
            "project find: would search tracked repos for \"oracle\""
        ));
        assert!(!out.stdout.ends_with('\n'));
    }

    #[test]
    fn project_search_alias_works() {
        let out = run_project_command(&args(&["search", "oracle"]));
        assert_eq!(out.code, 0);
        assert!(out.stderr.is_empty());
        // search is aliased to find output
        assert!(out.stdout.contains("project find:"));
    }

    #[test]
    fn project_learn_missing_url_exit1() {
        let out = run_project_command(&args(&["learn"]));
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, "usage: maw project learn <url>");
    }

    #[test]
    fn project_incubate_missing_url_exit1() {
        let out = run_project_command(&args(&["incubate"]));
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, "usage: maw project incubate <url>");
    }

    #[test]
    fn project_find_missing_query_exit1() {
        let out = run_project_command(&args(&["find"]));
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, "usage: maw project find <query>");
    }

    #[test]
    fn project_bogus_subcommand_exit1_both_streams() {
        let out = run_project_command(&args(&["bogus"]));
        assert_eq!(out.code, 1);
        assert!(!out.stdout.is_empty(), "stdout should contain help");
        assert!(out.stdout.contains("usage: maw project"));
        assert_eq!(
            out.stderr,
            "maw project: unknown subcommand \"bogus\" (expected learn|incubate|find|list)"
        );
    }

    #[test]
    fn project_dashes_filtered_before_sub_detection() {
        // --foo list → --foo is filtered → sub = list
        let out = run_project_command(&args(&["--foo", "list"]));
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("project list:"));
    }

    #[test]
    fn resolve_park_no_args_uses_current() {
        let current = "main-win";
        let known = vec!["main-win".to_owned(), "other-win".to_owned()];
        let (target, note) = resolve_park(&[], current, &known);
        assert_eq!(target, "main-win");
        assert!(note.is_none());
    }

    #[test]
    fn resolve_park_known_other_window_as_target() {
        let current = "main-win";
        let known = vec!["main-win".to_owned(), "other-win".to_owned()];
        let raw = args(&["other-win", "my note"]);
        let (target, note) = resolve_park(&raw, current, &known);
        assert_eq!(target, "other-win");
        assert_eq!(note.as_deref(), Some("my note"));
    }

    #[test]
    fn resolve_park_known_window_same_as_current_uses_current_note() {
        let current = "main-win";
        let known = vec!["main-win".to_owned()];
        let raw = args(&["main-win", "a note"]);
        let (target, note) = resolve_park(&raw, current, &known);
        // first arg is known but IS current → falls through to note path
        assert_eq!(target, "main-win");
        assert_eq!(note.as_deref(), Some("main-win a note"));
    }

    #[test]
    fn resolve_park_unknown_first_arg_becomes_note() {
        let current = "main-win";
        let known = vec!["main-win".to_owned()];
        let raw = args(&["handoff note here"]);
        let (target, note) = resolve_park(&raw, current, &known);
        assert_eq!(target, "main-win");
        assert_eq!(note.as_deref(), Some("handoff note here"));
    }

    #[test]
    fn time_ago_ms_boundaries() {
        assert_eq!(time_ago_ms(59 * 60_000), "59m ago");
        assert_eq!(time_ago_ms(60 * 60_000), "1h ago");
        assert_eq!(time_ago_ms(23 * 3_600_000), "23h ago");
        assert_eq!(time_ago_ms(24 * 3_600_000), "1d ago");
        assert_eq!(time_ago_ms(0), "0m ago");
    }

    #[test]
    fn missing_cmds_fake_maw_no_delegate_proof() {
        let _lock = env_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _path = EnvVarRestore::capture("PATH");
        let _ref_dir = EnvVarRestore::capture("MAW_JS_REF_DIR");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-missing-cmds-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let bin_dir = root.join("bin");
        std::fs::create_dir_all(&bin_dir).expect("fake bin dir");
        let fake_maw = bin_dir.join("maw");
        std::fs::write(
            &fake_maw,
            "#!/bin/sh\nprintf 'DELEGATED-MAW\\n'\nprintf 'bun\\n'\nexit 42\n",
        )
        .expect("write fake maw");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&fake_maw)
                .expect("fake maw metadata")
                .permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&fake_maw, perms).expect("chmod fake maw");
        }
        std::env::set_var(
            "PATH",
            format!(
                "{}:{}",
                bin_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        );
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");

        for argv in [
            args(&["learn", "owner/repo", "--deep"]),
            args(&["project"]),
            args(&["park"]),
            args(&["cleanup"]),
        ] {
            let output = run_cli(&argv);
            let combined = format!("{}{}", output.stdout, output.stderr);
            assert!(!combined.contains("DELEGATED-MAW"), "argv={argv:?}");
            assert!(!combined.contains("bun"), "argv={argv:?}");
        }
    }
}
