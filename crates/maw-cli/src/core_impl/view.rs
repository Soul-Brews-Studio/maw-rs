const DISPATCH_260: &[DispatcherEntry] = &[DispatcherEntry { command: "view", handler: Handler::Sync(view_run_command) }];

const VIEW_USAGE: &str = "usage: maw view <agent> [window] [--clean] [--kill] [--readonly|-r] [--split[=<anchor>]] [--wake|--no-wake]";

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct ViewArgs {
    agent: String,
    window_hint: Option<String>,
    clean: bool,
    kill: bool,
    readonly: bool,
    wake: bool,
    no_wake: bool,
    split_anchor: Option<ViewSplitAnchor>,
    alive: BTreeSet<String>,
    zombie_agents: bool,
    yes: bool,
    print: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ViewSplitAnchor {
    Active,
    Target(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewWindowRef { session: String, index: String, name: String }

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewPaneRef { id: String, command: String, target: String, title: String }

#[derive(Debug, Clone, PartialEq, Eq)]
struct ViewZombiePane { pane_id: String, info: String }

fn view_run_command(argv: &[String]) -> CliOutput {
    match view_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) | Err(output) => output,
    }
}

#[allow(clippy::format_push_string)]
fn view_run_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, CliOutput> {
    let parsed = view_parse_args(argv).map_err(|message| view_usage_error(&message))?;
    view_validate_args(&parsed).map_err(|message| command_target_error("view", &message))?;
    if parsed.readonly && parsed.split_anchor.is_some() {
        return Err(command_target_error("view", "maw view --readonly cannot be combined with --split yet; split attaches through a writable nested tmux client"));
    }
    if parsed.zombie_agents {
        return view_cleanup_zombie_agents(runner, parsed.yes).map(|stdout| CliOutput { code: 0, stdout, stderr: String::new() }).map_err(|message| command_target_error("view", &message));
    }

    let sessions = if parsed.alive.is_empty() { view_list_sessions(runner) } else { parsed.alive.iter().cloned().collect() };
    let windows = view_list_windows(runner);
    let session_name = match view_resolve_session(&parsed.agent, &sessions, &windows) {
        Ok(Some(session)) => session,
        Ok(None) => return view_missing_session(&parsed),
        Err(output) => return Err(output),
    };
    view_validate_tmux_token(&session_name, "resolved session").map_err(|message| command_target_error("view", &message))?;

    let view_name = if session_name.ends_with("-view") {
        session_name.clone()
    } else {
        view_view_name(&session_name, parsed.window_hint.as_deref())
    };
    view_validate_tmux_token(&view_name, "view session").map_err(|message| command_target_error("view", &message))?;

    if view_should_plan_without_mutation(&parsed) {
        let mut stdout = view_render_attach_line(&view_name, parsed.clean, parsed.readonly);
        stdout.push_str(&view_render_attach_plan(&view_name, parsed.readonly));
        return Ok(CliOutput { code: 0, stdout, stderr: String::new() });
    }

    let mut stdout = String::new();
    if session_name.ends_with("-view") {
        if parsed.clean {
            view_tmux_set(runner, &session_name, "status", "off")?;
        }
        view_select_window_if_requested(runner, &mut stdout, &session_name, &session_name, parsed.window_hint.as_deref(), &windows)?;
        if let Some(anchor) = &parsed.split_anchor {
            let anchor_pane = view_resolve_split_anchor(runner, anchor)?;
            view_split_view(runner, &session_name, anchor_pane.as_deref())?;
            stdout.push_str(&format!("split   → {session_name}\n"));
            return Ok(CliOutput { code: 0, stdout, stderr: String::new() });
        }
        stdout.push_str(&view_render_attach_line(&session_name, parsed.clean, parsed.readonly));
        if parsed.print || !view_stdout_is_terminal() {
            stdout.push_str(&view_render_attach_plan(&session_name, parsed.readonly));
            return Ok(CliOutput { code: 0, stdout, stderr: String::new() });
        }
        view_attach_or_switch(runner, &session_name, parsed.readonly)?;
        stdout.push_str(&format!("\x1b[90mhint\x1b[0m    → detach with prefix+d, then `tmux kill-session -t {session_name}` when done\n"));
        return Ok(CliOutput { code: 0, stdout, stderr: String::new() });
    }

    if view_has_session(runner, &view_name) {
        stdout.push_str(&format!("\x1b[36mreuse\x1b[0m   → {view_name} (existing grouped session — {session_name})\n"));
    } else {
        view_new_grouped_session(runner, &session_name, &view_name)?;
        stdout.push_str(&format!("\x1b[36mcreated\x1b[0m → {view_name} (grouped with {session_name})\n"));
    }
    view_select_window_if_requested(runner, &mut stdout, &session_name, &view_name, parsed.window_hint.as_deref(), &windows)?;
    if parsed.clean {
        view_tmux_set(runner, &view_name, "status", "off")?;
    }
    if let Some(anchor) = &parsed.split_anchor {
        let anchor_pane = view_resolve_split_anchor(runner, anchor)?;
        view_split_view(runner, &view_name, anchor_pane.as_deref())?;
        stdout.push_str(&format!("split   → {view_name}\n"));
        return Ok(CliOutput { code: 0, stdout, stderr: String::new() });
    }

    stdout.push_str(&view_render_attach_line(&view_name, parsed.clean, parsed.readonly));
    if parsed.print || !view_stdout_is_terminal() {
        stdout.push_str(&view_render_attach_plan(&view_name, parsed.readonly));
    } else {
        view_attach_or_switch(runner, &view_name, parsed.readonly)?;
        stdout.push_str(&format!("\x1b[90mhint\x1b[0m    → detach with prefix+d, then `tmux kill-session -t {view_name}` when done\n"));
    }
    if parsed.kill {
        view_kill_session_guarded(runner, &view_name, &session_name)?;
        stdout.push_str(&format!("\x1b[90mcleaned\x1b[0m → {view_name}\n"));
    }
    Ok(CliOutput { code: 0, stdout, stderr: String::new() })
}

fn view_parse_args(argv: &[String]) -> Result<ViewArgs, String> {
    let mut clean = false;
    let mut kill = false;
    let mut readonly = false;
    let mut wake = false;
    let mut no_wake = false;
    let mut split_anchor = None;
    let mut alive = BTreeSet::new();
    let mut zombie_agents = false;
    let mut yes = false;
    let mut print = false;
    let mut positional = Vec::new();
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(VIEW_USAGE.to_owned()),
            "--clean" => clean = true,
            "--kill" => kill = true,
            "--readonly" | "--read-only" | "-r" => readonly = true,
            "--wake" => wake = true,
            "--no-wake" => no_wake = true,
            "--split" => split_anchor = Some(ViewSplitAnchor::Active),
            "--zombie-agents" => zombie_agents = true,
            "--yes" | "-y" => yes = true,
            "--print" => print = true,
            "--alive" => {
                let Some(value) = argv.get(index + 1) else { return Err("view: missing --alive value".to_owned()); };
                alive.insert(value.clone());
                index += 1;
            }
            arg if arg.starts_with("--split=") => split_anchor = Some(ViewSplitAnchor::Target(arg["--split=".len()..].to_owned())),
            arg if arg.starts_with("--alive=") => { alive.insert(arg["--alive=".len()..].to_owned()); }
            arg if arg.starts_with('-') => return Err(format!("view: unknown argument {arg}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if zombie_agents && positional.is_empty() {
        positional.push("__zombie_cleanup__".to_owned());
    }
    if positional.is_empty() { return Err(VIEW_USAGE.to_owned()); }
    if positional.len() > 2 { return Err("view: too many positional arguments".to_owned()); }
    Ok(ViewArgs { agent: positional[0].clone(), window_hint: positional.get(1).cloned(), clean, kill, readonly, wake, no_wake, split_anchor, alive, zombie_agents, yes, print })
}

fn view_validate_args(args: &ViewArgs) -> Result<(), String> {
    if !args.zombie_agents { view_validate_user_value(&args.agent, "agent")?; }
    if let Some(window) = &args.window_hint { view_validate_user_value(window, "window")?; }
    if args.wake && args.no_wake { return Err("--wake and --no-wake cannot be combined".to_owned()); }
    for alive in &args.alive { view_validate_tmux_token(alive, "alive session")?; }
    if let Some(ViewSplitAnchor::Target(anchor)) = &args.split_anchor { view_validate_user_value(anchor, "split anchor")?; }
    Ok(())
}

fn view_validate_user_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.bytes().any(|byte| byte == 0 || byte < 0x20 || byte == 0x7f) {
        return Err(format!("view {label} must be non-empty, unpadded, not start with '-', and contain no NUL/control characters"));
    }
    Ok(())
}

fn view_validate_tmux_token(value: &str, label: &str) -> Result<(), String> {
    view_validate_user_value(value, label)?;
    if value.chars().any(char::is_whitespace) || value.contains(';') || value.contains('`') || value.contains('$') {
        return Err(format!("view {label} contains unsafe shell/tmux characters"));
    }
    Ok(())
}

fn view_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> BTreeSet<String> {
    runner.run("list-sessions", &["-F".to_owned(), "#{session_name}".to_owned()])
        .map(|raw| raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(ToOwned::to_owned).collect())
        .unwrap_or_default()
}

fn view_list_windows<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Vec<ViewWindowRef> {
    runner.run("list-windows", &["-a".to_owned(), "-F".to_owned(), "#{session_name}|||#{window_index}|||#{window_name}".to_owned()])
        .map(|raw| view_parse_windows(&raw))
        .unwrap_or_default()
}

fn view_parse_windows(raw: &str) -> Vec<ViewWindowRef> {
    raw.lines().filter_map(|line| {
        let mut parts = line.split("|||");
        let session = parts.next()?.trim();
        let index = parts.next()?.trim();
        let name = parts.next()?.trim();
        if session.is_empty() || index.is_empty() { return None; }
        Some(ViewWindowRef { session: session.to_owned(), index: index.to_owned(), name: name.to_owned() })
    }).collect()
}

fn view_parse_panes(raw: &str) -> Vec<ViewPaneRef> {
    raw.lines().filter_map(|line| {
        let mut parts = line.split("|||");
        let id = parts.next()?.trim();
        let command = parts.next().unwrap_or_default().trim();
        let target = parts.next().unwrap_or_default().trim();
        let title = parts.next().unwrap_or_default().trim();
        if id.is_empty() { return None; }
        Some(ViewPaneRef { id: id.to_owned(), command: command.to_owned(), target: target.to_owned(), title: title.to_owned() })
    }).collect()
}

fn view_resolve_session(agent: &str, sessions: &BTreeSet<String>, windows: &[ViewWindowRef]) -> Result<Option<String>, CliOutput> {
    let candidates: Vec<String> = sessions.iter().filter(|name| !name.ends_with("-view-view")).cloned().collect();
    match resolve_session_target(agent, &candidates) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => Ok(Some(matched)),
        ResolveResult::Ambiguous { candidates: matches } => {
            let mut list = String::new();
            for name in &matches {
                let _ = writeln!(list, "  \x1b[90m    • {name}\x1b[0m");
            }
            Err(CliOutput { code: 2, stdout: String::new(), stderr: format!("  \x1b[31m✗\x1b[0m '{agent}' is ambiguous — matches {} sessions:\n{list}  \x1b[90m  use the full name: maw view <exact-session>\x1b[0m\n", matches.len()) })
        }
        ResolveResult::None { .. } => {
            let agent_lower = agent.to_lowercase();
            Ok(windows.iter().find(|window| window.name.to_lowercase().contains(&agent_lower) && !window.session.ends_with("-view-view")).map(|window| window.session.clone()))
        }
    }
}

