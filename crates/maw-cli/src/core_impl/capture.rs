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

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureSession {
    name: String,
    windows: Vec<CaptureWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureWindow {
    index: u32,
    name: String,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
struct CaptureFleetEntry {
    name: String,
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
    let sessions = capture_list_sessions(runner)?;
    let target = capture_resolve_target(&options, &sessions, &capture_load_fleet())?;
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

fn capture_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<CaptureSession>, String> {
    let raw = runner
        .run(
            "list-windows",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}\t#{window_index}\t#{window_name}".to_owned(),
            ],
        )
        .map_err(|error| format!("capture failed: {}", error.message))?;
    Ok(capture_parse_sessions(&raw))
}

fn capture_parse_sessions(raw: &str) -> Vec<CaptureSession> {
    let mut sessions = Vec::<CaptureSession>::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let mut parts = line.splitn(3, '\t');
        let name = parts.next().unwrap_or_default();
        let index = parts.next().and_then(|value| value.parse::<u32>().ok()).unwrap_or(0);
        let window = parts.next().unwrap_or_default();
        if let Some(session) = sessions.iter_mut().find(|session| session.name == name) {
            session.windows.push(CaptureWindow { index, name: window.to_owned() });
        } else {
            sessions.push(CaptureSession {
                name: name.to_owned(),
                windows: vec![CaptureWindow { index, name: window.to_owned() }],
            });
        }
    }
    sessions
}

fn capture_load_fleet() -> Vec<CaptureFleetEntry> {
    let fleet_dir = active_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(fleet_dir) else { return Vec::new(); };
    entries
        .flatten()
        .filter_map(|entry| std::fs::read_to_string(entry.path()).ok())
        .filter_map(|text| serde_json::from_str::<CaptureFleetEntry>(&text).ok())
        .collect()
}

fn capture_resolve_target(
    options: &CaptureOptions,
    sessions: &[CaptureSession],
    fleet: &[CaptureFleetEntry],
) -> Result<String, String> {
    let (raw_session, explicit_window) = capture_split_target(&options.target);
    capture_validate_query(&raw_session)?;
    let session = capture_resolve_session(&raw_session, sessions, fleet)?;
    let window = explicit_window.unwrap_or_else(|| capture_default_window(session));
    let mut target = format!("{}:{window}", session.name);
    if let Some(pane) = options.pane {
        let _ = write!(target, ".{pane}");
    }
    Ok(target)
}

fn capture_split_target(target: &str) -> (String, Option<String>) {
    let Some((left, right)) = target.split_once(':') else { return (target.to_owned(), None); };
    if capture_is_tmux_window_suffix(right) {
        (left.to_owned(), Some(right.to_owned()))
    } else {
        (right.to_owned(), None)
    }
}

fn capture_is_tmux_window_suffix(value: &str) -> bool {
    let Some((left, right)) = value.split_once('.') else {
        return !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
    };
    !left.is_empty()
        && !right.is_empty()
        && left.bytes().all(|byte| byte.is_ascii_digit())
        && right.bytes().all(|byte| byte.is_ascii_digit())
}

fn capture_resolve_session<'a>(
    raw_session: &str,
    sessions: &'a [CaptureSession],
    fleet: &[CaptureFleetEntry],
) -> Result<&'a CaptureSession, String> {
    let matches = sessions
        .iter()
        .filter(|session| capture_running_match(session, raw_session))
        .collect::<Vec<_>>();
    capture_pick_session(raw_session, &matches, fleet).ok_or_else(|| {
        format!("session '{raw_session}' not found\n  \x1b[90m  try: maw ls\x1b[0m")
    })
}

fn capture_pick_session<'a>(
    target: &str,
    matches: &[&'a CaptureSession],
    fleet: &[CaptureFleetEntry],
) -> Option<&'a CaptureSession> {
    match matches {
        [] => None,
        [single] => Some(*single),
        _ => capture_exact_session(target, matches).or_else(|| capture_trusted_session(matches, fleet)),
    }
}

fn capture_exact_session<'a>(target: &str, matches: &[&'a CaptureSession]) -> Option<&'a CaptureSession> {
    matches.iter().copied().find(|session| session.name.eq_ignore_ascii_case(target))
}

