const DISPATCH_81: &[DispatcherEntry] = &[
    DispatcherEntry { command: "take", handler: Handler::Sync(take_run_command) },
    DispatcherEntry { command: "handover", handler: Handler::Sync(take_run_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct TakeOptions { source: String, target: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TakeSource { session: String, window: String }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TakeSession { name: String, windows: Vec<TakeWindow> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TakeWindow { index: u32, name: String }

fn take_run_command(argv: &[String]) -> CliOutput {
    match take_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) => output,
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn take_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, String> {
    let options = take_parse_args(argv)?;
    let source = take_parse_source(&options.source)?;
    let split = options.target.is_none();
    let target = take_target_session(&source.window, options.target.as_deref())?;
    if split { take_create_session(runner, &target)?; }
    if target == source.session { return Ok(take_same_session_output()); }
    let sessions = take_list_sessions(runner)?;
    let src_session = take_find_session(&sessions, &source.session)?;
    let src_window = take_resolve_source_window(&src_session.windows, &source.window)
        .ok_or_else(|| format!("window '{}' not found in session '{}'", source.window, source.session))?;
    let source_target = take_window_target(&src_session.name, &src_window.name)?;
    let pane_cwd = take_pane_cwd(runner, &source_target).unwrap_or_default();
    take_move_window(runner, &source_target, &target)?;
    if split { take_kill_default_window(runner, &target); }
    Ok(take_success_output(&src_session.name, &src_window.name, &target, split, &pane_cwd))
}

fn take_parse_args(argv: &[String]) -> Result<TakeOptions, String> {
    if argv.is_empty() { return Err(take_usage()); }
    if argv.iter().any(|arg| arg == "--") { return Err("take does not accept -- separator".to_owned()); }
    if argv.len() > 2 { return Err(take_usage()); }
    let source = take_validate_cli_value(&argv[0], "source")?;
    let target = argv.get(1).map(|value| take_validate_cli_value(value, "target-session")).transpose()?;
    Ok(TakeOptions { source, target })
}

fn take_validate_cli_value(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("take {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("take {label} must not contain whitespace or control characters"));
    }
    Ok(value.to_owned())
}

fn take_usage() -> String {
    "usage: maw take <session>:<window> [target-session]".to_owned()
}

fn take_parse_source(source: &str) -> Result<TakeSource, String> {
    let Some((session, window)) = source.split_once(':') else { return Err(take_usage_with_example()); };
    take_validate_tmux_target_part(session, "source session")?;
    take_validate_tmux_target_part(window, "source window")?;
    Ok(TakeSource { session: session.to_owned(), window: window.to_owned() })
}

fn take_usage_with_example() -> String {
    "usage: maw take <session>:<window> [target-session]\n  e.g. maw take neo:neo-skills pulse".to_owned()
}

fn take_target_session(source_window: &str, target: Option<&str>) -> Result<String, String> {
    let target = target.map_or_else(|| take_default_split_target_name(source_window), ToOwned::to_owned);
    take_validate_tmux_target_part(&target, "target session")?;
    Ok(target)
}

fn take_strip_tmux_display_suffix(window: &str) -> Option<&str> {
    (window.ends_with('-') && window.len() > 1).then(|| &window[..window.len() - 1])
}

fn take_default_split_target_name(window: &str) -> String {
    take_strip_tmux_display_suffix(window).unwrap_or(window).to_owned()
}

fn take_create_session<R: maw_tmux::TmuxRunner>(runner: &mut R, target: &str) -> Result<(), String> {
    take_validate_tmux_target_part(target, "target session")?;
    match runner.run("new-session", &["-d".to_owned(), "-s".to_owned(), target.to_owned()]) {
        Ok(_) => Ok(()),
        Err(error) if error.message.contains("duplicate") => Ok(()),
        Err(error) => Err(format!("could not create session '{target}': {}", error.message)),
    }
}

fn take_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<TakeSession>, String> {
    let raw = runner
        .run(
            "list-windows",
            &["-a".to_owned(), "-F".to_owned(), "#{session_name}\t#{window_index}\t#{window_name}".to_owned()],
        )
        .map_err(|error| format!("list-windows failed: {}", error.message))?;
    Ok(take_parse_sessions(&raw))
}

fn take_parse_sessions(raw: &str) -> Vec<TakeSession> {
    let mut sessions = Vec::<TakeSession>::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or_default();
        let index = parts.next().and_then(|value| value.parse::<u32>().ok()).unwrap_or(0);
        let window = parts.next().unwrap_or_default();
        let item = TakeWindow { index, name: window.to_owned() };
        if let Some(session) = sessions.iter_mut().find(|session| session.name == name) {
            session.windows.push(item);
        } else {
            sessions.push(TakeSession { name: name.to_owned(), windows: vec![item] });
        }
    }
    sessions
}

fn take_find_session<'a>(sessions: &'a [TakeSession], requested: &str) -> Result<&'a TakeSession, String> {
    sessions
        .iter()
        .find(|session| session.name.eq_ignore_ascii_case(requested))
        .ok_or_else(|| format!("session '{requested}' not found"))
}

fn take_resolve_source_window(windows: &[TakeWindow], requested: &str) -> Option<TakeWindow> {
    take_exact_window(windows, requested).or_else(|| {
        let canonical = take_strip_tmux_display_suffix(requested)?;
        take_exact_window(windows, canonical)
    })
}

fn take_exact_window(windows: &[TakeWindow], requested: &str) -> Option<TakeWindow> {
    windows
        .iter()
        .find(|window| window.name.eq_ignore_ascii_case(requested) || window.index.to_string() == requested)
        .cloned()
}

fn take_window_target(session: &str, window: &str) -> Result<String, String> {
    take_validate_tmux_target_part(session, "source session")?;
    take_validate_tmux_target_part(window, "source window")?;
    Ok(format!("{session}:{window}"))
}

fn take_pane_cwd<R: maw_tmux::TmuxRunner>(runner: &mut R, source_target: &str) -> Result<String, String> {
    take_validate_tmux_target(source_target)?;
    runner
        .run("display-message", &["-t".to_owned(), source_target.to_owned(), "-p".to_owned(), "#{pane_current_path}".to_owned()])
        .map(|raw| raw.trim().to_owned())
        .map_err(|error| error.message)
}

fn take_move_window<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    source_target: &str,
    target_session: &str,
) -> Result<(), String> {
    take_validate_tmux_target(source_target)?;
    take_validate_tmux_target_part(target_session, "target session")?;
    let dest = format!("{target_session}:");
    runner
        .run("move-window", &["-s".to_owned(), source_target.to_owned(), "-t".to_owned(), dest])
        .map(|_| ())
        .map_err(|error| format!("move failed: {}", error.message))
}

fn take_kill_default_window<R: maw_tmux::TmuxRunner>(runner: &mut R, target_session: &str) {
    if take_validate_tmux_target_part(target_session, "target session").is_err() { return; }
    let _ = runner.run("kill-window", &["-t".to_owned(), format!("{target_session}:1")]);
}

fn take_same_session_output() -> CliOutput {
    CliOutput { code: 0, stdout: "  \x1b[33m⚠\x1b[0m source and target are the same session\n".to_owned(), stderr: String::new() }
}

fn take_success_output(
    source_session: &str,
    source_window: &str,
    target: &str,
    split: bool,
    pane_cwd: &str,
) -> CliOutput {
    let suffix = if split { " (new session)" } else { "" };
    let mut stdout = format!("  \x1b[32m✓\x1b[0m {source_session}:{source_window} → {target}{suffix}\n");
    if !pane_cwd.is_empty() { let _ = writeln!(stdout, "  \x1b[90m  cwd: {pane_cwd}\x1b[0m"); }
    CliOutput { code: 0, stdout, stderr: String::new() }
}

fn take_validate_tmux_target_part(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("take {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("take {label} must not contain whitespace or control characters"));
    }
    Ok(())
}

fn take_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("tmux target/session must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod take_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct TakeMockTmux {
        calls: Vec<(String, Vec<String>)>,
        windows: String,
        cwd: String,
        fail_new: Option<String>,
        fail_move: bool,
    }

    impl maw_tmux::TmuxRunner for TakeMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "new-session" => match self.fail_new.clone() {
                    Some(message) => Err(maw_tmux::TmuxError::new(message)),
                    None => Ok(String::new()),
                },
                "list-windows" => Ok(self.windows.clone()),
                "display-message" => Ok(self.cwd.clone()),
                "move-window" if self.fail_move => Err(maw_tmux::TmuxError::new("bad move")),
                "move-window" | "kill-window" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    struct TakeEnvGuard { saved: Vec<(&'static str, Option<std::ffi::OsString>)> }

    impl TakeEnvGuard {
        fn new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "MAW_CONFIG_DIR", "TMUX", "PATH"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-take-test-{}", std::process::id()));
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

    impl Drop for TakeEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn take_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn take_dispatch_registers_take_and_handover() {
        assert_eq!(DISPATCH_81.len(), 2);
        assert_eq!(DISPATCH_81[0].command, "take");
        assert_eq!(DISPATCH_81[1].command, "handover");
    }

    #[test]
    fn take_moves_to_explicit_target_and_prints_cwd() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = TakeEnvGuard::new();
        let mut tmux = TakeMockTmux { windows: "neo\t2\tneo-skills\n".to_owned(), cwd: "/repo\n".to_owned(), ..Default::default() };

        let output = take_with_runner(&take_strings(&["neo:neo-skills", "pulse"]), &mut tmux).expect("take");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m neo:neo-skills → pulse\n  \x1b[90m  cwd: /repo\x1b[0m\n");
        assert_eq!(tmux.calls[0], ("list-windows".to_owned(), take_strings(&["-a", "-F", "#{session_name}\t#{window_index}\t#{window_name}"])));
        assert_eq!(tmux.calls[2], ("move-window".to_owned(), take_strings(&["-s", "neo:neo-skills", "-t", "pulse:"])));
    }

    #[test]
    fn take_split_creates_session_and_kills_default_window() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = TakeEnvGuard::new();
        let mut tmux = TakeMockTmux { windows: "neo\t3\tskills\n".to_owned(), ..Default::default() };

        let output = take_with_runner(&take_strings(&["neo:skills"]), &mut tmux).expect("take");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m neo:skills → skills (new session)\n");
        assert_eq!(tmux.calls[0], ("new-session".to_owned(), take_strings(&["-d", "-s", "skills"])));
        assert_eq!(tmux.calls[4], ("kill-window".to_owned(), take_strings(&["-t", "skills:1"])));
    }

    #[test]
    fn take_resolves_index_and_strips_display_suffix_for_split_name() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = TakeEnvGuard::new();
        let mut tmux = TakeMockTmux { windows: "neo\t4\tskills\n".to_owned(), ..Default::default() };

        let output = take_with_runner(&take_strings(&["neo:4-"]), &mut tmux).expect("take");

        assert!(output.stdout.contains("neo:skills → 4 (new session)"));
        assert_eq!(tmux.calls[0].1, take_strings(&["-d", "-s", "4"]));
        assert_eq!(tmux.calls[3].1, take_strings(&["-s", "neo:skills", "-t", "4:"]));
    }

    #[test]
    fn take_rejects_leading_dash_and_separator_before_tmux() {
        let mut tmux = TakeMockTmux::default();
        let err = take_with_runner(&take_strings(&["--", "neo:1"]), &mut tmux).expect_err("guard");
        assert!(err.contains("-- separator"));
        assert!(tmux.calls.is_empty());
        let err = take_with_runner(&take_strings(&["-Sbad:1"]), &mut tmux).expect_err("guard");
        assert!(err.contains("not start with '-'"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn take_rejects_bad_source_parts_before_tmux() {
        let mut tmux = TakeMockTmux::default();
        let err = take_with_runner(&take_strings(&["neo:-bad"]), &mut tmux).expect_err("guard");
        assert!(err.contains("source window"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn take_same_session_returns_warning_after_split_creation() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = TakeEnvGuard::new();
        let mut tmux = TakeMockTmux::default();

        let output = take_with_runner(&take_strings(&["neo:work", "neo"]), &mut tmux).expect("same");

        assert_eq!(output.stdout, "  \x1b[33m⚠\x1b[0m source and target are the same session\n");
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn take_duplicate_new_session_is_ignored_but_move_failure_is_reported() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = TakeEnvGuard::new();
        let mut tmux = TakeMockTmux { windows: "neo\t0\twork\n".to_owned(), fail_new: Some("duplicate session".to_owned()), fail_move: true, ..Default::default() };

        let err = take_with_runner(&take_strings(&["neo:work"]), &mut tmux).expect_err("move");

        assert_eq!(err, "move failed: bad move");
        assert_eq!(tmux.calls[0].0, "new-session");
        assert_eq!(tmux.calls[3].0, "move-window");
    }
}