fn view_missing_session(args: &ViewArgs) -> Result<CliOutput, CliOutput> {
    if args.wake {
        return Ok(CliOutput { code: 0, stdout: format!("\x1b[36m⚡\x1b[0m waking '{}'...\n  → maw wake {} --attach\n", args.agent, args.agent), stderr: String::new() });
    }
    if args.no_wake || !view_stdin_is_terminal() {
        return Err(CliOutput { code: 1, stdout: String::new(), stderr: format!("  \x1b[90m  try: maw ls\x1b[0m\nsession not found for: {}\n", args.agent) });
    }
    Err(CliOutput { code: 1, stdout: String::new(), stderr: format!("  \x1b[90m  try: maw ls\x1b[0m\nsession not found for: {}\n", args.agent) })
}

fn view_view_name(session_name: &str, window_hint: Option<&str>) -> String {
    let base = session_name.trim_start_matches(|c: char| c.is_ascii_digit() || c == '-');
    match window_hint { Some(window) => format!("{base}-view-{window}"), None => format!("{base}-view") }
}

fn view_has_session<R: maw_tmux::TmuxRunner>(runner: &mut R, name: &str) -> bool {
    runner.run("has-session", &["-t".to_owned(), name.to_owned()]).is_ok()
}

fn view_new_grouped_session<R: maw_tmux::TmuxRunner>(runner: &mut R, parent: &str, name: &str) -> Result<(), CliOutput> {
    runner.run("new-session", &["-d".to_owned(), "-t".to_owned(), parent.to_owned(), "-s".to_owned(), name.to_owned()])
        .map_err(|error| command_target_error("view", &format!("failed to create grouped session {name}: {}", error.message)))?;
    runner.run("set-option", &["-t".to_owned(), name.to_owned(), "window-size".to_owned(), "largest".to_owned()]).ok();
    Ok(())
}

