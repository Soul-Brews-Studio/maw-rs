const DISPATCH_87: &[DispatcherEntry] = &[DispatcherEntry {
    command: "resume",
    handler: Handler::Sync(resume_run_command),
}];

const RESUME_USAGE: &str = "usage: maw resume — resume sleeping oracle fleet sessions";

type ResumeFleetLoader = fn() -> Vec<NativeFleetSession>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResumeWindow {
    name: String,
    repo: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResumeSession {
    name: String,
    windows: Vec<ResumeWindow>,
}

trait ResumeTmux {
    fn resume_list_live_sessions(&mut self) -> Result<Vec<String>, String>;
    fn resume_new_session(&mut self, session: &str, window: &ResumeWindow) -> Result<(), String>;
    fn resume_new_window(&mut self, session: &str, window: &ResumeWindow) -> Result<(), String>;
    fn resume_restore_tab_order(&mut self, session: &str) -> Result<(), String>;
}

struct ResumeSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

impl ResumeSystemTmux {
    fn resume_new() -> Self {
        Self {
            runner: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl ResumeTmux for ResumeSystemTmux {
    fn resume_list_live_sessions(&mut self) -> Result<Vec<String>, String> {
        match resume_tmux_run(&mut self.runner, "list-sessions", &["-F", "#{session_name}"]) {
            Ok(raw) => Ok(resume_parse_live_sessions(&raw)),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn resume_new_session(&mut self, session: &str, window: &ResumeWindow) -> Result<(), String> {
        resume_validate_tmux_target(session)?;
        resume_validate_window(window)?;
        resume_tmux_run_owned(&mut self.runner, "new-session", &resume_new_session_args(session, window)).map(|_| ())
    }

    fn resume_new_window(&mut self, session: &str, window: &ResumeWindow) -> Result<(), String> {
        resume_validate_tmux_target(session)?;
        resume_validate_window(window)?;
        resume_tmux_run_owned(&mut self.runner, "new-window", &resume_new_window_args(session, window)).map(|_| ())
    }

    fn resume_restore_tab_order(&mut self, session: &str) -> Result<(), String> {
        resume_validate_tmux_target(session)?;
        let names = resume_read_tab_order(session)?;
        for (target_index, window_name) in names.iter().enumerate() {
            resume_validate_tmux_target_part(window_name, "tab order window")?;
            let target = format!("{session}:{}", target_index + 1);
            let source = format!("{session}:{window_name}");
            resume_validate_tmux_target(&target)?;
            resume_validate_tmux_target(&source)?;
            let _ = resume_tmux_run_owned(&mut self.runner, "move-window", &["-s".to_owned(), source, "-t".to_owned(), target]);
        }
        Ok(())
    }
}

fn resume_run_command(argv: &[String]) -> CliOutput {
    resume_run_command_with(argv, &mut ResumeSystemTmux::resume_new(), load_native_fleet)
}

fn resume_run_command_with(
    argv: &[String],
    tmux: &mut impl ResumeTmux,
    load_fleet: ResumeFleetLoader,
) -> CliOutput {
    match resume_run(argv, tmux, load_fleet) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn resume_run(
    argv: &[String],
    tmux: &mut impl ResumeTmux,
    load_fleet: ResumeFleetLoader,
) -> Result<String, String> {
    resume_parse_args(argv)?;
    let sessions = resume_fleet_sessions(load_fleet())?;
    let live = tmux.resume_list_live_sessions()?;
    resume_validate_live_sessions(&live)?;
    let mut output = String::new();
    let mut resumed = 0usize;
    for session in resume_sleeping_sessions(&sessions, &live) {
        resume_start_session(tmux, &session)?;
        let _ = tmux.resume_restore_tab_order(&session.name);
        let _ = writeln!(output, "  \x1b[32m●\x1b[0m {} — awake", session.name);
        resumed += 1;
    }
    let _ = writeln!(output, "\n  {resumed} sessions resumed.\n");
    Ok(output)
}

fn resume_parse_args(argv: &[String]) -> Result<(), String> {
    let Some(arg) = argv.first() else { return Ok(()); };
    match arg.as_str() {
        "--help" | "-h" | "help" => Err(RESUME_USAGE.to_owned()),
        "--" => Err("resume: -- separator is not allowed".to_owned()),
        value if value.starts_with('-') => Err(resume_flag_like_target(value)),
        value => Err(format!("resume: unexpected argument {value}")),
    }
}

fn resume_flag_like_target(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a target.\n  {RESUME_USAGE}")
}

fn resume_fleet_sessions(sessions: Vec<NativeFleetSession>) -> Result<Vec<ResumeSession>, String> {
    let mut out = Vec::new();
    for session in sessions {
        resume_validate_user_target(&session.name)?;
        let windows = resume_windows_from_fleet(&session)?;
        out.push(ResumeSession { name: session.name, windows });
    }
    out.sort_by(|left, right| left.name.cmp(&right.name));
    out.dedup_by(|left, right| left.name == right.name);
    Ok(out)
}

fn resume_windows_from_fleet(session: &NativeFleetSession) -> Result<Vec<ResumeWindow>, String> {
    let mut windows = session.windows.iter().map(resume_window_from_fleet).collect::<Result<Vec<_>, _>>()?;
    if windows.is_empty() {
        windows.push(ResumeWindow { name: "oracle".to_owned(), repo: String::new() });
    }
    Ok(windows)
}

fn resume_window_from_fleet(window: &NativeFleetWindow) -> Result<ResumeWindow, String> {
    resume_validate_tmux_target_part(&window.name, "window")?;
    resume_validate_repo(&window.repo)?;
    Ok(ResumeWindow { name: window.name.clone(), repo: window.repo.clone() })
}

fn resume_sleeping_sessions(sessions: &[ResumeSession], live: &[String]) -> Vec<ResumeSession> {
    sessions.iter().filter(|session| !live.iter().any(|live| live == &session.name)).cloned().collect()
}

fn resume_start_session(tmux: &mut impl ResumeTmux, session: &ResumeSession) -> Result<(), String> {
    let Some((first, rest)) = session.windows.split_first() else { return Ok(()); };
    resume_validate_user_target(&session.name)?;
    tmux.resume_new_session(&session.name, first)?;
    for window in rest {
        tmux.resume_new_window(&session.name, window)?;
    }
    Ok(())
}

fn resume_new_session_args(session: &str, window: &ResumeWindow) -> Vec<String> {
    let mut args = vec!["-d".to_owned(), "-s".to_owned(), session.to_owned(), "-n".to_owned(), window.name.clone()];
    resume_append_cwd_args(&mut args, window);
    args
}

fn resume_new_window_args(session: &str, window: &ResumeWindow) -> Vec<String> {
    let mut args = vec!["-t".to_owned(), session.to_owned(), "-n".to_owned(), window.name.clone()];
    resume_append_cwd_args(&mut args, window);
    args
}

fn resume_append_cwd_args(args: &mut Vec<String>, window: &ResumeWindow) {
    if window.repo.is_empty() { return; }
    let cwd = ghq_root().join("github.com").join(&window.repo);
    args.extend(["-c".to_owned(), cwd.display().to_string()]);
}

fn resume_validate_live_sessions(sessions: &[String]) -> Result<(), String> {
    for session in sessions {
        resume_validate_tmux_target(session)?;
    }
    Ok(())
}

fn resume_validate_user_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("resume target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("resume target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn resume_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("resume tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("resume tmux target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn resume_validate_tmux_target_part(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err(format!("resume {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("resume {label} must not contain whitespace or control characters"));
    }
    Ok(())
}

fn resume_validate_window(window: &ResumeWindow) -> Result<(), String> {
    resume_validate_tmux_target_part(&window.name, "window")?;
    resume_validate_repo(&window.repo)
}

fn resume_validate_repo(repo: &str) -> Result<(), String> {
    if repo.is_empty() { return Ok(()); }
    if repo.starts_with('-') || repo.contains("..") || repo.starts_with('/') || repo.contains('\\') {
        return Err("resume repo must be a safe relative org/repo path".to_owned());
    }
    if repo.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("resume repo must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn resume_parse_live_sessions(raw: &str) -> Vec<String> {
    raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect()
}

fn resume_read_tab_order(session: &str) -> Result<Vec<String>, String> {
    resume_validate_tmux_target(session)?;
    let path = maw_state_path(&current_xdg_env(), &["tab-order", &format!("{session}.json")]);
    let text = std::fs::read_to_string(&path).map_err(|error| format!("resume: tab-order read failed: {error}"))?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("resume: tab-order json failed: {error}"))?;
    Ok(resume_tab_order_names(&value))
}

fn resume_tab_order_names(value: &serde_json::Value) -> Vec<String> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("name").and_then(serde_json::Value::as_str))
        .filter(|name| resume_validate_tmux_target_part(name, "tab order window").is_ok())
        .map(str::to_owned)
        .collect()
}

fn resume_tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[&str],
) -> Result<String, String> {
    resume_tmux_run_owned(runner, subcommand, &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn resume_tmux_run_owned<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[String],
) -> Result<String, String> {
    runner.run(subcommand, args).map_err(|error| error.message)
}

#[cfg(test)]
mod resume_tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum ResumeCall {
        ListLive,
        NewSession(String, String),
        NewWindow(String, String),
        Restore(String),
    }

    #[derive(Debug, Default)]
    struct ResumeFakeTmux {
        live: Vec<String>,
        calls: Vec<ResumeCall>,
        fail_new: Vec<String>,
    }

    impl ResumeTmux for ResumeFakeTmux {
        fn resume_list_live_sessions(&mut self) -> Result<Vec<String>, String> {
            self.calls.push(ResumeCall::ListLive);
            Ok(self.live.clone())
        }

        fn resume_new_session(&mut self, session: &str, window: &ResumeWindow) -> Result<(), String> {
            resume_validate_tmux_target(session)?;
            resume_validate_window(window)?;
            self.calls.push(ResumeCall::NewSession(session.to_owned(), window.name.clone()));
            if self.fail_new.iter().any(|name| name == session) { Err("new failed".to_owned()) } else { Ok(()) }
        }

        fn resume_new_window(&mut self, session: &str, window: &ResumeWindow) -> Result<(), String> {
            resume_validate_tmux_target(session)?;
            resume_validate_window(window)?;
            self.calls.push(ResumeCall::NewWindow(session.to_owned(), window.name.clone()));
            Ok(())
        }

        fn resume_restore_tab_order(&mut self, session: &str) -> Result<(), String> {
            resume_validate_tmux_target(session)?;
            self.calls.push(ResumeCall::Restore(session.to_owned()));
            Ok(())
        }
    }

    struct ResumeEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl ResumeEnvGuard {
        fn resume_new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "MAW_CONFIG_DIR", "MAW_STATE_DIR", "TMUX", "PATH", "GHQ_ROOT"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-resume-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("state/tab-order")).expect("state");
            std::env::set_var("HOME", root.join("home"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
            std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
            std::env::set_var("MAW_STATE_DIR", root.join("state"));
            std::env::set_var("TMUX", "fake-tmux-socket");
            std::env::set_var("PATH", root.join("bin"));
            std::env::set_var("GHQ_ROOT", root.join("ghq"));
            Self { saved }
        }
    }

    impl Drop for ResumeEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn resume_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn resume_session(name: &str, windows: &[(&str, &str)]) -> NativeFleetSession {
        NativeFleetSession {
            name: name.to_owned(),
            windows: windows.iter().map(|(name, repo)| NativeFleetWindow { name: (*name).to_owned(), repo: (*repo).to_owned() }).collect(),
            ..NativeFleetSession::default()
        }
    }

    fn resume_fleet() -> Vec<NativeFleetSession> {
        vec![
            resume_session("01-wish", &[("wish", "tonkmac/wish"), ("logs", "")]),
            resume_session("08-gm-bo", &[("guardian", "tonkmac/gmtk-oracle")]),
            resume_session("99-tonk", &[]),
        ]
    }

    fn resume_bad_fleet() -> Vec<NativeFleetSession> {
        vec![resume_session("-bad", &[("ok", "")])]
    }

    #[test]
    fn resume_dispatch_registers_resume() {
        assert_eq!(DISPATCH_87.len(), 1);
        assert_eq!(DISPATCH_87[0].command, "resume");
    }

    #[test]
    fn resume_starts_only_missing_configured_sessions() {
        let mut tmux = ResumeFakeTmux { live: resume_strings(&["08-gm-bo", "stray"]), ..Default::default() };

        let output = resume_run(&[], &mut tmux, resume_fleet).expect("resume");

        assert!(output.contains("01-wish — awake"));
        assert!(output.contains("99-tonk — awake"));
        assert!(output.contains("2 sessions resumed"));
        assert_eq!(
            tmux.calls,
            vec![
                ResumeCall::ListLive,
                ResumeCall::NewSession("01-wish".to_owned(), "wish".to_owned()),
                ResumeCall::NewWindow("01-wish".to_owned(), "logs".to_owned()),
                ResumeCall::Restore("01-wish".to_owned()),
                ResumeCall::NewSession("99-tonk".to_owned(), "oracle".to_owned()),
                ResumeCall::Restore("99-tonk".to_owned()),
            ]
        );
    }

    #[test]
    fn resume_rejects_args_and_separator_before_tmux() {
        let mut tmux = ResumeFakeTmux::default();
        let flag = resume_run(&resume_strings(&["-bad"]), &mut tmux, resume_fleet).expect_err("flag");
        assert!(flag.contains("looks like a flag"));
        let sep = resume_run(&resume_strings(&["--"]), &mut tmux, resume_fleet).expect_err("sep");
        assert!(sep.contains("-- separator"));
        let extra = resume_run(&resume_strings(&["wish"]), &mut tmux, resume_fleet).expect_err("extra");
        assert!(extra.contains("unexpected argument"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn resume_rejects_bad_config_and_live_targets_before_create() {
        let mut tmux = ResumeFakeTmux::default();
        let error = resume_run(&[], &mut tmux, resume_bad_fleet).expect_err("config");
        assert!(error.contains("resume target"));
        assert!(tmux.calls.is_empty());
        let mut tmux = ResumeFakeTmux { live: resume_strings(&["-bad"]), ..Default::default() };
        let error = resume_run(&[], &mut tmux, resume_fleet).expect_err("live");
        assert!(error.contains("tmux target"));
        assert_eq!(tmux.calls, vec![ResumeCall::ListLive]);
    }

    #[test]
    fn resume_counts_only_successful_sessions() {
        let mut tmux = ResumeFakeTmux { fail_new: resume_strings(&["01-wish"]), ..Default::default() };

        let error = resume_run(&[], &mut tmux, resume_fleet).expect_err("fail");

        assert_eq!(error, "new failed");
        assert_eq!(tmux.calls, vec![ResumeCall::ListLive, ResumeCall::NewSession("01-wish".to_owned(), "wish".to_owned())]);
    }

    #[test]
    fn resume_builds_safe_tmux_args_with_cwd() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = ResumeEnvGuard::resume_new();
        let window = ResumeWindow { name: "wish".to_owned(), repo: "tonkmac/wish".to_owned() };

        assert_eq!(
            resume_new_session_args("01-wish", &window),
            vec![
                "-d".to_owned(),
                "-s".to_owned(),
                "01-wish".to_owned(),
                "-n".to_owned(),
                "wish".to_owned(),
                "-c".to_owned(),
                ghq_root().join("github.com/tonkmac/wish").display().to_string(),
            ]
        );
        assert!(resume_validate_repo("../bad").is_err());
        assert!(resume_validate_repo("-bad").is_err());
    }

    #[test]
    fn resume_reads_tab_order_names_hermetically() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = ResumeEnvGuard::resume_new();
        let path = maw_state_path(&current_xdg_env(), &["tab-order", "01-wish.json"]);
        std::fs::write(&path, r#"[{"index":1,"name":"logs"},{"index":0,"name":"wish"},{"name":"-bad"}]"#).expect("write");

        let names = resume_read_tab_order("01-wish").expect("read");

        assert_eq!(names, vec!["logs".to_owned(), "wish".to_owned()]);
    }
}
