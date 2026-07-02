const DISPATCH_118: &[DispatcherEntry] = &[DispatcherEntry {
    command: "sleep",
    handler: Handler::Sync(sleep_run_command),
}];

const SLEEP_USAGE: &str = "usage: maw sleep <oracle> [window] | maw sleep --all-done [--dry-run] — gracefully stop one Oracle window; see maw kill for immediate removal and maw done for worktrees";
const SLEEP_WINDOW_FORMAT: &str = "#{session_name}|||#{window_index}|||#{window_name}";
const SLEEP_PANE_FORMAT: &str = "#{session_name}|||#{window_index}|||#{window_name}|||#{pane_index}|||#{pane_id}|||#{pane_pid}|||#{pane_current_command}|||#{pane_title}";

#[derive(Debug, Clone, PartialEq, Eq)]
enum SleepArgs {
    One(SleepOneArgs),
    AllDone(SleepAllDoneArgs),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepOneArgs {
    target: String,
    window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepAllDoneArgs {
    dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepSession {
    name: String,
    windows: Vec<SleepWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepWindow {
    index: u32,
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepResolved {
    session: String,
    window: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SleepPane {
    session: String,
    window_index: u32,
    window_name: String,
    pane_index: u32,
    pane_id: String,
    pane_pid: String,
    current_command: String,
    title: String,
}

trait SleepTmux {
    fn sleep_list_sessions(&mut self) -> Result<Vec<SleepSession>, String>;
    fn sleep_list_windows(&mut self, session: &str) -> Result<Vec<SleepWindow>, String>;
    fn sleep_current_session(&mut self) -> Result<String, String>;
    fn sleep_list_panes(&mut self, session: &str) -> Result<Vec<SleepPane>, String>;
    fn sleep_capture_pane(&mut self, pane_id: &str) -> Result<String, String>;
    fn sleep_send_exit(&mut self, target: &str) -> Result<(), String>;
    fn sleep_wait_grace(&mut self) -> Result<(), String>;
    fn sleep_kill_window(&mut self, target: &str) -> Result<(), String>;
    fn sleep_kill_pane(&mut self, pane_id: &str) -> Result<(), String>;
    fn sleep_save_tab_order(&mut self, session: &str, windows: &[SleepWindow]) -> Result<(), String>;
    fn sleep_run_lifecycle(&mut self, resolved: &SleepResolved, requested: &str) -> Result<(), String>;
    fn sleep_append_log(&mut self, requested: &str, window: &str) -> Result<(), String>;
    fn sleep_take_snapshot(&mut self, sessions: &[SleepSession]) -> Result<(), String>;
}

struct SleepSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

impl SleepSystemTmux {
    fn sleep_new() -> Self { Self { runner: maw_tmux::CommandTmuxRunner::new() } }
}

impl SleepTmux for SleepSystemTmux {
    fn sleep_list_sessions(&mut self) -> Result<Vec<SleepSession>, String> {
        sleep_tmux_run(&mut self.runner, "list-windows", &["-a", "-F", SLEEP_WINDOW_FORMAT])
            .map(|raw| sleep_parse_sessions(&raw))
    }

    fn sleep_list_windows(&mut self, session: &str) -> Result<Vec<SleepWindow>, String> {
        sleep_validate_tmux_target(session)?;
        let raw = sleep_tmux_run(&mut self.runner, "list-windows", &["-t", session, "-F", "#{window_index}|||#{window_name}"])?;
        Ok(sleep_parse_windows(&raw))
    }

    fn sleep_current_session(&mut self) -> Result<String, String> {
        let raw = sleep_tmux_run(&mut self.runner, "display-message", &["-p", "#{session_name}"])?;
        let session = raw.trim().to_owned();
        sleep_validate_tmux_target(&session)?;
        Ok(session)
    }

    fn sleep_list_panes(&mut self, session: &str) -> Result<Vec<SleepPane>, String> {
        sleep_validate_tmux_target(session)?;
        let raw = sleep_tmux_run(&mut self.runner, "list-panes", &["-t", session, "-F", SLEEP_PANE_FORMAT])?;
        Ok(sleep_parse_panes(&raw))
    }

    fn sleep_capture_pane(&mut self, pane_id: &str) -> Result<String, String> {
        sleep_validate_pane_id(pane_id)?;
        sleep_tmux_run(&mut self.runner, "capture-pane", &["-p", "-t", pane_id, "-S", "-40"])
    }

    fn sleep_send_exit(&mut self, target: &str) -> Result<(), String> {
        sleep_validate_tmux_target(target)?;
        for ch in ["/", "e", "x", "i", "t"] {
            sleep_tmux_run(&mut self.runner, "send-keys", &["-t", target, "-l", ch])?;
        }
        sleep_tmux_run(&mut self.runner, "send-keys", &["-t", target, "Enter"]).map(|_| ())
    }

    fn sleep_wait_grace(&mut self) -> Result<(), String> {
        std::thread::sleep(std::time::Duration::from_secs(3));
        Ok(())
    }

    fn sleep_kill_window(&mut self, target: &str) -> Result<(), String> {
        sleep_validate_tmux_target(target)?;
        sleep_tmux_run(&mut self.runner, "kill-window", &["-t", target]).map(|_| ())
    }

    fn sleep_kill_pane(&mut self, pane_id: &str) -> Result<(), String> {
        sleep_validate_pane_id(pane_id)?;
        sleep_tmux_run(&mut self.runner, "kill-pane", &["-t", pane_id]).map(|_| ())
    }

    fn sleep_save_tab_order(&mut self, session: &str, windows: &[SleepWindow]) -> Result<(), String> {
        sleep_write_tab_order(session, windows)
    }

    fn sleep_run_lifecycle(&mut self, _resolved: &SleepResolved, _requested: &str) -> Result<(), String> {
        Ok(())
    }

    fn sleep_append_log(&mut self, requested: &str, window: &str) -> Result<(), String> {
        sleep_append_log_xdg(requested, window)
    }

    fn sleep_take_snapshot(&mut self, sessions: &[SleepSession]) -> Result<(), String> {
        sleep_write_snapshot(sessions)
    }
}

fn sleep_run_command(argv: &[String]) -> CliOutput {
    sleep_run_command_with(argv, &mut SleepSystemTmux::sleep_new(), load_native_fleet)
}

fn sleep_run_command_with(argv: &[String], tmux: &mut impl SleepTmux, load_fleet: StopFleetLoader) -> CliOutput {
    match sleep_run(argv, tmux, load_fleet) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn sleep_run(argv: &[String], tmux: &mut impl SleepTmux, load_fleet: StopFleetLoader) -> Result<String, String> {
    let options = sleep_parse_args(argv)?;
    let options = match options {
        SleepArgs::One(options) => options,
        SleepArgs::AllDone(options) => return sleep_run_all_done(tmux, &options),
    };
    let sessions = tmux.sleep_list_sessions()?;
    let fleet = load_fleet();
    let resolved = sleep_resolve_target(&options, &sessions, &fleet).ok_or_else(|| sleep_missing_target(&options.target, &sessions))?;
    sleep_validate_before_destructive(&resolved, &sessions)?;
    let windows = tmux.sleep_list_windows(&resolved.session).unwrap_or_else(|_| sleep_find_session(&sessions, &resolved.session).map_or_else(Vec::new, |s| s.windows.clone()));
    let _ = tmux.sleep_save_tab_order(&resolved.session, &windows);
    let _ = tmux.sleep_run_lifecycle(&resolved, &options.target);
    let target = sleep_tmux_window_target(&resolved);
    let mut out = format!("\x1b[90m...\x1b[0m sending /exit to {target}\n");
    let _ = tmux.sleep_send_exit(&target);
    tmux.sleep_wait_grace()?;
    let still = tmux.sleep_list_windows(&resolved.session).is_ok_and(|after| sleep_window_exists(&after, &resolved.window));
    if still {
        sleep_validate_before_destructive(&resolved, &sessions)?;
        tmux.sleep_kill_window(&target)?;
        let _ = writeln!(out, "  \x1b[33m!\x1b[0m force-killed {} (did not exit gracefully)", resolved.window);
    } else {
        let _ = writeln!(out, "  \x1b[32m✓\x1b[0m {} exited gracefully", resolved.window);
    }
    let _ = tmux.sleep_append_log(&options.target, &resolved.window);
    let latest = tmux.sleep_list_sessions().unwrap_or_default();
    let _ = tmux.sleep_take_snapshot(&latest);
    let _ = writeln!(out, "\x1b[32msleep\x1b[0m {} ({})", options.target, resolved.window);
    Ok(out)
}

fn sleep_run_all_done(tmux: &mut impl SleepTmux, options: &SleepAllDoneArgs) -> Result<String, String> {
    let session = tmux.sleep_current_session()?;
    let sessions = tmux.sleep_list_sessions()?;
    sleep_validate_current_session(&session, &sessions)?;
    let windows = tmux.sleep_list_windows(&session).unwrap_or_else(|_| sleep_find_session(&sessions, &session).map_or_else(Vec::new, |s| s.windows.clone()));
    let candidates = sleep_collect_done_panes(tmux, &session)?;
    let mut out = sleep_render_all_done_plan(&session, &candidates, options.dry_run);
    if candidates.is_empty() {
        let _ = writeln!(out, "\x1b[33m!\x1b[0m no done panes found");
        if options.dry_run { let _ = writeln!(out, "No changes made"); }
        return Ok(out);
    }
    if options.dry_run {
        let _ = writeln!(out, "No changes made");
        return Ok(out);
    }
    let owned = sleep_validate_all_done_ownership(tmux, &session, &candidates)?;
    let _ = tmux.sleep_save_tab_order(&session, &windows);
    for pane in &owned {
        let owned_now = sleep_validate_one_done_pane_ownership(tmux, &session, pane)?;
        tmux.sleep_kill_pane(&owned_now.pane_id)?;
        let _ = tmux.sleep_append_log("--all-done", &pane.window_name);
    }
    let latest = tmux.sleep_list_sessions().unwrap_or_default();
    let _ = tmux.sleep_take_snapshot(&latest);
    Ok(out)
}

fn sleep_parse_args(argv: &[String]) -> Result<SleepArgs, String> {
    if argv.is_empty() { return Err(SLEEP_USAGE.to_owned()); }
    let mut words = Vec::new();
    let mut all_done = false;
    let mut dry_run = false;
    for value in argv {
        match value.as_str() {
            "--help" | "-h" | "help" => return Err(SLEEP_USAGE.to_owned()),
            "--all-done" => all_done = true,
            "--dry-run" => dry_run = true,
            "--" => return Err("sleep: -- separator is not allowed".to_owned()),
            item if item.starts_with('-') => return Err(sleep_flag_like_target(item)),
            item => words.push(sleep_validate_user_target(item)?),
        }
    }
    if all_done {
        if !words.is_empty() { return Err("sleep --all-done does not accept a target".to_owned()); }
        return Ok(SleepArgs::AllDone(SleepAllDoneArgs { dry_run }));
    }
    if dry_run { return Err("sleep --dry-run is only supported with --all-done".to_owned()); }
    match words.as_slice() {
        [target] => Ok(SleepArgs::One(SleepOneArgs { target: target.clone(), window: None })),
        [target, window] => Ok(SleepArgs::One(SleepOneArgs { target: target.clone(), window: Some(window.clone()) })),
        _ => Err(SLEEP_USAGE.to_owned()),
    }
}

fn sleep_resolve_target(args: &SleepOneArgs, sessions: &[SleepSession], fleet: &[NativeFleetSession]) -> Option<SleepResolved> {
    if args.window.is_none() {
        if let Some(found) = sleep_resolve_by_window(&args.target, sessions) { return Some(found); }
    }
    if let Some(found) = sleep_resolve_by_session(args, sessions, fleet) { return Some(found); }
    sleep_detect_session(args, sessions, fleet)
}

fn sleep_resolve_by_window(target: &str, sessions: &[SleepSession]) -> Option<SleepResolved> {
    let needle = target.to_ascii_lowercase();
    let stripped = sleep_strip_dash(&needle);
    for session in sessions {
        for window in &session.windows {
            let name = window.name.to_ascii_lowercase();
            if name == needle || sleep_strip_dash(&name) == stripped {
                return Some(SleepResolved { session: session.name.clone(), window: window.name.clone() });
            }
        }
    }
    None
}

fn sleep_resolve_by_session(args: &SleepOneArgs, sessions: &[SleepSession], fleet: &[NativeFleetSession]) -> Option<SleepResolved> {
    let session = sessions.iter().find(|session| sleep_session_matches(&session.name, &args.target))?;
    let window = args.window.clone().or_else(|| sleep_fleet_primary(&session.name, fleet)).or_else(|| session.windows.first().map(|item| item.name.clone()))?;
    Some(SleepResolved { session: session.name.clone(), window })
}

fn sleep_detect_session(args: &SleepOneArgs, sessions: &[SleepSession], fleet: &[NativeFleetSession]) -> Option<SleepResolved> {
    let fleet_entry = fleet.iter().find(|entry| sleep_session_matches(&entry.name, &args.target))?;
    let session = fleet_entry.name.clone();
    let window = args.window.clone().or_else(|| fleet_entry.windows.first().map(|item| item.name.clone())).or_else(|| sleep_find_session(sessions, &session).and_then(|item| item.windows.first().map(|w| w.name.clone())))?;
    Some(SleepResolved { session, window })
}

fn sleep_session_matches(session: &str, target: &str) -> bool {
    session == target || session.ends_with(&format!("-{target}")) || sleep_strip_dash(session) == sleep_strip_dash(target)
}

fn sleep_fleet_primary(session: &str, fleet: &[NativeFleetSession]) -> Option<String> {
    fleet.iter().find(|entry| entry.name == session).and_then(|entry| entry.windows.first()).map(|window| window.name.clone())
}

fn sleep_validate_before_destructive(resolved: &SleepResolved, sessions: &[SleepSession]) -> Result<(), String> {
    sleep_validate_tmux_target(&resolved.session)?;
    sleep_validate_tmux_target(&resolved.window)?;
    let session = sleep_find_session(sessions, &resolved.session).ok_or_else(|| format!("sleep: refusing missing session {}", resolved.session))?;
    if sleep_window_exists(&session.windows, &resolved.window) { Ok(()) } else { Err(format!("sleep: refusing missing window {}:{}", resolved.session, resolved.window)) }
}

fn sleep_find_session<'a>(sessions: &'a [SleepSession], name: &str) -> Option<&'a SleepSession> {
    sessions.iter().find(|session| session.name == name)
}

fn sleep_window_exists(windows: &[SleepWindow], target: &str) -> bool {
    let stripped = sleep_strip_dash(target);
    windows.iter().any(|window| window.name == target || sleep_strip_dash(&window.name) == stripped)
}

fn sleep_tmux_window_target(resolved: &SleepResolved) -> String { format!("{}:{}", resolved.session, resolved.window) }

fn sleep_parse_sessions(raw: &str) -> Vec<SleepSession> {
    let mut sessions = Vec::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) { sleep_push_session_line(&mut sessions, line); }
    sessions
}

fn sleep_push_session_line(sessions: &mut Vec<SleepSession>, line: &str) {
    let parts = line.split("|||").collect::<Vec<_>>();
    let name = parts.first().copied().unwrap_or_default().to_owned();
    let window = SleepWindow { index: parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0), name: parts.get(2).copied().unwrap_or_default().to_owned() };
    if let Some(session) = sessions.iter_mut().find(|session| session.name == name) { session.windows.push(window); } else { sessions.push(SleepSession { name, windows: vec![window] }); }
}

fn sleep_parse_windows(raw: &str) -> Vec<SleepWindow> {
    raw.lines().filter_map(sleep_parse_window_line).collect()
}

fn sleep_parse_window_line(line: &str) -> Option<SleepWindow> {
    let mut fields = line.split("|||");
    Some(SleepWindow { index: fields.next()?.parse().ok()?, name: fields.next().unwrap_or_default().to_owned() })
}

fn sleep_parse_panes(raw: &str) -> Vec<SleepPane> {
    raw.lines().filter_map(sleep_parse_pane_line).collect()
}

fn sleep_parse_pane_line(line: &str) -> Option<SleepPane> {
    let mut fields = line.split("|||");
    Some(SleepPane {
        session: fields.next()?.to_owned(),
        window_index: fields.next()?.parse().ok()?,
        window_name: fields.next().unwrap_or_default().to_owned(),
        pane_index: fields.next()?.parse().ok()?,
        pane_id: fields.next().unwrap_or_default().to_owned(),
        pane_pid: fields.next().unwrap_or_default().to_owned(),
        current_command: fields.next().unwrap_or_default().to_owned(),
        title: fields.next().unwrap_or_default().to_owned(),
    })
}

fn sleep_validate_current_session(session: &str, sessions: &[SleepSession]) -> Result<(), String> {
    sleep_validate_tmux_target(session)?;
    if sleep_find_session(sessions, session).is_some() { Ok(()) } else { Err(format!("sleep --all-done: refusing missing current session {session}")) }
}

fn sleep_collect_done_panes(tmux: &mut impl SleepTmux, session: &str) -> Result<Vec<SleepPane>, String> {
    let mut candidates = Vec::new();
    for pane in tmux.sleep_list_panes(session)? {
        if pane.session != session { continue; }
        if !sleep_is_parked_command(&pane.current_command) { continue; }
        sleep_validate_pane_ownership_fields(&pane, session)?;
        let capture = tmux.sleep_capture_pane(&pane.pane_id)?;
        if sleep_capture_has_done_marker(&capture) { candidates.push(pane); }
    }
    Ok(candidates)
}

fn sleep_validate_all_done_ownership(tmux: &mut impl SleepTmux, session: &str, candidates: &[SleepPane]) -> Result<Vec<SleepPane>, String> {
    let mut owned = Vec::new();
    let refreshed = tmux.sleep_list_panes(session)?;
    for candidate in candidates {
        sleep_validate_pane_ownership_fields(candidate, session)?;
        let Some(current) = refreshed.iter().find(|pane| pane.pane_id == candidate.pane_id) else {
            return Err(format!("sleep --all-done: refusing pane {} because ownership disappeared", candidate.pane_id));
        };
        sleep_validate_same_owned_pane(candidate, current, session)?;
        let capture = tmux.sleep_capture_pane(&current.pane_id)?;
        if !sleep_capture_has_done_marker(&capture) || !sleep_is_parked_command(&current.current_command) {
            return Err(format!("sleep --all-done: refusing pane {} because it is no longer done", current.pane_id));
        }
        owned.push(current.clone());
    }
    Ok(owned)
}

fn sleep_validate_one_done_pane_ownership(tmux: &mut impl SleepTmux, session: &str, expected: &SleepPane) -> Result<SleepPane, String> {
    let refreshed = tmux.sleep_list_panes(session)?;
    let Some(current) = refreshed.iter().find(|pane| pane.pane_id == expected.pane_id) else {
        return Err(format!("sleep --all-done: refusing pane {} because ownership disappeared", expected.pane_id));
    };
    sleep_validate_same_owned_pane(expected, current, session)?;
    let capture = tmux.sleep_capture_pane(&current.pane_id)?;
    if !sleep_capture_has_done_marker(&capture) || !sleep_is_parked_command(&current.current_command) {
        return Err(format!("sleep --all-done: refusing pane {} because it is no longer done", current.pane_id));
    }
    Ok(current.clone())
}

fn sleep_validate_same_owned_pane(expected: &SleepPane, current: &SleepPane, session: &str) -> Result<(), String> {
    sleep_validate_pane_ownership_fields(current, session)?;
    if current.pane_pid != expected.pane_pid || current.window_name != expected.window_name || current.window_index != expected.window_index {
        return Err(format!("sleep --all-done: refusing pane {} because PID/window ownership changed", expected.pane_id));
    }
    Ok(())
}

fn sleep_validate_pane_ownership_fields(pane: &SleepPane, session: &str) -> Result<(), String> {
    if pane.session != session {
        return Err(format!("sleep --all-done: refusing pane {} outside current session {session}", pane.pane_id));
    }
    sleep_validate_tmux_target(&pane.session)?;
    sleep_validate_tmux_target(&pane.window_name)?;
    sleep_validate_pane_id(&pane.pane_id)?;
    sleep_validate_pid(&pane.pane_pid)?;
    Ok(())
}

fn sleep_is_parked_command(command: &str) -> bool {
    matches!(command.trim().to_ascii_lowercase().as_str(), "read" | "sleep")
}

fn sleep_capture_has_done_marker(capture: &str) -> bool {
    let lower = capture.to_ascii_lowercase();
    lower.contains("[done") && lower.contains("exit")
}

fn sleep_render_all_done_plan(session: &str, panes: &[SleepPane], dry_run: bool) -> String {
    let mut out = if dry_run { format!("\x1b[32msleep --all-done\x1b[0m {session} (dry-run)\n") } else { format!("\x1b[32msleep --all-done\x1b[0m {session}\n") };
    for pane in panes {
        let action = if dry_run { "would sleep" } else { "slept" };
        let _ = writeln!(out, "  {action} {} pid={} window={} command={}", pane.pane_id, pane.pane_pid, pane.window_name, pane.current_command);
    }
    out
}

fn sleep_validate_user_target(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" || value.contains('\0') || value.chars().any(char::is_control) {
        Err("sleep target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(value.to_owned())
    }
}

fn sleep_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" || value.contains('\0') || value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        Err("sleep tmux target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn sleep_validate_pane_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" || !value.starts_with('%') || value.contains('\0') || value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        Err("sleep pane id must be a tmux %pane id and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn sleep_validate_pid(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "0" || !value.chars().all(|ch| ch.is_ascii_digit()) {
        Err("sleep pane pid must be a non-zero decimal PID".to_owned())
    } else {
        Ok(())
    }
}

fn sleep_flag_like_target(value: &str) -> String { format!("\"{value}\" looks like a flag, not a target.\n  {SLEEP_USAGE}") }

fn sleep_missing_target(target: &str, sessions: &[SleepSession]) -> String {
    let mut out = format!("could not resolve sleep target: '{target}'");
    let flat = sessions.iter().flat_map(|s| s.windows.iter().map(move |w| format!("{}:{}", s.name, w.name))).take(10).collect::<Vec<_>>();
    if !flat.is_empty() { let _ = write!(out, "\n\x1b[90mavailable:\x1b[0m {}", flat.join(", ")); }
    out
}

fn sleep_strip_dash(value: &str) -> String { value.trim_end_matches('-').to_owned() }

fn sleep_tmux_run<R: maw_tmux::TmuxRunner>(runner: &mut R, subcommand: &str, args: &[&str]) -> Result<String, String> {
    let owned = args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>();
    runner.run(subcommand, &owned).map_err(|error| error.message)
}

fn sleep_write_tab_order(session: &str, windows: &[SleepWindow]) -> Result<(), String> {
    sleep_validate_tmux_target(session)?;
    let path = maw_state_path(&current_xdg_env(), &["tab-order", &format!("{session}.json")]);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("sleep: tab-order mkdir failed: {error}"))?; }
    let entries = windows.iter().map(|window| serde_json::json!({ "index": window.index, "name": window.name })).collect::<Vec<_>>();
    let text = serde_json::to_string_pretty(&entries).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{text}\n")).map_err(|error| format!("sleep: tab-order write failed: {error}"))
}

fn sleep_append_log_xdg(oracle: &str, window: &str) -> Result<(), String> {
    let path = maw_state_path(&current_xdg_env(), &["maw-log.jsonl"]);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let row = serde_json::json!({ "ts": sleep_now_iso(), "type": "sleep", "oracle": oracle, "window": window });
    std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| { use std::io::Write as _; writeln!(file, "{row}") }).map_err(|error| error.to_string())
}