#[allow(clippy::format_push_string)]
fn view_select_window_if_requested<R: maw_tmux::TmuxRunner>(runner: &mut R, stdout: &mut String, source_session: &str, view_name: &str, window_hint: Option<&str>, windows: &[ViewWindowRef]) -> Result<(), CliOutput> {
    let Some(hint) = window_hint else { return Ok(()); };
    if let Some(window) = windows.iter().find(|window| window.session == source_session && (window.name == hint || window.name.contains(hint) || window.index == hint)) {
        let target = format!("{view_name}:{}", window.index);
        view_validate_tmux_token(&target, "window target").map_err(|message| command_target_error("view", &message))?;
        runner.run("select-window", &["-t".to_owned(), target]).ok();
        stdout.push_str(&format!("\x1b[36mwindow\x1b[0m  → {} ({})\n", window.name, window.index));
    } else {
        stdout.push_str(&format!("\x1b[33mwarn\x1b[0m: window '{hint}' not found, using default\n"));
    }
    Ok(())
}

fn view_tmux_set<R: maw_tmux::TmuxRunner>(runner: &mut R, target: &str, option: &str, value: &str) -> Result<(), CliOutput> {
    view_validate_tmux_token(target, "set target").map_err(|message| command_target_error("view", &message))?;
    view_validate_tmux_token(option, "set option").map_err(|message| command_target_error("view", &message))?;
    view_validate_tmux_token(value, "set value").map_err(|message| command_target_error("view", &message))?;
    runner.run("set", &["-t".to_owned(), target.to_owned(), option.to_owned(), value.to_owned()]).map(|_| ()).map_err(|error| command_target_error("view", &error.message))
}

