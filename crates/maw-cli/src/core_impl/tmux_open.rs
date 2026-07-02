const DISPATCH_284: &[DispatcherEntry] = &[];

const TMUX_SUB_284: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["open"],
    handler: run_tmux_open_command,
}];

const TMUX_OPEN_USAGE: &str = "usage: maw tmux open [target]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxOpenOptions {
    target: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxOpenWindow {
    index: String,
    panes: u32,
}

fn run_tmux_open_command(argv: &[String]) -> CliOutput {
    match tmux_open_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn tmux_open_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = tmux_open_parse(argv)?;
    tmux_open_require_tmux()?;
    match opts.target.as_deref() {
        Some(target) => tmux_open_target(runner, target),
        None => tmux_open_hidden_windows(runner),
    }
}

fn tmux_open_parse(argv: &[String]) -> Result<TmuxOpenOptions, (i32, String)> {
    let mut target = None;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, TMUX_OPEN_USAGE.to_owned())),
            value if value.starts_with('-') => {
                return Err((2, format!("tmux open: unknown argument {value}")));
            }
            value => {
                if target.is_some() {
                    return Err((2, "tmux open: target already provided".to_owned()));
                }
                target = Some(value.to_owned());
            }
        }
    }
    Ok(TmuxOpenOptions { target })
}

fn tmux_open_require_tmux() -> Result<(), (i32, String)> {
    if std::env::var_os("TMUX").is_some() {
        Ok(())
    } else {
        Err((1, "tmux open: not in tmux".to_owned()))
    }
}

fn tmux_open_target<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
) -> Result<String, (i32, String)> {
    tmux_open_validate_target(target).map_err(|message| (1, message))?;
    let args = tmux_open_strings(&["-h", "-l", "50%", "-t", target]);
    runner
        .run("split-window", &args)
        .map_err(|error| (1, format!("tmux open: split-window failed: {}", error.message)))?;
    Ok(format!("\x1b[32m✓\x1b[0m opened {target}\n"))
}

fn tmux_open_hidden_windows<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, (i32, String)> {
    let my_window = tmux_open_display(runner, "#{window_index}", "window_index")?;
    let my_pane = tmux_open_current_pane(runner)?;
    tmux_open_validate_target(&my_window).map_err(|message| (1, format!("tmux open: current window: {message}")))?;
    tmux_open_validate_target(&my_pane).map_err(|message| (1, format!("tmux open: current pane: {message}")))?;
    let raw = runner
        .run("list-windows", &tmux_open_strings(&["-F", "#{window_index}:#{window_panes}"]))
        .map_err(|error| (1, format!("tmux open: list-windows failed: {}", error.message)))?;
    let hidden = tmux_open_hidden_window_indices(&raw, &my_window);
    if hidden.is_empty() {
        return Ok("\x1b[90mno hidden panes to open\x1b[0m\n".to_owned());
    }
    let mut joined = 0usize;
    for window in hidden {
        if tmux_open_validate_target(&window.index).is_err() {
            continue;
        }
        let source = format!(":{}", window.index);
        if runner
            .run("join-pane", &tmux_open_strings(&["-h", "-s", &source, "-t", &my_pane]))
            .is_ok()
        {
            joined += 1;
        }
    }
    Ok(format!(
        "\x1b[32m✓\x1b[0m opened {joined} hidden pane{}\n",
        if joined == 1 { "" } else { "s" }
    ))
}

fn tmux_open_display<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    format: &str,
    label: &str,
) -> Result<String, (i32, String)> {
    runner
        .run("display-message", &tmux_open_strings(&["-p", format]))
        .map(|raw| raw.trim().to_owned())
        .map_err(|error| (1, format!("tmux open: display-message {label} failed: {}", error.message)))
}

fn tmux_open_current_pane<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, (i32, String)> {
    std::env::var("TMUX_PANE")
        .ok()
        .filter(|pane| !pane.trim().is_empty())
        .map_or_else(|| tmux_open_display(runner, "#{pane_id}", "pane_id"), Ok)
}