fn sleep_write_snapshot(sessions: &[SleepSession]) -> Result<(), String> {
    let dir = maw_state_path(&current_xdg_env(), &["snapshots"]);
    std::fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
    let timestamp = sleep_now_iso();
    let file = dir.join(format!("sleep-{}.json", sleep_snapshot_stamp()));
    let snapshot = serde_json::json!({ "timestamp": timestamp, "trigger": "sleep", "sessions": sleep_snapshot_sessions(sessions) });
    let text = serde_json::to_string_pretty(&snapshot).map_err(|error| error.to_string())?;
    std::fs::write(file, format!("{text}\n")).map_err(|error| error.to_string())
}

fn sleep_snapshot_sessions(sessions: &[SleepSession]) -> Vec<serde_json::Value> {
    sessions.iter().map(|session| serde_json::json!({ "name": session.name, "windows": session.windows.iter().map(|window| serde_json::json!({ "name": window.name })).collect::<Vec<_>>() })).collect()
}

fn sleep_now_iso() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("epoch-seconds:{seconds}")
}

fn sleep_snapshot_stamp() -> String {
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_nanos());
    nanos.to_string()
}

#[cfg(test)]
mod sleep_tests {
    use super::*;

    #[derive(Default)]
    struct SleepFakeTmux {
        sessions: Vec<SleepSession>,
        current_session: String,
        panes: Vec<SleepPane>,
        captures: std::collections::BTreeMap<String, String>,
        mutate_pid_on_refresh: bool,
        calls: Vec<String>,
        log: Vec<String>,
    }