fn view_resolve_split_anchor<R: maw_tmux::TmuxRunner>(runner: &mut R, anchor: &ViewSplitAnchor) -> Result<Option<String>, CliOutput> {
    match anchor {
        ViewSplitAnchor::Active => Ok(None),
        ViewSplitAnchor::Target(value) if value.contains(':') => {
            view_validate_tmux_token(value, "split anchor").map_err(|message| command_target_error("view", &message))?;
            Ok(Some(value.clone()))
        }
        ViewSplitAnchor::Target(value) => {
            let view_name = format!("{}-view", value.trim_end_matches("-view"));
            view_validate_tmux_token(&view_name, "split anchor view").map_err(|message| command_target_error("view", &message))?;
            if !view_has_session(runner, &view_name) {
                let sessions = view_list_sessions(runner);
                let windows = view_list_windows(runner);
                let Some(parent) = view_resolve_session(value, &sessions, &windows)?.filter(|name| !name.ends_with("-view")) else {
                    return Err(command_target_error("view", &format!("--split={value}: no matching session or existing view")));
                };
                view_new_grouped_session(runner, &parent, &view_name)?;
            }
            Ok(Some(format!("{view_name}:0")))
        }
    }
}

fn view_split_view<R: maw_tmux::TmuxRunner>(runner: &mut R, view_name: &str, anchor_pane: Option<&str>) -> Result<(), CliOutput> {
    let target = anchor_pane.unwrap_or(view_name);
    view_validate_tmux_token(target, "split target").map_err(|message| command_target_error("view", &message))?;
    let command = format!("tmux switch-client -t {view_name}");
    runner.run("split-window", &["-h".to_owned(), "-l".to_owned(), "50%".to_owned(), "-t".to_owned(), target.to_owned(), command]).map(|_| ()).map_err(|error| command_target_error("view", &error.message))
}

