const DISPATCH_280: &[DispatcherEntry] = &[];

const TMUX_SUB_280: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["break"],
    handler: run_tmux_break_command,
}];

const TMUX_BREAK_USAGE: &str = "usage: maw tmux break <pane> [--force]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxBreakOptions {
    target: String,
    force: bool,
}

fn run_tmux_break_command(argv: &[String]) -> CliOutput {
    match tmux_break_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn tmux_break_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = tmux_break_parse(argv)?;
    tmux_break_validate_target(&opts.target).map_err(|message| (1, message))?;
    if !opts.force && std::env::var("TMUX_PANE").is_ok_and(|pane| pane == opts.target) {
        return Err((1, "tmux break: refusing to break current pane; pass --force to override".to_owned()));
    }
    let break_args = tmux_break_args(&opts.target, runner);
    runner
        .run("break-pane", &break_args)
        .map_err(|error| (1, format!("tmux break: break-pane failed: {}", error.message)))?;
    Ok(format!("✓ broke {} → {} (hidden — still alive)\n", opts.target, opts.target))
}

fn tmux_break_parse(argv: &[String]) -> Result<TmuxBreakOptions, (i32, String)> {
    let mut target = None;
    let mut force = false;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, TMUX_BREAK_USAGE.to_owned())),
            "--force" => force = true,
            value if value.starts_with('-') => {
                return Err((2, format!("tmux break: unknown argument {value}")));
            }
            value => {
                if target.is_some() {
                    return Err((2, "tmux break: target already provided".to_owned()));
                }
                target = Some(value.to_owned());
            }
        }
    }
    Ok(TmuxBreakOptions {
        target: target.ok_or_else(|| (2, TMUX_BREAK_USAGE.to_owned()))?,
        force,
    })
}

fn tmux_break_args<R: maw_tmux::TmuxRunner>(target: &str, runner: &mut R) -> Vec<String> {
    let mut args = vec!["-d".to_owned(), "-t".to_owned(), target.to_owned()];
    let display_args = vec![
        "-p".to_owned(),
        "-t".to_owned(),
        target.to_owned(),
        "#{window_name}".to_owned(),
    ];
    if let Ok(name) = runner.run("display-message", &display_args) {
        let name = name.trim();
        if !name.is_empty() && tmux_break_valid_window_name(name) {
            args.push("-n".to_owned());
            args.push(name.to_owned());
        }
    }
    args
}

fn tmux_break_validate_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err("tmux break: target must be non-empty, unpadded, not '--', and not start with '-'".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err("tmux break: target must not contain control characters".to_owned());
    }
    if !value.chars().all(tmux_break_valid_target_char) {
        return Err("tmux break: target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_break_valid_target_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')
}

fn tmux_break_valid_window_name(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && !value.starts_with('-')
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tmux_break_tests {
    use super::*;

    #[derive(Default)]
    struct BreakFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        window_name: Option<String>,
    }

    impl maw_tmux::TmuxRunner for BreakFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            if subcommand == "display-message" {
                return Ok(self.window_name.clone().unwrap_or_default());
            }
            Ok(String::new())
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn tmux_break_fragment_is_part280_only() {
        assert!(DISPATCH_280.is_empty());
        assert_eq!(TMUX_SUB_280.len(), 1);
        assert_eq!(TMUX_SUB_280[0].names, &["break"]);
    }

    #[test]
    fn tmux_break_uses_tmux_runner_arg_vector_and_preserves_window_name() {
        let mut runner = BreakFakeRunner { window_name: Some("work".to_owned()), ..BreakFakeRunner::default() };
        let out = tmux_break_with_runner(&strings(&["%42"]), &mut runner).expect("break");
        assert_eq!(out, "✓ broke %42 → %42 (hidden — still alive)\n");
        assert_eq!(
            runner.calls,
            vec![
                (
                    "display-message".to_owned(),
                    strings(&["-p", "-t", "%42", "#{window_name}"]),
                ),
                (
                    "break-pane".to_owned(),
                    strings(&["-d", "-t", "%42", "-n", "work"]),
                ),
            ]
        );
    }

    #[test]
    fn tmux_break_rejects_leading_dash_before_runner() {
        let mut runner = BreakFakeRunner::default();
        let err = tmux_break_with_runner(&strings(&["-oProxyCommand=bad"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("unknown argument"));
        assert!(runner.calls.is_empty(), "guarded arg reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_break_rejects_control_target_before_runner() {
        let mut runner = BreakFakeRunner::default();
        let err = tmux_break_with_runner(&strings(&["bad\npane"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("control"));
        assert!(runner.calls.is_empty(), "guarded arg reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_break_fake_maw_no_delegate_and_no_bun_runtime() {
        let _guard = env_test_lock().lock().expect("env lock");
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut runner = BreakFakeRunner::default();
        let out = tmux_break_with_runner(&strings(&["session:1.0"]), &mut runner).expect("break");
        assert_eq!(out, "✓ broke session:1.0 → session:1.0 (hidden — still alive)\n");
        assert!(runner.calls.iter().all(|(subcommand, _)| subcommand != "bun"));
    }
}
