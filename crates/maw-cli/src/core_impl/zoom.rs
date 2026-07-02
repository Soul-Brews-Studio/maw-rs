const DISPATCH_80: &[DispatcherEntry] = &[DispatcherEntry {
    command: "zoom",
    handler: Handler::Sync(zoom_run_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZoomOptions {
    target: String,
    pane: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZoomSession {
    name: String,
    windows: Vec<ZoomWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ZoomWindow {
    index: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ZoomResolveKind {
    Exact,
    Fuzzy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ZoomResolveResult<'a> {
    None { hints: Vec<&'a ZoomSession> },
    Match { session: &'a ZoomSession, kind: ZoomResolveKind },
    Ambiguous { candidates: Vec<&'a ZoomSession> },
}

fn zoom_run_command(argv: &[String]) -> CliOutput {
    match zoom_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) => output,
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn zoom_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, String> {
    let options = zoom_parse_args(argv)?;
    let sessions = zoom_list_sessions(runner)?;
    let target = zoom_resolve_target(&options, &sessions)?;
    zoom_validate_tmux_target(&target)?;
    zoom_toggle(runner, &target)?;
    Ok(CliOutput {
        code: 0,
        stdout: format!("  \x1b[32m✓\x1b[0m toggled zoom on {target}\n"),
        stderr: String::new(),
    })
}

fn zoom_parse_args(argv: &[String]) -> Result<ZoomOptions, String> {
    let mut rest = argv.iter().peekable();
    let mut positionals = Vec::new();
    let mut pane = None;
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--help" | "-h" if positionals.is_empty() => return Err(zoom_usage()),
            "--" => {
                positionals.extend(rest.cloned());
                break;
            }
            "--pane" => pane = Some(zoom_parse_pane(rest.next())?),
            value if value.starts_with("--pane=") => pane = Some(zoom_parse_pane_value(&value[7..])?),
            value if value.starts_with('-') && positionals.is_empty() => {
                return Err(zoom_flag_like_target(value));
            }
            value if value.starts_with('-') => return Err(format!("unknown zoom flag '{value}'")),
            value => positionals.push(value.to_owned()),
        }
    }
    let Some(target) = positionals.first().cloned() else { return Err(zoom_usage()); };
    if target.starts_with('-') || target == "--" {
        return Err(zoom_flag_like_target(&target));
    }
    zoom_validate_parse_target(&target)?;
    Ok(ZoomOptions { target, pane })
}

fn zoom_validate_parse_target(target: &str) -> Result<(), String> {
    let (left, suffix) = zoom_split_target(target)?;
    zoom_validate_query(&left)?;
    if let Some(suffix) = suffix { zoom_validate_suffix(&suffix)?; }
    Ok(())
}

fn zoom_parse_pane(value: Option<&String>) -> Result<u32, String> {
    let Some(value) = value else { return Err("--pane requires a positive number".to_owned()); };
    zoom_parse_pane_value(value)
}

fn zoom_parse_pane_value(value: &str) -> Result<u32, String> {
    if value.is_empty() || value.starts_with('-') || value == "--" {
        return Err("--pane requires a positive number".to_owned());
    }
    value
        .parse::<u32>()
        .map_err(|_| "--pane requires a positive number".to_owned())
}

fn zoom_usage() -> String {
    "usage: maw zoom <target> [--pane N]".to_owned()
}

fn zoom_flag_like_target(target: &str) -> String {
    format!("\"{target}\" looks like a flag, not a target.\n  usage: maw zoom <target>")
}

fn zoom_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<ZoomSession>, String> {
    let raw = runner
        .run(
            "list-windows",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}\t#{window_index}".to_owned(),
            ],
        )
        .map_err(|error| format!("zoom failed: {}", error.message))?;
    Ok(zoom_parse_sessions(&raw))
}

fn zoom_parse_sessions(raw: &str) -> Vec<ZoomSession> {
    let mut sessions = Vec::<ZoomSession>::new();
    for line in raw.lines().filter(|line| !line.is_empty()) {
        let mut parts = line.splitn(2, '\t');
        let name = parts.next().unwrap_or_default();
        let index = parts.next().and_then(|value| value.parse::<u32>().ok()).unwrap_or(0);
        if let Some(session) = sessions.iter_mut().find(|session| session.name == name) {
            session.windows.push(ZoomWindow { index });
        } else {
            sessions.push(ZoomSession { name: name.to_owned(), windows: vec![ZoomWindow { index }] });
        }
    }
    sessions
}

fn zoom_resolve_target(options: &ZoomOptions, sessions: &[ZoomSession]) -> Result<String, String> {
    let (raw_session, suffix) = zoom_split_target(&options.target)?;
    zoom_validate_query(&raw_session)?;
    let session = zoom_resolve_or_error(&raw_session, sessions)?;
    let window = suffix.unwrap_or_else(|| zoom_default_window(session));
    let mut target = format!("{}:{window}", session.name);
    if let Some(pane) = options.pane {
        let _ = write!(target, ".{pane}");
    }
    Ok(target)
}

fn zoom_split_target(target: &str) -> Result<(String, Option<String>), String> {
    let Some((left, right)) = target.split_once(':') else { return Ok((target.to_owned(), None)); };
    zoom_validate_suffix(right)?;
    Ok((left.to_owned(), Some(right.to_owned())))
}

fn zoom_validate_suffix(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err("zoom tmux window suffix must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("zoom tmux window suffix must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn zoom_resolve_or_error<'a>(
    raw: &str,
    sessions: &'a [ZoomSession],
) -> Result<&'a ZoomSession, String> {
    match zoom_resolve_session_target(raw, sessions) {
        ZoomResolveResult::Match { session, .. } => Ok(session),
        ZoomResolveResult::Ambiguous { candidates } => Err(zoom_ambiguous_error(raw, &candidates)),
        ZoomResolveResult::None { hints } => Err(zoom_not_found_error(raw, &hints)),
    }
}

fn zoom_resolve_session_target<'a>(
    target: &str,
    sessions: &'a [ZoomSession],
) -> ZoomResolveResult<'a> {
    let lc = target.trim().to_ascii_lowercase();
    if lc.is_empty() { return ZoomResolveResult::None { hints: Vec::new() }; }
    if let Some(session) = sessions.iter().find(|session| session.name.eq_ignore_ascii_case(&lc)) {
        return ZoomResolveResult::Match { session, kind: ZoomResolveKind::Exact };
    }
    let suffix = sessions.iter().filter(|session| zoom_suffix_match(&session.name, &lc)).collect::<Vec<_>>();
    if let Some(result) = zoom_pick_fuzzy(&suffix) { return result; }
    let mid = sessions.iter().filter(|session| zoom_prefix_mid_match(&session.name, &lc)).collect::<Vec<_>>();
    if let Some(result) = zoom_pick_fuzzy(&mid) { return result; }
    let hints = sessions.iter().filter(|session| session.name.to_ascii_lowercase().contains(&lc)).collect();
    ZoomResolveResult::None { hints }
}

fn zoom_pick_fuzzy<'a>(matches: &[&'a ZoomSession]) -> Option<ZoomResolveResult<'a>> {
    match matches {
        [] => None,
        [session] => Some(ZoomResolveResult::Match { session, kind: ZoomResolveKind::Fuzzy }),
        _ => Some(ZoomResolveResult::Ambiguous { candidates: matches.to_vec() }),
    }
}