fn view_attach_or_switch<R: maw_tmux::TmuxRunner>(runner: &mut R, session: &str, readonly: bool) -> Result<(), CliOutput> {
    view_validate_tmux_token(session, "attach session").map_err(|message| command_target_error("view", &message))?;
    if std::env::var_os("TMUX").is_some() {
        let mut args = Vec::new();
        if readonly { args.push("-r".to_owned()); }
        args.extend(["-t".to_owned(), session.to_owned()]);
        runner.run("switch-client", &args).map(|_| ()).map_err(|error| command_target_error("view", &error.message))
    } else {
        let mut args = Vec::new();
        if readonly { args.push("-r".to_owned()); }
        args.extend(["-t".to_owned(), session.to_owned()]);
        runner.run("attach-session", &args).map(|_| ()).map_err(|error| command_target_error("view", &error.message))
    }
}

fn view_render_attach_line(session: &str, clean: bool, readonly: bool) -> String {
    format!("\x1b[36mattach\x1b[0m  → {session}{}{}\n", if clean { " (clean)" } else { "" }, if readonly { " (read-only)" } else { "" })
}

fn view_render_attach_plan(session: &str, readonly: bool) -> String {
    format!("Run: tmux attach-session {}-t {session}\n  detach with: Ctrl-b d\n", if readonly { "-r " } else { "" })
}

fn view_kill_session_guarded<R: maw_tmux::TmuxRunner>(runner: &mut R, view_name: &str, parent_session: &str) -> Result<(), CliOutput> {
    view_validate_tmux_token(view_name, "kill session").map_err(|message| command_target_error("view", &message))?;
    if !view_name.ends_with("-view") && !view_name.contains("-view-") {
        return Err(command_target_error("view", &format!("refusing to kill non-view session {view_name}")));
    }
    let fresh_sessions = view_list_sessions(runner);
    if !fresh_sessions.contains(view_name) {
        return Err(command_target_error("view", &format!("refusing to kill {view_name}: session disappeared before kill")));
    }
    if !parent_session.ends_with("-view") && !fresh_sessions.contains(parent_session) {
        return Err(command_target_error("view", &format!("refusing to kill {view_name}: parent session {parent_session} is not live")));
    }
    runner.run("kill-session", &["-t".to_owned(), view_name.to_owned()]).map(|_| ()).map_err(|error| command_target_error("view", &format!("kill failed for {view_name}: {}", error.message)))
}

#[allow(clippy::format_push_string)]
fn view_cleanup_zombie_agents<R: maw_tmux::TmuxRunner>(runner: &mut R, yes: bool) -> Result<String, String> {
    let raw = runner.run("list-panes", &["-a".to_owned(), "-F".to_owned(), "#{pane_id}|||#{pane_current_command}|||#{session_name}:#{window_index}.#{pane_index}|||#{pane_title}".to_owned()]).map_err(|error| error.message)?;
    let panes = view_parse_panes(&raw);
    let zombies = view_find_zombie_panes(&panes);
    let mut stdout = "\x1b[36mScanning tmux panes...\x1b[0m\n".to_owned();
    if zombies.is_empty() {
        stdout.push_str("\x1b[32m✓\x1b[0m No zombie agent panes found.\n");
        return Ok(stdout);
    }
    stdout.push_str(&format!("\n\x1b[33m{}\x1b[0m orphan claude pane(s) to kill:\n\n", zombies.len()));
    for zombie in &zombies { stdout.push_str(&format!("  \x1b[33m{}\x1b[0m  {}  \x1b[90m(team: unknown — DELETED)\x1b[0m\n", zombie.pane_id, zombie.info)); }
    if !yes {
        stdout.push_str("\nRun with \x1b[36m--yes\x1b[0m to kill them.\n");
        return Ok(stdout);
    }
    stdout.push_str("\x1b[36mKilling...\x1b[0m\n");
    for zombie in zombies {
        view_kill_pane_guarded(runner, &zombie.pane_id)?;
        stdout.push_str(&format!("\x1b[32m✓\x1b[0m killed {}\n", zombie.pane_id));
    }
    Ok(stdout)
}