    impl SleepFakeTmux {
        fn sleep_fixture() -> Self {
            Self {
                current_session: "01-nova".to_owned(),
                sessions: vec![SleepSession { name: "01-nova".to_owned(), windows: vec![SleepWindow { index: 0, name: "nova-oracle".to_owned() }, SleepWindow { index: 1, name: "worktree-task-".to_owned() }] }],
                ..Self::default()
            }
        }

        fn sleep_all_done_fixture() -> Self {
            let mut fake = Self::sleep_fixture();
            fake.panes = vec![
                SleepPane { session: "01-nova".to_owned(), window_index: 1, window_name: "worktree-task-".to_owned(), pane_index: 0, pane_id: "%21".to_owned(), pane_pid: "421".to_owned(), current_command: "read".to_owned(), title: "done".to_owned() },
                SleepPane { session: "01-nova".to_owned(), window_index: 0, window_name: "nova-oracle".to_owned(), pane_index: 0, pane_id: "%20".to_owned(), pane_pid: "420".to_owned(), current_command: "codex".to_owned(), title: "busy".to_owned() },
                SleepPane { session: "02-other".to_owned(), window_index: 0, window_name: "other-done".to_owned(), pane_index: 0, pane_id: "%99".to_owned(), pane_pid: "999".to_owned(), current_command: "read".to_owned(), title: "done".to_owned() },
            ];
            fake.captures.insert("%21".to_owned(), "job output\n[done — exit 0]\n".to_owned());
            fake.captures.insert("%20".to_owned(), "still running\n".to_owned());
            fake.captures.insert("%99".to_owned(), "[done — exit 0]\n".to_owned());
            fake
        }
    }