fn zoom_suffix_match(name: &str, lc: &str) -> bool {
    name.to_ascii_lowercase().ends_with(&format!("-{lc}"))
}

fn zoom_prefix_mid_match(name: &str, lc: &str) -> bool {
    let name = name.to_ascii_lowercase();
    !zoom_numbered(&name) && (name.starts_with(&format!("{lc}-")) || name.contains(&format!("-{lc}-")))
}

fn zoom_numbered(name: &str) -> bool {
    name.as_bytes().first().is_some_and(u8::is_ascii_digit) && name.contains('-')
}

fn zoom_default_window(session: &ZoomSession) -> String {
    session.windows.first().map_or_else(|| "0".to_owned(), |window| window.index.to_string())
}

fn zoom_ambiguous_error(target: &str, candidates: &[&ZoomSession]) -> String {
    let mut message = format!("  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} sessions:", candidates.len());
    for candidate in candidates {
        let _ = write!(message, "\n  \x1b[90m    • {}\x1b[0m", candidate.name);
    }
    let _ = write!(message, "\n'{target}' is ambiguous — matches {} sessions", candidates.len());
    message
}

fn zoom_not_found_error(target: &str, hints: &[&ZoomSession]) -> String {
    let mut message = String::new();
    if hints.is_empty() {
        message.push_str("  \x1b[90m  try: maw ls\x1b[0m\n");
    } else {
        message.push_str("  \x1b[90m  did you mean:\x1b[0m");
        for hint in hints {
            let _ = write!(message, "\n  \x1b[90m    • {}\x1b[0m", hint.name);
        }
        message.push('\n');
    }
    let _ = write!(message, "session '{target}' not found");
    message
}

