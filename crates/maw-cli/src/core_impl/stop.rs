const DISPATCH_86: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "stop",
        handler: Handler::Sync(stop_run_command),
    },
    DispatcherEntry {
        command: "rest",
        handler: Handler::Sync(stop_run_command),
    },
];

const STOP_USAGE: &str = "usage: maw stop — stop ALL oracle fleet sessions";
const STOP_WINDOW_FORMAT: &str = "#{window_index}\t#{window_name}";

type StopFleetLoader = fn() -> Vec<NativeFleetSession>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct StopWindowOrder {
    index: u32,
    name: String,
}

trait StopTmux {
    fn stop_list_live_sessions(&mut self) -> Result<Vec<String>, String>;
    fn stop_save_tab_order(&mut self, session: &str) -> Result<(), String>;
    fn stop_kill_session(&mut self, session: &str) -> Result<(), String>;
}

struct StopSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

impl StopSystemTmux {
    fn stop_new() -> Self {
        Self {
            runner: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl StopTmux for StopSystemTmux {
    fn stop_list_live_sessions(&mut self) -> Result<Vec<String>, String> {
        match stop_tmux_run(&mut self.runner, "list-sessions", &["-F", "#{session_name}"]) {
            Ok(raw) => Ok(stop_parse_live_sessions(&raw)),
            Err(_) => Ok(Vec::new()),
        }
    }

    fn stop_save_tab_order(&mut self, session: &str) -> Result<(), String> {
        stop_validate_tmux_target(session)?;
        let raw = stop_tmux_run(
            &mut self.runner,
            "list-windows",
            &["-t", session, "-F", STOP_WINDOW_FORMAT],
        )?;
        stop_write_tab_order(session, &stop_parse_window_order(&raw))
    }

    fn stop_kill_session(&mut self, session: &str) -> Result<(), String> {
        stop_validate_tmux_target(session)?;
        stop_tmux_run(&mut self.runner, "kill-session", &["-t", session]).map(|_| ())
    }
}

fn stop_run_command(argv: &[String]) -> CliOutput {
    stop_run_command_with(argv, &mut StopSystemTmux::stop_new(), load_native_fleet)
}

fn stop_run_command_with(
    argv: &[String],
    tmux: &mut impl StopTmux,
    load_fleet: StopFleetLoader,
) -> CliOutput {
    match stop_run(argv, tmux, load_fleet) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn stop_run(
    argv: &[String],
    tmux: &mut impl StopTmux,
    load_fleet: StopFleetLoader,
) -> Result<String, String> {
    stop_parse_args(argv)?;
    let sessions = stop_fleet_session_names(load_fleet())?;
    let live = tmux.stop_list_live_sessions()?;
    stop_validate_live_sessions(&live)?;
    let live = stop_live_fleet_sessions(&sessions, &live);
    let mut output = String::new();
    let mut stopped = 0usize;
    for session in live {
        stop_validate_before_stop(&sessions, &session)?;
        let _ = tmux.stop_save_tab_order(&session);
        if tmux.stop_kill_session(&session).is_ok() {
            let _ = writeln!(output, "  \x1b[90m●\x1b[0m {session} — sleep");
            stopped += 1;
        }
    }
    let _ = writeln!(output, "\n  {stopped} sessions put to sleep.\n");
    Ok(output)
}

fn stop_parse_args(argv: &[String]) -> Result<(), String> {
    let Some(arg) = argv.first() else {
        return Ok(());
    };
    match arg.as_str() {
        "--help" | "-h" | "help" => Err(STOP_USAGE.to_owned()),
        "--" => Err("stop: -- separator is not allowed".to_owned()),
        value if value.starts_with('-') => Err(stop_flag_like_target(value)),
        value => Err(format!("stop: unexpected argument {value}")),
    }
}

fn stop_flag_like_target(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a target.\n  {STOP_USAGE}")
}

fn stop_fleet_session_names(sessions: Vec<NativeFleetSession>) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for session in sessions {
        stop_validate_user_target(&session.name)?;
        names.push(session.name);
    }
    names.sort();
    names.dedup();
    Ok(names)
}

fn stop_live_fleet_sessions(configured: &[String], live: &[String]) -> Vec<String> {
    configured
        .iter()
        .filter(|session| live.iter().any(|live_session| live_session == *session))
        .cloned()
        .collect()
}

fn stop_validate_live_sessions(sessions: &[String]) -> Result<(), String> {
    for session in sessions {
        stop_validate_tmux_target(session)?;
    }
    Ok(())
}

fn stop_validate_before_stop(configured: &[String], session: &str) -> Result<(), String> {
    stop_validate_user_target(session)?;
    stop_validate_tmux_target(session)?;
    if configured.iter().any(|name| name == session) {
        Ok(())
    } else {
        Err(format!("stop: refusing to stop non-fleet session {session}"))
    }
}

fn stop_validate_user_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("stop target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("stop target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn stop_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("stop tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("stop tmux target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn stop_parse_live_sessions(raw: &str) -> Vec<String> {
    raw.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

fn stop_parse_window_order(raw: &str) -> Vec<StopWindowOrder> {
    let mut windows = raw
        .lines()
        .filter_map(stop_window_order_from_line)
        .collect::<Vec<_>>();
    windows.sort_by_key(|window| window.index);
    windows
}

fn stop_window_order_from_line(line: &str) -> Option<StopWindowOrder> {
    let mut fields = line.splitn(2, '\t');
    let index = fields.next()?.parse::<u32>().ok()?;
    let name = fields.next().unwrap_or_default().to_owned();
    Some(StopWindowOrder { index, name })
}

fn stop_write_tab_order(session: &str, windows: &[StopWindowOrder]) -> Result<(), String> {
    stop_validate_tmux_target(session)?;
    let env = current_xdg_env();
    let path = maw_state_path(&env, &["tab-order", &format!("{session}.json")]);
    let Some(parent) = path.parent() else {
        return Err("stop: invalid tab-order path".to_owned());
    };
    std::fs::create_dir_all(parent).map_err(|error| format!("stop: tab-order mkdir failed: {error}"))?;
    let text = stop_tab_order_json(windows)?;
    std::fs::write(&path, text).map_err(|error| format!("stop: tab-order write failed: {error}"))
}

fn stop_tab_order_json(windows: &[StopWindowOrder]) -> Result<String, String> {
    let entries = windows
        .iter()
        .map(|window| serde_json::json!({ "index": window.index, "name": window.name }))
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&entries)
        .map(|text| format!("{text}\n"))
        .map_err(|error| error.to_string())
}

fn stop_tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[&str],
) -> Result<String, String> {
    let owned = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    runner.run(subcommand, &owned).map_err(|error| error.message)
}

#[cfg(test)]
mod stop_tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum StopCall {
        ListLive,
        Save(String),
        Kill(String),
    }

    #[derive(Debug, Default)]
    struct StopFakeTmux {
        live: Vec<String>,
        calls: Vec<StopCall>,
        kill_fail: Vec<String>,
    }

    impl StopTmux for StopFakeTmux {
        fn stop_list_live_sessions(&mut self) -> Result<Vec<String>, String> {
            self.calls.push(StopCall::ListLive);
            Ok(self.live.clone())
        }

        fn stop_save_tab_order(&mut self, session: &str) -> Result<(), String> {
            stop_validate_tmux_target(session)?;
            self.calls.push(StopCall::Save(session.to_owned()));
            Ok(())
        }

        fn stop_kill_session(&mut self, session: &str) -> Result<(), String> {
            stop_validate_tmux_target(session)?;
            self.calls.push(StopCall::Kill(session.to_owned()));
            if self.kill_fail.iter().any(|name| name == session) {
                Err("gone".to_owned())
            } else {
                Ok(())
            }
        }
    }

    fn stop_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn stop_session(name: &str) -> NativeFleetSession {
        NativeFleetSession { name: name.to_owned(), ..NativeFleetSession::default() }
    }

    fn stop_fleet() -> Vec<NativeFleetSession> {
        vec![stop_session("01-wish"), stop_session("08-gm-bo"), stop_session("99-tonk")]
    }

    fn stop_bad_fleet() -> Vec<NativeFleetSession> {
        vec![stop_session("-bad")]
    }

    fn stop_empty_fleet() -> Vec<NativeFleetSession> {
        Vec::new()
    }

    #[test]
    fn stop_dispatch_registers_stop_and_rest() {
        let commands = DISPATCH_86.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, vec!["stop", "rest"]);
    }

    #[test]
    fn stop_only_stops_configured_live_sessions_after_validation() {
        let mut tmux = StopFakeTmux {
            live: stop_strings(&["08-gm-bo", "stray", "01-wish"]),
            ..StopFakeTmux::default()
        };

        let output = stop_run(&[], &mut tmux, stop_fleet).expect("stop");

        assert!(output.contains("01-wish — sleep"));
        assert!(output.contains("08-gm-bo — sleep"));
        assert!(output.contains("2 sessions put to sleep"));
        assert_eq!(
            tmux.calls,
            vec![
                StopCall::ListLive,
                StopCall::Save("01-wish".to_owned()),
                StopCall::Kill("01-wish".to_owned()),
                StopCall::Save("08-gm-bo".to_owned()),
                StopCall::Kill("08-gm-bo".to_owned()),
            ]
        );
    }

    #[test]
    fn stop_ignores_missing_sessions_like_maw_js() {
        let mut tmux = StopFakeTmux { live: Vec::new(), ..StopFakeTmux::default() };

        let output = stop_run(&[], &mut tmux, stop_fleet).expect("stop");

        assert!(output.contains("0 sessions put to sleep"));
        assert_eq!(tmux.calls, vec![StopCall::ListLive]);
    }

    #[test]
    fn stop_counts_only_successful_kills() {
        let mut tmux = StopFakeTmux {
            live: stop_strings(&["01-wish", "08-gm-bo"]),
            kill_fail: stop_strings(&["08-gm-bo"]),
            ..StopFakeTmux::default()
        };

        let output = stop_run(&[], &mut tmux, stop_fleet).expect("stop");

        assert!(output.contains("01-wish — sleep"));
        assert!(!output.contains("08-gm-bo — sleep"));
        assert!(output.contains("1 sessions put to sleep"));
    }

    #[test]
    fn stop_rejects_args_and_separator_before_tmux() {
        let mut tmux = StopFakeTmux::default();
        let flag = stop_run(&stop_strings(&["-bad"]), &mut tmux, stop_fleet).expect_err("flag");
        assert!(flag.contains("looks like a flag"));
        let sep = stop_run(&stop_strings(&["--"]), &mut tmux, stop_fleet).expect_err("separator");
        assert!(sep.contains("-- separator"));
        let extra = stop_run(&stop_strings(&["wish"]), &mut tmux, stop_fleet).expect_err("extra");
        assert!(extra.contains("unexpected argument"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn stop_rejects_bad_config_before_tmux() {
        let mut tmux = StopFakeTmux::default();
        let error = stop_run(&[], &mut tmux, stop_bad_fleet).expect_err("bad config");
        assert!(error.contains("stop target"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn stop_rejects_bad_live_session_before_kill() {
        let mut tmux = StopFakeTmux { live: stop_strings(&["-bad"]), ..StopFakeTmux::default() };

        let error = stop_run(&[], &mut tmux, stop_fleet).expect_err("bad live");

        assert!(error.contains("tmux target"));
        assert_eq!(tmux.calls, vec![StopCall::ListLive]);
    }

    #[test]
    fn stop_empty_fleet_does_not_touch_tmux_kills() {
        let mut tmux = StopFakeTmux { live: stop_strings(&["01-wish"]), ..StopFakeTmux::default() };

        let output = stop_run(&[], &mut tmux, stop_empty_fleet).expect("empty");

        assert!(output.contains("0 sessions put to sleep"));
        assert_eq!(tmux.calls, vec![StopCall::ListLive]);
    }

    #[test]
    fn stop_parses_tab_order_like_js_save_tab_order() {
        let windows = stop_parse_window_order("2\twork\n0\toracle\n");
        assert_eq!(
            windows,
            vec![
                StopWindowOrder { index: 0, name: "oracle".to_owned() },
                StopWindowOrder { index: 2, name: "work".to_owned() },
            ]
        );
        let json = stop_tab_order_json(&windows).expect("json");
        assert!(json.contains("\"index\": 0"));
        assert!(json.contains("\"name\": \"oracle\""));
    }
}