fn capture_trusted_session<'a>(
    matches: &[&'a CaptureSession],
    fleet: &[CaptureFleetEntry],
) -> Option<&'a CaptureSession> {
    let registered = matches
        .iter()
        .copied()
        .filter(|session| fleet.iter().any(|entry| entry.name.eq_ignore_ascii_case(&session.name)))
        .collect::<Vec<_>>();
    if registered.len() == 1 { return Some(registered[0]); }
    let numbered = matches.iter().copied().filter(|session| capture_numbered(&session.name)).collect::<Vec<_>>();
    (numbered.len() == 1).then_some(numbered[0])
}

fn capture_running_match(session: &CaptureSession, target: &str) -> bool {
    capture_name_matches(&session.name, target)
        || session.windows.iter().any(|window| capture_exact_oracle_window(&window.name, target))
}

fn capture_name_matches(name: &str, target: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let t = target.to_ascii_lowercase();
    n == t
        || n.ends_with(&format!("-{t}"))
        || n == format!("{t}-oracle")
        || n.ends_with(&format!("-{t}-oracle"))
        || capture_strip_dash(&n) == capture_strip_dash(&t)
        || capture_legacy_dashless_match(&n, &t)
}

fn capture_exact_oracle_window(name: &str, target: &str) -> bool {
    let n = name.to_ascii_lowercase();
    let t = target.to_ascii_lowercase();
    n == t || n == format!("{t}-oracle") || n.ends_with(&format!("-{t}-oracle"))
}

fn capture_strip_dash(value: &str) -> &str { value.trim_end_matches('-') }

fn capture_legacy_dashless_match(name: &str, target: &str) -> bool {
    target.contains('-')
        && capture_strip_fleet_oracle(name).replace('-', "")
            == capture_strip_fleet_oracle(target).replace('-', "")
}

fn capture_strip_fleet_oracle(value: &str) -> String {
    let value = value.trim_start_matches(|ch: char| ch.is_ascii_digit()).trim_start_matches('-');
    value.strip_suffix("-oracle").unwrap_or(value).to_owned()
}

fn capture_numbered(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_digit) && name.contains('-')
}

fn capture_default_window(session: &CaptureSession) -> String {
    session.windows.first().map_or_else(|| "0".to_owned(), |window| window.index.to_string())
}

fn capture_validate_query(query: &str) -> Result<(), String> {
    if query.is_empty() || query.trim() != query || query.starts_with('-') || query == "--" {
        Err("capture target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
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
            windows: "03-neo\t2\tmain\n".to_owned(),
            capture: "hello\n".to_owned(),
            ..CaptureMockTmux::default()
        };

        let output = capture_with_runner(&capture_strings(&["neo"]), &mut tmux).expect("capture");

        assert_eq!(output.stdout, "hello\n");
        assert_eq!(tmux.calls[0], ("list-windows".to_owned(), capture_strings(&["-a", "-F", "#{session_name}\t#{window_index}\t#{window_name}"])));
        assert_eq!(tmux.calls[1], ("capture-pane".to_owned(), capture_strings(&["-t", "03-neo:2", "-p", "-S", "-50"])));
    }

    #[test]
    fn capture_full_and_pane_override_lines() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = CaptureEnvGuard::new();
        let mut tmux = CaptureMockTmux { windows: "neo\t0\tzsh\n".to_owned(), ..CaptureMockTmux::default() };
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
            windows: "03-neo\t0\tmain\n03-neo\t1\tneo-oracle\n".to_owned(),
            fail_capture: true,
            ..CaptureMockTmux::default()
        };

        let error = capture_with_runner(&capture_strings(&["neo-oracle"]), &mut tmux).expect_err("fail");

        assert_eq!(error, "capture failed: no pane");
        assert_eq!(tmux.calls[1].1, capture_strings(&["-t", "03-neo:0", "-p", "-S", "-50"]));
    }

    #[test]
    fn capture_validate_rejects_bad_resolved_tmux_target() {
        let error = capture_validate_tmux_target("neo:bad pane").expect_err("guard");
        assert!(error.contains("whitespace"));
    }
}
