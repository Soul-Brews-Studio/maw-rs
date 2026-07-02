const DISPATCH_281: &[DispatcherEntry] = &[];

const TMUX_SUB_281: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["close", "unsplit"],
    handler: run_tmux_close_command,
}];

const TMUX_CLOSE_USAGE: &str = "usage: maw tmux close [pane]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxCloseOptions {
    target: Option<String>,
}

fn run_tmux_close_command(argv: &[String]) -> CliOutput {
    match tmux_close_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn tmux_close_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = tmux_close_parse(argv)?;
    let current_pane = tmux_close_current_pane()?;
    if let Some(target) = opts.target.as_deref() {
        tmux_close_validate_target(target).map_err(|message| (1, message))?;
        if target == current_pane {
            return Err((1, "tmux close: refusing to close current pane (would lose this shell)".to_owned()));
        }
        tmux_close_break_pane_detached(target, runner)?;
        return Ok(format!("✓ closed {target} (hidden — still alive)\n"));
    }

    let panes = tmux_close_list_current_window_panes(runner)?;
    if panes.len() <= 1 {
        return Ok("no panes to close\n".to_owned());
    }
    let mut hidden = 0usize;
    for pane in panes {
        if pane == current_pane {
            continue;
        }
        if tmux_close_validate_pane_id(&pane).is_err() {
            continue;
        }
        if tmux_close_break_pane_detached(&pane, runner).is_ok() {
            hidden += 1;
        }
    }
    Ok(format!("✓ closed {hidden} pane{} (hidden — still alive)\n", if hidden == 1 { "" } else { "s" }))
}

fn tmux_close_parse(argv: &[String]) -> Result<TmuxCloseOptions, (i32, String)> {
    let mut target = None;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, TMUX_CLOSE_USAGE.to_owned())),
            value if value.starts_with('-') => {
                return Err((2, format!("tmux close: unknown argument {value}")));
            }
            value => {
                if target.is_some() {
                    return Err((2, "tmux close: target already provided".to_owned()));
                }
                target = Some(value.to_owned());
            }
        }
    }
    Ok(TmuxCloseOptions { target })
}

fn tmux_close_current_pane() -> Result<String, (i32, String)> {
    if std::env::var_os("TMUX").is_none() {
        return Err((1, "tmux close: not in tmux".to_owned()));
    }
    let pane = std::env::var("TMUX_PANE")
        .map_err(|_| (1, "tmux close: current pane is unknown".to_owned()))?;
    tmux_close_validate_pane_id(&pane).map_err(|message| (1, message))?;
    Ok(pane)
}

fn tmux_close_list_current_window_panes<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<String>, (i32, String)> {
    let args = vec!["-F".to_owned(), "#{pane_id}".to_owned()];
    let raw = runner
        .run("list-panes", &args)
        .map_err(|error| (1, format!("tmux close: list-panes failed: {}", error.message)))?;
    Ok(raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
}

fn tmux_close_break_pane_detached<R: maw_tmux::TmuxRunner>(target: &str, runner: &mut R) -> Result<(), (i32, String)> {
    let args = vec!["-d".to_owned(), "-t".to_owned(), target.to_owned()];
    runner
        .run("break-pane", &args)
        .map_err(|error| (1, format!("tmux close: break-pane failed for '{target}': {}", error.message)))?;
    Ok(())
}

fn tmux_close_validate_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err("tmux close: target must be non-empty, unpadded, not '--', and not start with '-'".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err("tmux close: target must not contain control characters".to_owned());
    }
    if !value.chars().all(tmux_close_valid_target_char) {
        return Err("tmux close: target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_close_validate_pane_id(value: &str) -> Result<(), String> {
    if value.is_empty() || !value.starts_with('%') || !value[1..].bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("tmux close: pane id must look like %123".to_owned());
    }
    Ok(())
}

fn tmux_close_valid_target_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')
}

#[cfg(test)]
mod tmux_close_tests281 {
    use super::*;