fn tmux_open_hidden_window_indices(raw: &str, current_window: &str) -> Vec<TmuxOpenWindow> {
    raw.lines()
        .filter_map(|line| {
            let (index, panes_raw) = line.split_once(':')?;
            let index = index.trim();
            let panes = panes_raw.trim().parse::<u32>().ok()?;
            (index != current_window && panes == 1).then(|| TmuxOpenWindow {
                index: index.to_owned(),
                panes,
            })
        })
        .collect()
}

fn tmux_open_validate_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err(
            "tmux open: target must be non-empty, unpadded, not '--', and not start with '-'"
                .to_owned(),
        );
    }
    if value.chars().any(char::is_control) {
        return Err("tmux open: target must not contain control characters".to_owned());
    }
    if !value.chars().all(tmux_open_valid_target_char) {
        return Err("tmux open: target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_open_valid_target_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')
}

fn tmux_open_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[cfg(test)]
mod tmux_open_tests {
    use super::*;


    #[derive(Default)]
    struct OpenFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        current_window: String,
        current_pane: String,
        windows: String,
        fail_join: bool,
    }

    impl maw_tmux::TmuxRunner for OpenFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" if args.last().is_some_and(|arg| arg == "#{window_index}") => {
                    Ok(self.current_window.clone())
                }
                "display-message" if args.last().is_some_and(|arg| arg == "#{pane_id}") => {
                    Ok(self.current_pane.clone())
                }
                "list-windows" => Ok(self.windows.clone()),
                "join-pane" if self.fail_join => Err(maw_tmux::TmuxError::new("join failed")),
                "join-pane" | "split-window" => Ok(String::new()),
                other => panic!("unexpected tmux subcommand: {other}"),
            }
        }
    }

    struct TmuxOpenEnvGuard {
        tmux: Option<std::ffi::OsString>,
        tmux_pane: Option<std::ffi::OsString>,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl TmuxOpenEnvGuard {
        fn in_tmux(pane: Option<&str>) -> Self {
            let guard = env_test_lock()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let tmux = std::env::var_os("TMUX");
            let tmux_pane = std::env::var_os("TMUX_PANE");
            std::env::set_var("TMUX", "/tmp/tmux-open-test,1,0");
            if let Some(pane) = pane {
                std::env::set_var("TMUX_PANE", pane);
            } else {
                std::env::remove_var("TMUX_PANE");
            }
            Self {
                tmux,
                tmux_pane,
                _lock: guard,
            }
        }

        fn outside_tmux() -> Self {
            let guard = env_test_lock()
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let tmux = std::env::var_os("TMUX");
            let tmux_pane = std::env::var_os("TMUX_PANE");
            std::env::remove_var("TMUX");
            std::env::remove_var("TMUX_PANE");
            Self {
                tmux,
                tmux_pane,
                _lock: guard,
            }
        }
    }

    impl Drop for TmuxOpenEnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.tmux.take() {
                std::env::set_var("TMUX", value);
            } else {
                std::env::remove_var("TMUX");
            }
            if let Some(value) = self.tmux_pane.take() {
                std::env::set_var("TMUX_PANE", value);
            } else {
                std::env::remove_var("TMUX_PANE");
            }
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn open_runner() -> OpenFakeRunner {
        OpenFakeRunner {
            current_window: "1".to_owned(),
            current_pane: "%9".to_owned(),
            windows: "0:1\n1:2\n2:1\n3:3\n".to_owned(),
            ..OpenFakeRunner::default()
        }
    }

    #[test]
    fn tmux_open_fragment_is_part284_only() {
        assert!(DISPATCH_284.is_empty());
        assert_eq!(TMUX_SUB_284.len(), 1);
        assert_eq!(TMUX_SUB_284[0].names, &["open"]);
    }

    #[test]
    fn tmux_open_no_target_joins_single_pane_hidden_windows_to_current_pane() {
        let _env = TmuxOpenEnvGuard::in_tmux(Some("%9"));
        let mut runner = open_runner();
        let out = tmux_open_with_runner(&[], &mut runner).expect("open hidden panes");
        assert_eq!(out, "\x1b[32m✓\x1b[0m opened 2 hidden panes\n");
        assert_eq!(runner.calls[0], ("display-message".to_owned(), strings(&["-p", "#{window_index}"])));
        assert_eq!(runner.calls[1], ("list-windows".to_owned(), strings(&["-F", "#{window_index}:#{window_panes}"])));
        assert_eq!(runner.calls[2], ("join-pane".to_owned(), strings(&["-h", "-s", ":0", "-t", "%9"])));
        assert_eq!(runner.calls[3], ("join-pane".to_owned(), strings(&["-h", "-s", ":2", "-t", "%9"])));
    }

    #[test]
    fn tmux_open_no_target_reports_no_hidden_panes() {
        let _env = TmuxOpenEnvGuard::in_tmux(Some("%9"));
        let mut runner = OpenFakeRunner {
            current_window: "1".to_owned(),
            windows: "1:2\n3:3\n".to_owned(),
            ..OpenFakeRunner::default()
        };
        let out = tmux_open_with_runner(&[], &mut runner).expect("no hidden panes");
        assert_eq!(out, "\x1b[90mno hidden panes to open\x1b[0m\n");
        assert_eq!(runner.calls.len(), 2);
    }

    #[test]
    fn tmux_open_target_uses_native_split_primitive_without_command() {
        let _env = TmuxOpenEnvGuard::in_tmux(Some("%9"));
        let mut runner = open_runner();
        let out = tmux_open_with_runner(&strings(&["%42"]), &mut runner).expect("open target");
        assert_eq!(out, "\x1b[32m✓\x1b[0m opened %42\n");
        assert_eq!(runner.calls, vec![("split-window".to_owned(), strings(&["-h", "-l", "50%", "-t", "%42"]))]);
    }

    #[test]
    fn tmux_open_requires_tmux_before_runner() {
        let _env = TmuxOpenEnvGuard::outside_tmux();
        let mut runner = open_runner();
        let err = tmux_open_with_runner(&strings(&["%42"]), &mut runner).expect_err("tmux required");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("not in tmux"));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn tmux_open_rejects_leading_dash_before_runner() {
        let _env = TmuxOpenEnvGuard::in_tmux(Some("%9"));
        let mut runner = open_runner();
        let err = tmux_open_with_runner(&strings(&["-oProxyCommand=bad"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("unknown argument"));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn tmux_open_rejects_control_target_before_runner() {
        let _env = TmuxOpenEnvGuard::in_tmux(Some("%9"));
        let mut runner = open_runner();
        let err = tmux_open_with_runner(&strings(&["bad\ntarget"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("control"));
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn tmux_open_falls_back_to_display_current_pane_when_env_pane_missing() {
        let _env = TmuxOpenEnvGuard::in_tmux(None);
        let mut runner = open_runner();
        let out = tmux_open_with_runner(&[], &mut runner).expect("open hidden panes");
        assert!(out.contains("opened 2 hidden panes"));
        assert_eq!(runner.calls[1], ("display-message".to_owned(), strings(&["-p", "#{pane_id}"])));
    }

    #[test]
    fn tmux_open_fake_maw_no_delegate_and_no_bun_runtime() {
        let _env = TmuxOpenEnvGuard::in_tmux(Some("%9"));
        let _restore_ref_dir = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut runner = open_runner();
        let out = tmux_open_with_runner(&strings(&["session:1.0"]), &mut runner).expect("open target");
        assert_eq!(out, "\x1b[32m✓\x1b[0m opened session:1.0\n");
        assert!(runner.calls.iter().all(|(subcommand, _)| subcommand != "bun"));
    }
}