    impl SleepTmux for SleepFakeTmux {
        fn sleep_list_sessions(&mut self) -> Result<Vec<SleepSession>, String> { Ok(self.sessions.clone()) }
        fn sleep_list_windows(&mut self, session: &str) -> Result<Vec<SleepWindow>, String> { Ok(sleep_find_session(&self.sessions, session).map_or_else(Vec::new, |s| s.windows.clone())) }
        fn sleep_current_session(&mut self) -> Result<String, String> { Ok(self.current_session.clone()) }
        fn sleep_list_panes(&mut self, session: &str) -> Result<Vec<SleepPane>, String> {
            self.calls.push(format!("list-panes {session}"));
            let mut panes = self.panes.iter().filter(|pane| pane.session == session).cloned().collect::<Vec<_>>();
            if self.mutate_pid_on_refresh && self.calls.iter().filter(|call| call.as_str() == "list-panes 01-nova").count() > 1 {
                if let Some(pane) = panes.iter_mut().find(|pane| pane.pane_id == "%21") { pane.pane_pid = "422".to_owned(); }
            }
            Ok(panes)
        }
        fn sleep_capture_pane(&mut self, pane_id: &str) -> Result<String, String> {
            self.calls.push(format!("capture-pane {pane_id}"));
            Ok(self.captures.get(pane_id).cloned().unwrap_or_default())
        }
        fn sleep_send_exit(&mut self, target: &str) -> Result<(), String> { self.calls.push(format!("send-exit {target}")); Ok(()) }
        fn sleep_wait_grace(&mut self) -> Result<(), String> { self.calls.push("wait3s".to_owned()); Ok(()) }
        fn sleep_kill_window(&mut self, target: &str) -> Result<(), String> { self.calls.push(format!("kill-window {target}")); for s in &mut self.sessions { s.windows.retain(|w| format!("{}:{}", s.name, w.name) != target); } Ok(()) }
        fn sleep_kill_pane(&mut self, pane_id: &str) -> Result<(), String> { self.calls.push(format!("kill-pane {pane_id}")); self.panes.retain(|pane| pane.pane_id != pane_id); Ok(()) }
        fn sleep_save_tab_order(&mut self, session: &str, _windows: &[SleepWindow]) -> Result<(), String> { self.calls.push(format!("save-tab-order {session}")); Ok(()) }
        fn sleep_run_lifecycle(&mut self, resolved: &SleepResolved, requested: &str) -> Result<(), String> { self.calls.push(format!("lifecycle {requested} {}:{}", resolved.session, resolved.window)); Ok(()) }
        fn sleep_append_log(&mut self, requested: &str, window: &str) -> Result<(), String> { self.log.push(format!("{requested}:{window}")); Ok(()) }
        fn sleep_take_snapshot(&mut self, _sessions: &[SleepSession]) -> Result<(), String> { self.calls.push("snapshot sleep".to_owned()); Ok(()) }
    }

