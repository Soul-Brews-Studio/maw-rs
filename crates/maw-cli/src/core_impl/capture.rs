const DISPATCH_77: &[DispatcherEntry] = &[DispatcherEntry {
    command: "capture",
    handler: Handler::Sync(capture_run_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureOptions {
    target: String,
    pane: Option<u32>,
    lines: Option<u32>,
    full: bool,
}

fn capture_run_command(argv: &[String]) -> CliOutput {
    match capture_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) => output,
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn capture_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, String> {
    let options = capture_parse_args(argv)?;
    capture_validate_tmux_target(&options.target)?;
    let target = capture_apply_pane(
        resolve_local_tmux_runner_target(runner, &options.target, "capture")?,
        options.pane,
    );
    capture_validate_tmux_target(&target)?;
    let raw = capture_capture_pane(runner, &target, &options)?;
    Ok(CliOutput {
        code: 0,
        stdout: raw,
        stderr: String::new(),
    })
}

fn capture_parse_args(argv: &[String]) -> Result<CaptureOptions, String> {
    let mut rest = argv.iter().peekable();
    let mut positionals = Vec::new();
    let mut pane = None;
    let mut lines = None;
    let mut full = false;
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--help" | "-h" if positionals.is_empty() => return Err(capture_usage_cli()),
            "--" => {
                positionals.extend(rest.cloned());
                break;
            }
            "--full" => full = true,
            "--pane" => pane = Some(capture_parse_u32_flag("--pane", rest.next())?),
            "--lines" => lines = Some(capture_parse_u32_flag("--lines", rest.next())?),
            value if value.starts_with("--pane=") => pane = Some(capture_parse_u32_value("--pane", &value[7..])?),
            value if value.starts_with("--lines=") => lines = Some(capture_parse_u32_value("--lines", &value[8..])?),
            value if value.starts_with('-') && positionals.is_empty() => {
                return Err(capture_flag_like_target(value));
            }
            value if value.starts_with('-') => return Err(format!("unknown capture flag '{value}'")),
            value => positionals.push(value.to_owned()),
        }
    }
    let Some(target) = positionals.first().cloned() else { return Err(capture_usage_cli()); };
    if target.starts_with('-') || target == "--" { return Err(capture_flag_like_target(&target)); }
    Ok(CaptureOptions { target, pane, lines, full })
}

fn capture_parse_u32_flag(flag: &str, value: Option<&String>) -> Result<u32, String> {
    let Some(value) = value else { return Err(format!("{flag} requires a positive number")); };
    capture_parse_u32_value(flag, value)
}

fn capture_parse_u32_value(flag: &str, value: &str) -> Result<u32, String> {
    if value.is_empty() || value.starts_with('-') || value == "--" {
        return Err(format!("{flag} requires a positive number"));
    }
    value
        .parse::<u32>()
        .map_err(|_| format!("{flag} requires a positive number"))
}

fn capture_usage_cli() -> String {
    "usage: maw capture <target> [--pane N] [--lines N] [--full]  (see: maw peek for quick glance)".to_owned()
}

fn capture_flag_like_target(target: &str) -> String {
    format!("\"{target}\" looks like a flag, not a target.\n  usage: maw capture <target>  (see: maw peek for quick glance)")
}

fn capture_apply_pane(mut target: String, pane: Option<u32>) -> String {
    if let Some(pane) = pane {
        let _ = write!(target, ".{pane}");
    }
    target
}

fn capture_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') || target == "--" {
        return Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if target.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("tmux target/session must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn capture_capture_pane<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
    options: &CaptureOptions,
) -> Result<String, String> {
    let start = if options.full { "-".to_owned() } else { format!("-{}", options.lines.unwrap_or(50)) };
    runner
        .run(
            "capture-pane",
            &["-t".to_owned(), target.to_owned(), "-p".to_owned(), "-S".to_owned(), start],
        )
        .map_err(|error| format!("capture failed: {}", error.message))
}

#[cfg(test)]
mod capture_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct CaptureMockTmux {
        calls: Vec<(String, Vec<String>)>,
        windows: String,
        capture: String,
        fail_capture: bool,
    }

    impl maw_tmux::TmuxRunner for CaptureMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-windows" => Ok(self.windows.clone()),
                "capture-pane" if self.fail_capture => Err(maw_tmux::TmuxError::new("no pane")),
                "capture-pane" => Ok(self.capture.clone()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    struct CaptureEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl CaptureEnvGuard {
        fn new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "MAW_CONFIG_DIR", "TMUX", "PATH"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-capture-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("config/fleet")).expect("config");
            std::env::set_var("HOME", root.join("home"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
            std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
            std::env::set_var("TMUX", "fake-tmux-socket");
            std::env::set_var("PATH", root.join("bin"));
            Self { saved }
        }
    }

    impl Drop for CaptureEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn capture_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn capture_dispatch_registers_single_native_command() {
        assert_eq!(DISPATCH_77.len(), 1);
        assert_eq!(DISPATCH_77[0].command, "capture");
    }

    #[test]
    fn capture_tail_defaults_to_first_window_and_fifty_lines() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = CaptureEnvGuard::new();
        let mut tmux = CaptureMockTmux {
            windows: "03-neo|||2|||main|||1|||\n".to_owned(),
            capture: "hello\n".to_owned(),
            ..CaptureMockTmux::default()
        };

        let output = capture_with_runner(&capture_strings(&["neo"]), &mut tmux).expect("capture");

        assert_eq!(output.stdout, "hello\n");
        assert_eq!(
            tmux.calls[0],
            (
                "list-windows".to_owned(),
                capture_strings(&[
                    "-a",
                    "-F",
                    "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}",
                ]),
            )
        );
        assert_eq!(tmux.calls[1], ("capture-pane".to_owned(), capture_strings(&["-t", "03-neo:2", "-p", "-S", "-50"])));
    }

    #[test]
    fn capture_full_and_pane_override_lines() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = CaptureEnvGuard::new();
        let mut tmux = CaptureMockTmux { windows: "neo|||1|||zsh|||1|||\n".to_owned(), ..CaptureMockTmux::default() };
        let args = capture_strings(&["neo:1", "--pane", "3", "--lines", "7", "--full"]);

        let output = capture_with_runner(&args, &mut tmux).expect("capture");

        assert_eq!(output.code, 0);
        assert_eq!(tmux.calls[1], ("capture-pane".to_owned(), capture_strings(&["-t", "neo:1.3", "-p", "-S", "-"])));
    }

    #[test]
    fn capture_rejects_leading_dash_target_before_tmux() {
        let mut tmux = CaptureMockTmux::default();
        let error = capture_with_runner(&capture_strings(&["--", "-Sbad"]), &mut tmux).expect_err("guard");
        assert!(error.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn capture_rejects_bad_numeric_flags_before_tmux() {
        let mut tmux = CaptureMockTmux::default();
        let error = capture_with_runner(&capture_strings(&["neo", "--pane", "-1"]), &mut tmux).expect_err("guard");
        assert_eq!(error, "--pane requires a positive number");
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn capture_resolves_window_name_alias_and_reports_tmux_failure() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = CaptureEnvGuard::new();
        let mut tmux = CaptureMockTmux {
            windows: "03-neo|||0|||main|||1|||\n03-neo|||1|||neo-oracle|||0|||\n".to_owned(),
            fail_capture: true,
            ..CaptureMockTmux::default()
        };

        let error = capture_with_runner(&capture_strings(&["neo-oracle"]), &mut tmux).expect_err("fail");

        assert_eq!(error, "capture failed: no pane");
        assert_eq!(tmux.calls[1].1, capture_strings(&["-t", "03-neo:1", "-p", "-S", "-50"]));
    }

    #[test]
    fn capture_validate_rejects_bad_resolved_tmux_target() {
        let error = capture_validate_tmux_target("neo:bad pane").expect_err("guard");
        assert!(error.contains("whitespace"));
    }

    #[test]
    fn capture_explicit_session_window_pins_duplicate_window_names() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = CaptureEnvGuard::new();
        let mut tmux = CaptureMockTmux {
            windows: concat!(
                "webhook-relay-v3|||2|||codex-1|||1|||\n",
                "arra-oracle-v3|||4|||codex-1|||0|||\n"
            )
            .to_owned(),
            capture: "right pane\n".to_owned(),
            ..CaptureMockTmux::default()
        };

        let output = capture_with_runner(&capture_strings(&["webhook-relay-v3:codex-1"]), &mut tmux)
            .expect("capture");

        assert_eq!(output.stdout, "right pane\n");
        assert_eq!(
            tmux.calls[1],
            ("capture-pane".to_owned(), capture_strings(&["-t", "webhook-relay-v3:2", "-p", "-S", "-50"]))
        );
    }

    #[test]
    fn capture_explicit_session_window_miss_is_loud_without_cross_session_fallback() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = CaptureEnvGuard::new();
        let mut tmux = CaptureMockTmux {
            windows: concat!(
                "webhook-relay-v3|||0|||oracle|||1|||\n",
                "arra-oracle-v3|||4|||codex-1|||0|||\n"
            )
            .to_owned(),
            ..CaptureMockTmux::default()
        };

        let error = capture_with_runner(&capture_strings(&["webhook-relay-v3:codex-1"]), &mut tmux)
            .expect_err("missing window");

        assert!(error.contains("no window 'codex-1' in session 'webhook-relay-v3'"), "{error}");
        assert_eq!(tmux.calls.len(), 1, "{:?}", tmux.calls);
    }
}
