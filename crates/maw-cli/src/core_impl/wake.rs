const DISPATCH_64: &[DispatcherEntry] = &[DispatcherEntry { command: "wake", handler: Handler::Async(wake_async_native) }];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct WakeOptionsNative {
    target: String,
    task: Option<String>,
    wt: Option<String>,
    prompt: Option<String>,
    repo: Option<String>,
    issue: Option<String>,
    pr: Option<String>,
    incubate: Option<String>,
    parent: Option<String>,
    peer: Option<String>,
    layout: Option<String>,
    from: Option<String>,
    snapshot: Option<String>,
    engine: Option<String>,
    name: Option<String>,
    repo_path: Option<std::path::PathBuf>,
    on_ready: Vec<String>,
    all: bool,
    all_local: bool,
    attach: bool,
    dry_run: bool,
    fresh: bool,
    from_snapshot: bool,
    kill: bool,
    list: bool,
    main: bool,
    new_window: bool,
    no_attach: bool,
    pick: bool,
    resume: bool,
    solo: bool,
    split: bool,
    bud: bool,
    channels: bool,
    wait: bool,
    yes: bool,
}


#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeResolvedNative {
    oracle: String,
    session: String,
    window: String,
    repo_path: std::path::PathBuf,
    repo_fuzzy_match: Option<String>,
    repo_warning: Option<String>,
    command: String,
    command_warnings: Vec<String>,
    target: String,
}







trait WakeTmuxNative {
    fn wake_list(&mut self) -> Vec<TmuxSession>;
    fn wake_has_session(&mut self, name: &str) -> bool;
    fn wake_new_session(&mut self, name: &str, window: &str, cwd: &std::path::Path) -> Result<(), String>;
    fn wake_new_window(&mut self, session: &str, window: &str, cwd: &std::path::Path) -> Result<(), String>;
    fn wake_send_text(&mut self, target: &str, text: &str) -> Result<(), String>;
    fn wake_send_text_detached(&mut self, target: String, text: String) -> Result<Option<std::thread::JoinHandle<()>>, String> {
        self.wake_send_text(&target, &text)?;
        Ok(None)
    }
    fn wake_select_window(&mut self, target: &str) -> Result<(), String>;
    fn wake_target_pane_id(&mut self, target: &str) -> Result<String, String>;
    fn wake_pane_current_command(&mut self, target: &str) -> Result<String, String>;
    fn wake_pane_capture(&mut self, target: &str) -> Result<String, String>;
    fn wake_confirm_poll_sleep(&mut self, delay: std::time::Duration) { std::thread::sleep(delay); }
}

struct WakeNativeTmux;

impl WakeTmuxNative for WakeNativeTmux {
    fn wake_list(&mut self) -> Vec<TmuxSession> { TmuxClient::local().list_all() }

    fn wake_has_session(&mut self, name: &str) -> bool { TmuxClient::local().has_session(name) }

    fn wake_new_session(&mut self, name: &str, window: &str, cwd: &std::path::Path) -> Result<(), String> {
        wake_validate_tmux_name(name, "session")?;
        wake_validate_tmux_name(window, "window")?;
        wake_validate_cwd(cwd)?;
        let mut tmux = TmuxClient::local();
        let opts = maw_tmux::NewSessionOptions {
            window: Some(window.to_owned()),
            cwd: Some(cwd.display().to_string()),
            detached: true,
            command: None,
            print_format: None,
        };
        tmux.new_session(name, &opts).map(|_| ()).map_err(|error| error.to_string())
    }

    fn wake_new_window(&mut self, session: &str, window: &str, cwd: &std::path::Path) -> Result<(), String> {
        wake_validate_tmux_name(session, "session")?;
        wake_validate_tmux_name(window, "window")?;
        wake_validate_cwd(cwd)?;
        TmuxClient::local().new_window(session, window, Some(&cwd.display().to_string())).map_err(|error| error.to_string())
    }

    fn wake_send_text(&mut self, target: &str, text: &str) -> Result<(), String> {
        wake_validate_tmux_target(target)?;
        TmuxClient::local().send_text(target, text).map(|_| ()).map_err(|error| error.to_string())
    }