fn view_find_zombie_panes(panes: &[ViewPaneRef]) -> Vec<ViewZombiePane> {
    let safe_pane_ids: BTreeSet<String> = panes.iter().filter(|pane| view_is_fleet_or_view_target(&pane.target) || view_is_primary_oracle_pane(&pane.target)).map(|pane| pane.id.clone()).collect();
    panes.iter().filter(|pane| {
        pane.command.contains("claude") && !safe_pane_ids.contains(&pane.id) && !view_is_fleet_or_view_target(&pane.target) && !view_is_primary_oracle_pane(&pane.target)
    }).map(|pane| ViewZombiePane { pane_id: pane.id.clone(), info: format!("{}  \"{}\"", pane.target, pane.title.chars().take(50).collect::<String>()) }).collect()
}

fn view_is_fleet_or_view_target(target: &str) -> bool {
    let session = target.split(':').next().unwrap_or_default();
    session == "maw-view" || session.ends_with("-view") || session.strip_prefix(|c: char| c.is_ascii_digit() || c == '-').is_some_and(|stem| !stem.is_empty() && target.starts_with(session) && !stem.contains("team"))
}

fn view_is_primary_oracle_pane(target: &str) -> bool {
    target.split_once(':').and_then(|(_, rest)| rest.split_once('.')).is_some_and(|(window, pane)| window == "1" && pane == "0")
}

fn view_kill_pane_guarded<R: maw_tmux::TmuxRunner>(runner: &mut R, pane_id: &str) -> Result<(), String> {
    view_validate_pane_id(pane_id)?;
    let fresh = runner.run("list-panes", &["-a".to_owned(), "-F".to_owned(), "#{pane_id}|||#{pane_current_command}|||#{session_name}:#{window_index}.#{pane_index}|||#{pane_title}".to_owned()]).map_err(|error| error.message)?;
    let panes = view_parse_panes(&fresh);
    let zombies = view_find_zombie_panes(&panes);
    if !zombies.iter().any(|zombie| zombie.pane_id == pane_id) {
        return Err(format!("refusing to kill {pane_id}: pane ownership changed before kill"));
    }
    runner.run("kill-pane", &["-t".to_owned(), pane_id.to_owned()]).map(|_| ()).map_err(|error| error.message)
}

fn view_validate_pane_id(value: &str) -> Result<(), String> {
    if !value.strip_prefix('%').is_some_and(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit())) {
        return Err(format!("unsafe pane id '{value}'"));
    }
    Ok(())
}

fn view_usage_error(message: &str) -> CliOutput {
    if message == VIEW_USAGE {
        CliOutput { code: 2, stdout: String::new(), stderr: format!("{VIEW_USAGE}\n") }
    } else {
        CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n{VIEW_USAGE}\n") }
    }
}

fn view_should_plan_without_mutation(args: &ViewArgs) -> bool {
    (args.print || !view_stdout_is_terminal())
        && !args.clean
        && !args.kill
        && args.split_anchor.is_none()
        && !args.zombie_agents
}

fn view_stdout_is_terminal() -> bool { std::io::IsTerminal::is_terminal(&std::io::stdout()) }
fn view_stdin_is_terminal() -> bool { std::io::IsTerminal::is_terminal(&std::io::stdin()) }

#[cfg(test)]
mod view_tests_260 {
    use super::*;

