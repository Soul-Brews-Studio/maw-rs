const DISPATCH_291: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "learn",
        handler: Handler::Sync(run_learn_command),
    },
    DispatcherEntry {
        command: "project",
        handler: Handler::Sync(run_project_fail_closed_command),
    },
    DispatcherEntry {
        command: "park",
        handler: Handler::Sync(run_park_fail_closed_command),
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

fn run_project_fail_closed_command(_: &[String]) -> CliOutput {
    missing_cmd_fail_closed("project")
}

fn run_park_fail_closed_command(_: &[String]) -> CliOutput {
    missing_cmd_fail_closed("park")
}

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
    fn project_park_cleanup_refuse_nonzero_without_delegation_text() {
        for command in ["project", "park", "cleanup"] {
            let output = run_cli(&args(&[command, "--anything"]));
            assert_eq!(output.code, 1, "{command}");
            assert!(output.stdout.is_empty(), "{command}: stdout={}", output.stdout);
            assert_eq!(
                output.stderr,
                format!(
                    "{command} not yet native in maw-rs — port pending; use maw-js for now\n"
                ),
                "{command}"
            );
            assert!(!output.stdout.contains("DELEGATED-MAW"), "{command}");
            assert!(!output.stderr.contains("DELEGATED-MAW"), "{command}");
            assert!(!output.stdout.contains("bun"), "{command}");
            assert!(!output.stderr.contains("bun"), "{command}");
        }
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