    fn wake_send_text_detached(&mut self, target: String, text: String) -> Result<Option<std::thread::JoinHandle<()>>, String> {
        wake_validate_tmux_target(&target)?;
        std::thread::Builder::new()
            .name("maw-wake-send-text".to_owned())
            .spawn(move || {
                let mut tmux = WakeNativeTmux;
                let _ = tmux.wake_send_text(&target, &text);
            })
            .map(Some)
            .map_err(|error| format!("wake: failed to spawn engine sender: {error}"))
    }

    fn wake_pane_current_command(&mut self, target: &str) -> Result<String, String> {
        wake_validate_tmux_target(target)?;
        TmuxClient::local().display_pane_current_command(target).map_err(|error| error.to_string())
    }

    fn wake_target_pane_id(&mut self, target: &str) -> Result<String, String> {
        wake_validate_tmux_target(target)?;
        TmuxClient::local()
            .first_pane_id(target)
            .ok_or_else(|| format!("wake: target pane not found: {target}"))
    }

    fn wake_pane_capture(&mut self, target: &str) -> Result<String, String> {
        wake_validate_tmux_target(target)?;
        // Visible screen only — deliberately no `-S` history depth: scrollback
        // could contain a stale trust prompt from an earlier run in the same
        // pane and cause a false positive.
        let output = std::process::Command::new("tmux")
            .args(["capture-pane", "-p", "-t", target])
            .output()
            .map_err(|error| format!("wake: failed to execute tmux capture-pane: {error}"))?;
        if !output.status.success() {
            return Err(format!("wake: tmux capture-pane exited with status {}", output.status));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    fn wake_select_window(&mut self, target: &str) -> Result<(), String> {
        wake_validate_tmux_target(target)?;
        let session = target.split(':').next().unwrap_or(target);
        let mut tmux = TmuxClient::local();
        if std::env::var_os("TMUX").is_some() {
            tmux.switch_client(session);
            tmux.select_window(target);
            return Ok(());
        }
        tmux.select_window(target);
        let status = std::process::Command::new("tmux")
            .arg("attach-session")
            .arg("-t")
            .arg(session)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|error| format!("wake: failed to execute tmux attach-session: {error}"))?;
        if status.success() { Ok(()) } else { Err(format!("wake: tmux attach-session exited with status {status}")) }
    }
}

fn wake_async_native(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move {
        if wants_help(&args, wake_help_value_flags()) {
            return help_output(wake_usage());
        }
        match wake_parse_args(&args) {
            Ok(options) if wake_should_use_peer_target(&options) => run_wake_async(args).await,
            Ok(_) => run_wake_command(&args),
            Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
        }
    })
}

fn run_wake_command(argv: &[String]) -> CliOutput {
    if wants_help(argv, wake_help_value_flags()) {
        return help_output(wake_usage());
    }
    let mut fleet_wake = |args: &[String]| run_fleet_command(args);
    run_wake_command_with(argv, &mut WakeNativeTmux, &mut fleet_wake)
}