    #[derive(Default)]
    struct CloseFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        panes: Vec<String>,
        fail_break: bool,
    }

    impl maw_tmux::TmuxRunner for CloseFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-panes" => Ok(self.panes.join("\n")),
                "break-pane" if self.fail_break => Err(maw_tmux::TmuxError::new("break failed")),
                "break-pane" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    struct EnvGuard {
        tmux: Option<String>,
        pane: Option<String>,
        maw_js_ref_dir: Option<String>,
    }

    impl EnvGuard {
        fn set(tmux: &str, pane: &str) -> Self {
            let guard = Self {
                tmux: std::env::var("TMUX").ok(),
                pane: std::env::var("TMUX_PANE").ok(),
                maw_js_ref_dir: std::env::var("MAW_JS_REF_DIR").ok(),
            };
            std::env::set_var("TMUX", tmux);
            std::env::set_var("TMUX_PANE", pane);
            guard
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match &self.tmux {
                Some(value) => std::env::set_var("TMUX", value),
                None => std::env::remove_var("TMUX"),
            }
            match &self.pane {
                Some(value) => std::env::set_var("TMUX_PANE", value),
                None => std::env::remove_var("TMUX_PANE"),
            }
            match &self.maw_js_ref_dir {
                Some(value) => std::env::set_var("MAW_JS_REF_DIR", value),
                None => std::env::remove_var("MAW_JS_REF_DIR"),
            }
        }
    }

    #[test]
    fn tmux_close_fragment_is_part281_only() {
        assert!(DISPATCH_281.is_empty());
        assert_eq!(TMUX_SUB_281.len(), 1);
        assert_eq!(TMUX_SUB_281[0].names, &["close", "unsplit"]);
    }

    #[test]
    fn tmux_close_explicit_target_breaks_detached_with_arg_vector() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _env = EnvGuard::set("/tmp/tmux-1/default,1,0", "%1");
        let mut runner = CloseFakeRunner::default();
        let out = tmux_close_with_runner(&strings(&["%42"]), &mut runner).expect("close");
        assert_eq!(out, "✓ closed %42 (hidden — still alive)\n");
        assert_eq!(runner.calls, vec![("break-pane".to_owned(), strings(&["-d", "-t", "%42"]))]);
    }

    #[test]
    fn tmux_close_no_target_hides_non_current_panes() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _env = EnvGuard::set("/tmp/tmux-1/default,1,0", "%2");
        let mut runner = CloseFakeRunner { panes: strings(&["%1", "%2", "%3"]), ..CloseFakeRunner::default() };
        let out = tmux_close_with_runner(&[], &mut runner).expect("close");
        assert_eq!(out, "✓ closed 2 panes (hidden — still alive)\n");
        assert_eq!(
            runner.calls,
            vec![
                ("list-panes".to_owned(), strings(&["-F", "#{pane_id}"])),
                ("break-pane".to_owned(), strings(&["-d", "-t", "%1"])),
                ("break-pane".to_owned(), strings(&["-d", "-t", "%3"])),
            ]
        );
    }

    #[test]
    fn tmux_close_refuses_current_pane_before_runner() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _env = EnvGuard::set("/tmp/tmux-1/default,1,0", "%7");
        let mut runner = CloseFakeRunner::default();
        let err = tmux_close_with_runner(&strings(&["%7"]), &mut runner).expect_err("current pane guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("refusing to close current pane"));
        assert!(runner.calls.is_empty(), "current pane reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_close_rejects_leading_dash_before_runner() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _env = EnvGuard::set("/tmp/tmux-1/default,1,0", "%1");
        let mut runner = CloseFakeRunner::default();
        let err = tmux_close_with_runner(&strings(&["-oProxyCommand=bad"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("unknown argument"));
        assert!(runner.calls.is_empty(), "guarded arg reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_close_rejects_control_target_before_runner() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _env = EnvGuard::set("/tmp/tmux-1/default,1,0", "%1");
        let mut runner = CloseFakeRunner::default();
        let err = tmux_close_with_runner(&strings(&["bad\npane"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("control"));
        assert!(runner.calls.is_empty(), "guarded arg reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_close_fake_maw_no_delegate_and_no_bun_runtime() {
        let _lock = env_test_lock().lock().expect("env lock");
        let _env = EnvGuard::set("/tmp/tmux-1/default,1,0", "%1");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut runner = CloseFakeRunner::default();
        let out = tmux_close_with_runner(&strings(&["session:1.2"]), &mut runner).expect("close");
        assert_eq!(out, "✓ closed session:1.2 (hidden — still alive)\n");
        assert!(runner.calls.iter().all(|(subcommand, _)| subcommand != "bun"));
    }
}