    fn sleep_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn sleep_fleet() -> Vec<NativeFleetSession> { vec![NativeFleetSession { name: "01-nova".to_owned(), windows: vec![NativeFleetWindow { name: "nova-oracle".to_owned(), repo: String::new() }], ..NativeFleetSession::default() }] }

    #[test]
    fn sleep_dispatch_registers_native() {
        assert_eq!(DISPATCH_118[0].command, "sleep");
    }

    #[test]
    fn sleep_preserves_destructive_flow_and_golden() {
        let mut tmux = SleepFakeTmux::sleep_fixture();
        let out = sleep_run(&sleep_args(&["nova"]), &mut tmux, sleep_fleet).expect("sleep ok");
        assert_eq!(out, include_str!("../../tests/fixtures/native-sleep/sleep-default.stdout"));
        assert_eq!(tmux.calls, ["save-tab-order 01-nova", "lifecycle nova 01-nova:nova-oracle", "send-exit 01-nova:nova-oracle", "wait3s", "kill-window 01-nova:nova-oracle", "snapshot sleep"]);
        assert_eq!(tmux.log, ["nova:nova-oracle"]);
    }

    #[test]
    fn sleep_all_done_dry_run_is_hermetic_golden_and_non_mutating() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = SleepFakeTmux::sleep_all_done_fixture();
        let out = sleep_run(&sleep_args(&["--all-done", "--dry-run"]), &mut tmux, sleep_fleet).expect("sleep all done dry-run");
        assert_eq!(out, include_str!("../../tests/fixtures/native-sleep/sleep-all-done-dry-run.stdout"));
        assert_eq!(tmux.calls, ["list-panes 01-nova", "capture-pane %21"]);
        assert!(tmux.log.is_empty());
        assert!(tmux.panes.iter().any(|pane| pane.pane_id == "%21"));
    }

    #[test]
    fn sleep_all_done_live_validates_ownership_before_kill_and_uses_golden() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = SleepFakeTmux::sleep_all_done_fixture();
        let out = sleep_run(&sleep_args(&["--all-done"]), &mut tmux, sleep_fleet).expect("sleep all done");
        assert_eq!(out, include_str!("../../tests/fixtures/native-sleep/sleep-all-done-live.stdout"));
        assert_eq!(
            tmux.calls,
            [
                "list-panes 01-nova",
                "capture-pane %21",
                "list-panes 01-nova",
                "capture-pane %21",
                "save-tab-order 01-nova",
                "list-panes 01-nova",
                "capture-pane %21",
                "kill-pane %21",
                "snapshot sleep"
            ]
        );
        assert_eq!(tmux.log, ["--all-done:worktree-task-"]);
        assert!(!tmux.panes.iter().any(|pane| pane.pane_id == "%21"));
        assert!(tmux.panes.iter().any(|pane| pane.pane_id == "%99"));
    }

    #[test]
    fn sleep_all_done_refuses_pid_ownership_race_before_any_kill() {
        let mut tmux = SleepFakeTmux::sleep_all_done_fixture();
        tmux.mutate_pid_on_refresh = true;
        let err = sleep_run(&sleep_args(&["--all-done"]), &mut tmux, sleep_fleet).unwrap_err();
        assert!(err.contains("ownership changed"));
        assert!(tmux.calls.iter().all(|call| !call.starts_with("kill-pane")));
        assert!(tmux.log.is_empty());
    }

    #[test]
    fn sleep_all_done_rejects_bad_pane_id_before_kill() {
        let mut tmux = SleepFakeTmux::sleep_all_done_fixture();
        tmux.panes[0].pane_id = "-bad".to_owned();
        tmux.captures.insert("-bad".to_owned(), "[done — exit 0]\n".to_owned());
        let err = sleep_run(&sleep_args(&["--all-done"]), &mut tmux, sleep_fleet).unwrap_err();
        assert!(err.contains("pane id"));
        assert!(tmux.calls.iter().all(|call| !call.starts_with("kill-pane")));
    }

    #[test]
    fn sleep_validates_before_destructive_and_rejects_flags() {
        let mut tmux = SleepFakeTmux::sleep_fixture();
        let bad = sleep_run(&sleep_args(&["-bad"]), &mut tmux, sleep_fleet).unwrap_err();
        assert!(bad.contains("looks like a flag"));
        tmux.sessions[0].windows.clear();
        let missing = sleep_run(&sleep_args(&["nova"]), &mut tmux, sleep_fleet).unwrap_err();
        assert!(missing.contains("refusing missing window") || missing.contains("could not resolve"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn sleep_fs_log_path_is_hermetic_with_xdg_env() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let root = std::env::temp_dir().join(format!("maw-sleep-test-{}", sleep_snapshot_stamp()));
        let state = root.join("state");
        let _restore = EnvVarRestore::capture("MAW_STATE_DIR");
        std::env::set_var("MAW_STATE_DIR", &state);
        sleep_append_log_xdg("nova", "nova-oracle").expect("append log");
        let log = std::fs::read_to_string(state.join("maw-log.jsonl")).expect("log read");
        assert!(log.contains("\"type\":\"sleep\""));
        assert!(log.contains("\"oracle\":\"nova\""));
    }
}