fn run_wake_command_with(
    argv: &[String],
    tmux: &mut impl WakeTmuxNative,
    fleet_wake: &mut impl FnMut(&[String]) -> CliOutput,
) -> CliOutput {
    let options = match wake_parse_args(argv) {
        Ok(options) => options,
        Err(message) => return CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    };
    let sessions = tmux.wake_list();
    if let Some(output) = wake_picker_output(&options, &sessions, tmux, fleet_wake) { return output; }
    match wake_run_options(&options, &sessions, tmux) {
        Ok((code, stdout)) => CliOutput { code, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn wake_run(argv: &[String], tmux: &mut impl WakeTmuxNative) -> Result<(i32, String), String> {
    let options = wake_parse_args(argv)?;
    let sessions = tmux.wake_list();
    wake_run_options(&options, &sessions, tmux)
}

fn wake_run_options(options: &WakeOptionsNative, sessions: &[TmuxSession], tmux: &mut impl WakeTmuxNative) -> Result<(i32, String), String> {
    if options.list { return Ok((0, wake_render_list(options, sessions))); }
    if options.all { return Ok((0, wake_render_all_plan(options, sessions))); }
    if let Some(result) = wake_attach_live_registry_session(options, sessions, tmux) {
        return result;
    }
    let mut out = String::new();
    let started = std::time::Instant::now();
    let resolved = wake_resolve(options, sessions)?;
    wake_record_phase(&resolved, "resolve", wake_elapsed_ms(started), &mut out, true);
    if options.dry_run { return Ok((0, wake_render_dry_run(options, &resolved))); }
    wake_apply(options, &resolved, tmux, &mut out)?;
    Ok((0, out))
}

fn wake_attach_live_registry_session(
    options: &WakeOptionsNative,
    sessions: &[TmuxSession],
    tmux: &mut impl WakeTmuxNative,
) -> Option<Result<(i32, String), String>> {
    let requested_session = options.parent.as_deref()?;
    if !options.attach || options.dry_run || options.target != requested_session {
        return None;
    }
    let registry_has_session = fleet_load_entries()
        .into_iter()
        .filter(fleet_entry_is_session)
        .any(|entry| entry.session.name == requested_session || entry.file == requested_session);
    if !registry_has_session {
        return None;
    }
    let live = sessions.iter().find(|session| session.name == requested_session)?;
    let window = live.windows.iter().find(|window| window.active).or_else(|| live.windows.first())?;
    let target = format!("{requested_session}:{}", window.name);
    Some(tmux.wake_select_window(&target).map(|()| (0, String::new())))
}

fn wake_picker_output(
    options: &WakeOptionsNative,
    sessions: &[TmuxSession],
    tmux: &mut impl WakeTmuxNative,
    fleet_wake: &mut impl FnMut(&[String]) -> CliOutput,
) -> Option<CliOutput> {
    let (context, rows) = wake_picker_rows(options, sessions)?;
    let execute_without_prompt = rows.len() == 1
        && (options.yes
            || (options.dry_run
                && rows[0].matched.candidate.kind == maw_matcher::ResolveCandidateKind::FleetSquad));
    if execute_without_prompt {
        return Some(wake_run_picker_row(&rows[0], options, sessions, tmux, fleet_wake));
    }
    if !wake_stdin_is_terminal() {
        return Some(CliOutput {
            code: 1,
            stdout: picker_render_text("wake", &options.target, context, &rows),
            stderr: String::new(),
        });
    }
    Some(wake_prompt_picker(&options.target, context, &rows).map_or_else(
        || CliOutput { code: 1, stdout: String::new(), stderr: "wake: picker cancelled\n".to_owned() },
        |row| wake_run_picker_row(&row, options, sessions, tmux, fleet_wake),
    ))
}

fn wake_stdin_is_terminal() -> bool {
    use std::io::IsTerminal as _;
    std::io::stdin().is_terminal()
}

fn wake_prompt_picker(target: &str, context: &str, rows: &[PickerRow]) -> Option<PickerRow> {
    use std::io::Write as _;
    eprint!("{}", picker_render_text("wake", target, context, rows));
    let yes_hint = if rows.len() == 1 { ", Enter/y" } else { "" };
    loop {
        eprint!("pick [1-{}]{yes_hint} or q: ", rows.len());
        let _ = std::io::stderr().flush();
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line).is_err() { return None; }
        match picker_parse_selection(&line, rows.len()) {
            PickerSelection::Pick(index) => return rows.get(index).cloned(),
            PickerSelection::Quit => return None,
            PickerSelection::Invalid => eprintln!("wake: enter a number from 1 to {} or q", rows.len()),
        }
    }
}

fn wake_picker_rows(options: &WakeOptionsNative, sessions: &[TmuxSession]) -> Option<(&'static str, Vec<PickerRow>)> {
    if options.list || options.all || options.target.contains(':') || wake_should_bypass_typed_resolution(options) {
        return None;
    }
    let alive = sessions.iter().map(|session| session.name.clone()).collect::<BTreeSet<_>>();
    let candidates = local_resolver_candidates(&alive);
    let (context, matches) = match maw_matcher::resolve_typed_target(&options.target, &candidates) {
        maw_matcher::ResolveTypedResult::Match { matched }
            if options.pick
                || matched.rank == maw_matcher::ResolveMatchRank::Fuzzy
                || matched.candidate.kind == maw_matcher::ResolveCandidateKind::FleetSquad =>
        {
            ("is not a native wake target", vec![matched])
        }
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => {
            let preferred = wake_preferred_matches(candidates);
            if preferred.len() == 1
                && !options.pick
                && preferred[0].rank != maw_matcher::ResolveMatchRank::Fuzzy
                && preferred[0].candidate.kind != maw_matcher::ResolveCandidateKind::FleetSquad
            {
                return None;
            }
            ("matches multiple targets", preferred)
        }
        maw_matcher::ResolveTypedResult::None =>
            ("was not found exactly", deadend_closest_matches(&options.target, &candidates)),
        maw_matcher::ResolveTypedResult::Match { .. } => return None,
    };
    let rows = matches.into_iter().filter_map(wake_picker_row).collect::<Vec<_>>();
    (!rows.is_empty()).then_some((context, rows))
}

fn wake_preferred_matches(candidates: Vec<maw_matcher::ResolveMatch>) -> Vec<maw_matcher::ResolveMatch> {
    let Some(priority) = candidates.iter().map(|matched| wake_kind_priority(matched.candidate.kind)).min() else { return Vec::new(); };
    candidates.into_iter().filter(|matched| wake_kind_priority(matched.candidate.kind) == priority).collect()
}

fn wake_kind_priority(kind: maw_matcher::ResolveCandidateKind) -> u8 {
    match kind {
        maw_matcher::ResolveCandidateKind::SleepingRegistry => 0,
        maw_matcher::ResolveCandidateKind::Oracle | maw_matcher::ResolveCandidateKind::Repo => 1,
        maw_matcher::ResolveCandidateKind::LiveSession | maw_matcher::ResolveCandidateKind::Window => 2,
        maw_matcher::ResolveCandidateKind::FleetSquad => 3,
        maw_matcher::ResolveCandidateKind::Peer => 4,
    }
}

fn wake_picker_row(matched: maw_matcher::ResolveMatch) -> Option<PickerRow> {
    let action = match matched.candidate.kind {
        maw_matcher::ResolveCandidateKind::FleetSquad => format!("maw fleet wake {}", matched.candidate.name),
        maw_matcher::ResolveCandidateKind::Peer => return None,
        _ => format!("maw wake {}", matched.candidate.name),
    };
    Some(PickerRow { detail: attach_picker_detail(&matched), matched, action })
}

fn wake_run_picker_row(
    row: &PickerRow,
    options: &WakeOptionsNative,
    sessions: &[TmuxSession],
    tmux: &mut impl WakeTmuxNative,
    fleet_wake: &mut impl FnMut(&[String]) -> CliOutput,
) -> CliOutput {
    if let Err(message) = wake_validate_target_value(&row.matched.candidate.name, "picker target") {
        return CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") };
    }
    if row.matched.candidate.kind == maw_matcher::ResolveCandidateKind::FleetSquad {
        let mut args = vec!["wake".to_owned(), row.matched.candidate.name.clone()];
        if options.dry_run { args.push("--dry-run".to_owned()); }
        if options.kill { args.push("--kill".to_owned()); }
        if options.resume { args.push("--resume".to_owned()); }
        return fleet_wake(&args);
    }
    let mut selected = options.clone();
    selected.target.clone_from(&row.matched.candidate.name);
    selected.pick = false;
    selected.yes = false;
    match wake_run_options(&selected, sessions, tmux) {
        Ok((code, stdout)) => CliOutput { code, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn wake_should_use_peer_target(options: &WakeOptionsNative) -> bool {
    if options.dry_run || options.list || options.all || options.repo.is_some() || options.incubate.is_some() { return false; }
    if workon_github_slug(&options.target).is_some() { return false; }
    options.target.contains(':') || options.peer.is_some()
}






















fn wake_render_list(options: &WakeOptionsNative, sessions: &[TmuxSession]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[36mwake\x1b[0m live sessions for {}", wake_label(options));
    if sessions.is_empty() { out.push_str("  no live sessions\n"); }
    for session in sessions {
        let _ = writeln!(out, "  - {} ({} windows)", session.name, session.windows.len());
    }
    out
}

fn wake_render_all_plan(options: &WakeOptionsNative, sessions: &[TmuxSession]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\x1b[36mwake\x1b[0m all plan");
    let _ = writeln!(out, "  all-local: {}", options.all_local);
    let _ = writeln!(out, "  dry-run: {}", options.dry_run);
    for session in sessions { let _ = writeln!(out, "  - {}", session.name); }
    out
}

fn wake_label(options: &WakeOptionsNative) -> String {
    if options.target.is_empty() { "all".to_owned() } else { options.target.clone() }
}

fn wake_resolve(options: &WakeOptionsNative, sessions: &[TmuxSession]) -> Result<WakeResolvedNative, String> {
    let fleet_entries = fleet_load_entries().into_iter().filter(fleet_entry_is_session).collect::<Vec<_>>();
    let initial_oracle = wake_oracle(options)?;
    let typed = wake_typed_resolution(options, &initial_oracle, &fleet_entries, sessions)?;
    let typed_session_hint = typed.as_ref().and_then(|resolution| resolution.session_hint.clone());
    let matched_window = typed.as_ref().and_then(|resolution| resolution.matched_window.clone());
    let oracle = typed.as_ref().map_or_else(|| initial_oracle.clone(), |resolution| resolution.oracle.clone());
    let repo = typed.map_or_else(|| wake_repo_path(options, &oracle, &fleet_entries), |resolution| Ok(resolution.repo))?;
    let repo_path = repo.path;
    let session_hint = typed_session_hint.or_else(|| wake_registry_session_hint(&initial_oracle, &repo_path, &fleet_entries, sessions));
    let session = options
        .parent
        .clone()
        .or_else(|| wake_detect_session(&oracle, sessions))
        .or(session_hint)
        .or_else(|| wake_detect_session_from_fleet_registry(&oracle, &repo_path, &fleet_entries))
        .unwrap_or_else(|| wake_session_name(&oracle, sessions));
    let window = wake_window_name(options, &oracle, matched_window.as_deref());
    let target = format!("{session}:{window}");
    let (command, command_warnings) = wake_command(&window, &repo_path, options);
    Ok(WakeResolvedNative {
        oracle,
        session,
        window,
        repo_path,
        repo_fuzzy_match: repo.fuzzy_match,
        repo_warning: repo.warning,
        command,
        command_warnings,
        target,
    })
}

























































fn wake_render_dry_run(options: &WakeOptionsNative, resolved: &WakeResolvedNative) -> String {
    let mut out = String::new();
    if let Some(warning) = &resolved.repo_warning { let _ = writeln!(out, "\x1b[33mwarning:\x1b[0m {warning}"); }
    for warning in &resolved.command_warnings { let _ = writeln!(out, "\x1b[33mwarning:\x1b[0m {warning}"); }
    if let Some(name) = &resolved.repo_fuzzy_match {
        let _ = writeln!(out, "\x1b[36m→\x1b[0m fuzzy match: {name}");
    }
    let _ = writeln!(out, "\x1b[36m→\x1b[0m found \x1b[1m{}\x1b[0m ({})", resolved.oracle, resolved.repo_path.display());
    out.push_str("\x1b[90mdry-run — no tmux sessions/windows will be changed\x1b[0m\n");
    let _ = writeln!(out, "\x1b[32m+\x1b[0m would wake window '{}' in session '{}'", resolved.window, resolved.session);
    if options.task.is_some() || options.wt.is_some() {
        let _ = writeln!(out, "\x1b[33m⚡\x1b[0m would wake worktree/task: {}", options.wt.as_deref().or(options.task.as_deref()).unwrap_or_default());
    }
    out
}

fn wake_elapsed_ms(started: std::time::Instant) -> u64 { u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX) }

fn wake_record_phase(resolved: &WakeResolvedNative, phase: &str, ms: u64, out: &mut String, pre_attach: bool) {
    if pre_attach && ms > 300 {
        let _ = writeln!(out, "\x1b[36m→\x1b[0m wake {phase} took {ms}ms");
    }
    wake_write_phase_audit(resolved, phase, ms);
}

fn wake_write_phase_audit(resolved: &WakeResolvedNative, phase: &str, ms: u64) {
    let row = serde_json::json!({
        "ts": cli_dispatch_now_iso(),
        "event": "wake.phase",
        "cmd": "wake",
        "phase": phase,
        "ms": ms,
        "session": resolved.session,
        "window": resolved.window,
        "target": resolved.target,
        "binary": "maw-rs",
        "version": MAW_RS_BUILD_VERSION,
    });
    let _ = append_jsonl_atomic(&audit_jsonl_path(&current_xdg_env()), &row);
}

fn wake_apply(
    options: &WakeOptionsNative,
    resolved: &WakeResolvedNative,
    tmux: &mut impl WakeTmuxNative,
    out: &mut String,
) -> Result<(), String> {
    let started = std::time::Instant::now();
    if !resolved.repo_path.is_dir() { return Err(format!("wake: repo path missing: {}", resolved.repo_path.display())); }
    wake_record_phase(resolved, "repo-check", wake_elapsed_ms(started), out, true);
    if let Some(warning) = &resolved.repo_warning { let _ = writeln!(out, "\x1b[33mwarning:\x1b[0m {warning}"); }
    for warning in &resolved.command_warnings { let _ = writeln!(out, "\x1b[33mwarning:\x1b[0m {warning}"); }
    if let Some(name) = &resolved.repo_fuzzy_match {
        let _ = writeln!(out, "\x1b[36m→\x1b[0m fuzzy match: {name}");
    }
    let started = std::time::Instant::now();
    let session_exists = tmux.wake_has_session(&resolved.session);
    wake_record_phase(resolved, "session-probe", wake_elapsed_ms(started), out, true);
    let started = std::time::Instant::now();
    let deferred_send = if session_exists {
        wake_create_or_reuse_window(options, resolved, tmux, out)?
    } else {
        wake_create_session(options, resolved, tmux, out)?
    };
    wake_record_phase(resolved, "first-window", wake_elapsed_ms(started), out, true);
    if options.attach {
        let send_thread = if deferred_send {
            tmux.wake_send_text_detached(resolved.target.clone(), resolved.command.clone())?
        } else { None };
        wake_record_phase(resolved, "attach", 0, out, false);
        let attach_result = tmux.wake_select_window(&resolved.target);
        if let Some(send_thread) = send_thread {
            send_thread.join().map_err(|_| "wake: engine sender thread panicked".to_owned())?;
        }
        attach_result?;
    }
    let started = std::time::Instant::now();
    wake_register_fleet_session(resolved, tmux)?;
    wake_record_phase(resolved, "fleet-upsert", wake_elapsed_ms(started), out, false);
    let started = std::time::Instant::now();
    let hooks = wake_post_wake_hooks(options, &resolved.repo_path);
    wake_run_post_wake_hooks(&resolved.oracle, &resolved.session, &resolved.window, &hooks);
    wake_record_phase(resolved, "post-wake-hooks", wake_elapsed_ms(started), out, false);
    Ok(())
}


fn wake_post_wake_hooks(options: &WakeOptionsNative, cwd: &std::path::Path) -> Vec<String> {
    let mut hooks = wake_config_post_wake_hooks(Some(cwd));
    hooks.extend(options.on_ready.iter().cloned());
    hooks
}

/// `cwd: Some(path)` resolves `hooks.postWake` dir-aware against the resolved
/// repo path; `None` keeps the process-cwd (global) read — used by fleet group
/// hooks, where a squad has no single repo path (per-member resolution is a
/// follow-up to #600).
fn wake_config_post_wake_hooks(cwd: Option<&std::path::Path>) -> Vec<String> {
    let config = cwd.map_or_else(merged_config_value, merged_config_value_in_dir);
    config
        .pointer("/hooks/postWake")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

fn wake_run_post_wake_hooks(oracle: &str, session: &str, window: &str, hooks: &[String]) {
    for hook in hooks.iter().map(String::as_str).map(str::trim).filter(|hook| !hook.is_empty()) {
        let _ = std::process::Command::new("sh")
            .arg("-c")
            .arg(hook)
            .env("MAW_ORACLE", oracle)
            .env("MAW_SESSION", session)
            .env("MAW_WINDOW", window)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}











fn wake_create_session(options: &WakeOptionsNative, resolved: &WakeResolvedNative, tmux: &mut impl WakeTmuxNative, out: &mut String) -> Result<bool, String> {
    tmux.wake_new_session(&resolved.session, &resolved.window, &resolved.repo_path)?;
    wake_wait_for_shell_ready(tmux, &resolved.target);
    if options.attach {
        let _ = writeln!(out, "\x1b[32m+\x1b[0m created session '{}' (main: {})", resolved.session, resolved.window);
        return Ok(true);
    }
    tmux.wake_send_text(&resolved.target, &resolved.command)?;
    wake_confirm_engine_launch(tmux, &resolved.target, &resolved.command)?;
    let _ = writeln!(out, "\x1b[32m+\x1b[0m created session '{}' (attach: maw a {})", resolved.session, resolved.session);
    Ok(false)
}

fn wake_create_or_reuse_window(
    options: &WakeOptionsNative,
    resolved: &WakeResolvedNative,
    tmux: &mut impl WakeTmuxNative,
    out: &mut String,
) -> Result<bool, String> {
    let windows = tmux.wake_list().into_iter().find(|session| session.name == resolved.session).map(|session| session.windows).unwrap_or_default();
    let mut self_pane_launch = false;
    if !options.new_window && windows.iter().any(|window| window.name == resolved.window) {
        self_pane_launch = wake_target_is_current_pane(tmux, &resolved.target);
        if !self_pane_launch {
            match tmux.wake_pane_current_command(&resolved.target) {
                Ok(command) if wake_pane_command_is_shell(&command) => {}
                Ok(_) | Err(_) => {
                    let _ = writeln!(out, "\x1b[32m⚡\x1b[0m '{}' running in {}", resolved.window, resolved.session);
                    return Ok(false);
                }
            }
        }
    } else {
        tmux.wake_new_window(&resolved.session, &resolved.window, &resolved.repo_path)?;
        wake_wait_for_shell_ready(tmux, &resolved.target);
    }
    if options.attach {
        let _ = writeln!(out, "\x1b[32m✅\x1b[0m woke '{}' in {} → {}", resolved.window, resolved.session, resolved.repo_path.display());
        return Ok(true);
    }
    tmux.wake_send_text(&resolved.target, &resolved.command)?;
    if !self_pane_launch {
        wake_confirm_engine_launch(tmux, &resolved.target, &resolved.command)?;
    }
    let _ = writeln!(out, "\x1b[32m✅\x1b[0m woke '{}' in {} → {}", resolved.window, resolved.session, resolved.repo_path.display());
    Ok(false)
}

fn wake_register_fleet_session(
    resolved: &WakeResolvedNative,
    tmux: &mut impl WakeTmuxNative,
) -> Result<(), String> {
    let windows = wake_registry_windows(resolved, tmux);
    if windows.is_empty() {
        return Ok(());
    }
    fleet_registry_upsert_session(&resolved.session, &windows, "maw wake")
        .map(|_| ())
        .map_err(|error| format!("wake: {error}"))
}

fn wake_registry_windows(
    resolved: &WakeResolvedNative,
    tmux: &mut impl WakeTmuxNative,
) -> Vec<FleetWindowSummary> {
    let mut windows = tmux
        .wake_list()
        .into_iter()
        .find(|session| session.name == resolved.session)
        .map_or_else(Vec::new, |session| fleet_registry_windows_from_tmux(&session.windows, None));
    if !windows.iter().any(|window| window.name == resolved.window) {
        if let Some(repo) = fleet_repo_slug_from_path(&resolved.repo_path, None) {
            windows.push(FleetWindowSummary {
                name: resolved.window.clone(),
                repo,
                kind: Some(fleet_kind_from_window_name(&resolved.window)),
            });
        }
    }
    windows
}