    #[derive(Default)]
    struct ViewFakeRunner { calls: Vec<(String, Vec<String>)>, sessions: BTreeSet<String>, windows: String, panes: String }

    impl maw_tmux::TmuxRunner for ViewFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-sessions" => Ok(self.sessions.iter().cloned().collect::<Vec<_>>().join("\n") + "\n"),
                "list-windows" => Ok(self.windows.clone()),
                "list-panes" => Ok(self.panes.clone()),
                "has-session" => {
                    if args.get(1).is_some_and(|name| self.sessions.contains(name)) { Ok(String::new()) } else { Err(maw_tmux::TmuxError::new("missing")) }
                }
                "new-session" => {
                    if let Some(pos) = args.iter().position(|arg| arg == "-s") { if let Some(name) = args.get(pos + 1) { self.sessions.insert(name.clone()); } }
                    Ok(String::new())
                }
                _ => Ok(String::new()),
            }
        }
    }

    fn view_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn view_dispatch_fragment_owns_view_in_part260() { assert_eq!(DISPATCH_260[0].command, "view"); }

    #[test]
    fn view_clean_kill_relists_before_kill_session() {
        let mut runner = ViewFakeRunner { sessions: ["50-mawjs".to_owned()].into_iter().collect(), windows: "50-mawjs|||0|||main\n".to_owned(), ..ViewFakeRunner::default() };
        let out = view_run_with_runner(&view_strings(&["mawjs", "--clean", "--kill", "--print"]), &mut runner).unwrap();
        assert_eq!(out.code, 0);
        let calls = runner.calls.iter().map(|(cmd, _)| cmd.as_str()).collect::<Vec<_>>();
        assert!(calls.windows(2).any(|w| w == ["list-sessions", "kill-session"]));
        assert!(runner.calls.iter().any(|(cmd, args)| cmd == "set" && args == &vec!["-t".to_owned(), "mawjs-view".to_owned(), "status".to_owned(), "off".to_owned()]));
    }

    #[test]
    fn view_split_anchor_bootstraps_anchor_view_and_splits() {
        let mut runner = ViewFakeRunner { sessions: ["50-mawjs".to_owned(), "51-anchor".to_owned()].into_iter().collect(), windows: "50-mawjs|||0|||main\n51-anchor|||0|||main\n".to_owned(), ..ViewFakeRunner::default() };
        let out = view_run_with_runner(&view_strings(&["mawjs", "--split=anchor", "--print"]), &mut runner).unwrap();
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("split   → mawjs-view"));
        assert!(runner.calls.iter().any(|(cmd, args)| cmd == "split-window" && args.iter().any(|arg| arg == "anchor-view:0")));
    }

    #[test]
    fn view_zombie_agents_requires_yes_and_fresh_relist_before_kill() {
        let panes = "%9|||claude|||team-old:2.1|||stale\n%1|||claude|||50-mawjs:1.0|||live\n".to_owned();
        let mut runner = ViewFakeRunner { panes, ..ViewFakeRunner::default() };
        let preview = view_run_with_runner(&view_strings(&["--zombie-agents"]), &mut runner).unwrap();
        assert!(preview.stdout.contains("Run with"));
        assert!(!runner.calls.iter().any(|(cmd, _)| cmd == "kill-pane"));
        runner.calls.clear();
        let killed = view_run_with_runner(&view_strings(&["--zombie-agents", "--yes"]), &mut runner).unwrap();
        assert!(killed.stdout.contains("killed %9"));
        let calls = runner.calls.iter().map(|(cmd, _)| cmd.as_str()).collect::<Vec<_>>();
        assert!(calls.windows(2).any(|w| w == ["list-panes", "kill-pane"]));
    }

    #[test]
    fn view_rejects_dash_nul_and_control_input_before_tmux() {
        let mut runner = ViewFakeRunner::default();
        for bad in ["-bad", "bad\nname", "bad\0name"] {
            let out = view_run_with_runner(&[bad.to_owned()], &mut runner).unwrap_err();
            assert_ne!(out.code, 0);
        }
        assert!(runner.calls.is_empty());
    }
}