fn zoom_validate_query(query: &str) -> Result<(), String> {
    if query.is_empty() || query.trim() != query || query.starts_with('-') || query == "--" {
        Err("zoom target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else if query.chars().any(char::is_control) {
        Err("zoom target must not contain control characters".to_owned())
    } else {
        Ok(())
    }
}

fn zoom_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') || target == "--" {
        return Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if target.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("tmux target/session must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn zoom_toggle<R: maw_tmux::TmuxRunner>(runner: &mut R, target: &str) -> Result<(), String> {
    zoom_validate_tmux_target(target)?;
    runner
        .run("resize-pane", &["-Z".to_owned(), "-t".to_owned(), target.to_owned()])
        .map(|_| ())
        .map_err(|error| format!("zoom failed: {}", error.message))
}

#[cfg(test)]
mod zoom_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct ZoomMockTmux {
        calls: Vec<(String, Vec<String>)>,
        windows: String,
        fail_zoom: bool,
    }

    impl maw_tmux::TmuxRunner for ZoomMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-windows" => Ok(self.windows.clone()),
                "resize-pane" if self.fail_zoom => Err(maw_tmux::TmuxError::new("no pane")),
                "resize-pane" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    struct ZoomEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl ZoomEnvGuard {
        fn new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "MAW_CONFIG_DIR", "TMUX", "PATH"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-zoom-test-{}", std::process::id()));
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

    impl Drop for ZoomEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn zoom_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn zoom_dispatch_registers_single_native_command() {
        assert_eq!(DISPATCH_80.len(), 1);
        assert_eq!(DISPATCH_80[0].command, "zoom");
    }

    #[test]
    fn zoom_default_window_toggles_with_safe_tmux_args() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = ZoomEnvGuard::new();
        let mut tmux = ZoomMockTmux { windows: "03-neo\t2\n".to_owned(), ..ZoomMockTmux::default() };

        let output = zoom_with_runner(&zoom_strings(&["neo"]), &mut tmux).expect("zoom");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m toggled zoom on 03-neo:2\n");
        assert_eq!(tmux.calls[0], ("list-windows".to_owned(), zoom_strings(&["-a", "-F", "#{session_name}\t#{window_index}"])));
        assert_eq!(tmux.calls[1], ("resize-pane".to_owned(), zoom_strings(&["-Z", "-t", "03-neo:2"])));
    }

    #[test]
    fn zoom_explicit_window_and_pane_are_preserved() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = ZoomEnvGuard::new();
        let mut tmux = ZoomMockTmux { windows: "03-neo\t0\n".to_owned(), ..ZoomMockTmux::default() };
        let args = zoom_strings(&["neo:1", "--pane", "3"]);

        let output = zoom_with_runner(&args, &mut tmux).expect("zoom");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m toggled zoom on 03-neo:1.3\n");
        assert_eq!(tmux.calls[1].1, zoom_strings(&["-Z", "-t", "03-neo:1.3"]));
    }

    #[test]
    fn zoom_rejects_leading_dash_target_before_tmux() {
        let mut tmux = ZoomMockTmux::default();
        let error = zoom_with_runner(&zoom_strings(&["--", "-Sbad"]), &mut tmux).expect_err("guard");
        assert!(error.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn zoom_rejects_bad_suffix_before_tmux() {
        let mut tmux = ZoomMockTmux::default();
        let error = zoom_with_runner(&zoom_strings(&["neo:-bad"]), &mut tmux).expect_err("guard");
        assert!(error.contains("window suffix"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn zoom_rejects_bad_pane_before_tmux() {
        let mut tmux = ZoomMockTmux::default();
        let error = zoom_with_runner(&zoom_strings(&["neo", "--pane", "--"]), &mut tmux).expect_err("guard");
        assert_eq!(error, "--pane requires a positive number");
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn zoom_reports_ambiguous_suffix_without_resize() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = ZoomEnvGuard::new();
        let mut tmux = ZoomMockTmux { windows: "01-neo\t0\n02-neo\t1\n".to_owned(), ..ZoomMockTmux::default() };

        let error = zoom_with_runner(&zoom_strings(&["neo"]), &mut tmux).expect_err("ambiguous");

        assert!(error.contains("'neo' is ambiguous — matches 2 sessions"));
        assert_eq!(tmux.calls.len(), 1);
    }

    #[test]
    fn zoom_reports_failure_after_validated_target() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = ZoomEnvGuard::new();
        let mut tmux = ZoomMockTmux { windows: "neo\t0\n".to_owned(), fail_zoom: true, ..ZoomMockTmux::default() };

        let error = zoom_with_runner(&zoom_strings(&["neo"]), &mut tmux).expect_err("fail");

        assert_eq!(error, "zoom failed: no pane");
        assert_eq!(tmux.calls[1].1, zoom_strings(&["-Z", "-t", "neo:0"]));
    }

    #[test]
    fn zoom_validate_rejects_bad_resolved_tmux_target() {
        let error = zoom_validate_tmux_target("neo:bad pane").expect_err("guard");
        assert!(error.contains("whitespace"));
    }
}
