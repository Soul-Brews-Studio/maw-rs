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

type WakeEqualsSetter = fn(&mut WakeOptionsNative, &str) -> Result<(), String>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeResolvedNative {
    oracle: String,
    session: String,
    window: String,
    repo_path: std::path::PathBuf,
    repo_fuzzy_match: Option<String>,
    repo_warning: Option<String>,
    command: String,
    target: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeRepoResolution {
    path: std::path::PathBuf,
    fuzzy_match: Option<String>,
    warning: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeRepoCandidate {
    name: String,
    path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeTypedRegistryCandidate {
    candidate: maw_matcher::ResolveTypedCandidate,
    oracle: String,
    session: String,
    repo_path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeTypedRepoCandidate {
    candidate: maw_matcher::ResolveTypedCandidate,
    path: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeTypedResolution {
    oracle: String,
    repo: WakeRepoResolution,
    session_hint: Option<String>,
}

impl maw_matcher::Named for WakeRepoCandidate {
    fn name(&self) -> &str { &self.name }
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
    let mut out = String::new();
    let started = std::time::Instant::now();
    let resolved = wake_resolve(options, sessions)?;
    wake_record_phase(&resolved, "resolve", wake_elapsed_ms(started), &mut out, true);
    if options.dry_run { return Ok((0, wake_render_dry_run(options, &resolved))); }
    wake_apply(options, &resolved, tmux, &mut out)?;
    Ok((0, out))
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

fn wake_parse_args(argv: &[String]) -> Result<WakeOptionsNative, String> {
    let mut options = wake_default_options();
    let mut positionals = Vec::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if let Some(consumed) = wake_parse_value_arg(argv, index, &mut options)? { index += consumed; continue; }
        if wake_parse_bool_arg(arg, &mut options)? { index += 1; continue; }
        if arg.starts_with('-') { return Err(format!("wake: unknown argument {arg}")); }
        wake_validate_target_value(arg, "target")?;
        positionals.push(arg.clone());
        index += 1;
    }
    wake_finalize_options(options, &positionals)
}

fn wake_default_options() -> WakeOptionsNative {
    WakeOptionsNative {
        target: String::new(), task: None, wt: None, prompt: None, repo: None, issue: None, pr: None,
        incubate: None, parent: None, peer: None, layout: None, from: None, snapshot: None, engine: None,
        name: None, repo_path: None, on_ready: Vec::new(), all: false, all_local: false, attach: true, dry_run: false, fresh: false,
        from_snapshot: false, kill: false, list: false, main: false, new_window: false, no_attach: false,
        pick: false, resume: false, solo: false, split: false, bud: false, channels: false, wait: false, yes: false,
    }
}

fn wake_parse_value_arg(argv: &[String], index: usize, options: &mut WakeOptionsNative) -> Result<Option<usize>, String> {
    let arg = &argv[index];
    let consumed = match arg.as_str() {
        "--task" => { options.task = Some(wake_take_text(argv, index, "--task")?); 2 }
        "--wt" => { options.wt = Some(wake_take_value(argv, index, "--wt", wake_validate_slug)?); 2 }
        "--prompt" => { options.prompt = Some(wake_take_text(argv, index, "--prompt")?); 2 }
        "--repo" => { options.repo = Some(wake_take_value(argv, index, "--repo", wake_validate_repo)?); 2 }
        "--issue" => { options.issue = Some(wake_take_value(argv, index, "--issue", wake_validate_issue)?); 2 }
        "--pr" => { options.pr = Some(wake_take_value(argv, index, "--pr", wake_validate_issue)?); 2 }
        "--incubate" => { options.incubate = Some(wake_take_value(argv, index, "--incubate", wake_validate_repo)?); 2 }
        "--parent" | "--session" => { options.parent = Some(wake_take_value(argv, index, arg, wake_validate_target_value)?); 2 }
        "--peer" | "--from" => { wake_set_peer_or_from(options, arg, &wake_take_value(argv, index, arg, wake_validate_target_value)?); 2 }
        "--layout" => { options.layout = Some(wake_take_value(argv, index, "--layout", wake_validate_layout)?); 2 }
        "--snapshot" => { options.snapshot = Some(wake_take_value(argv, index, "--snapshot", wake_validate_target_value)?); 2 }
        "-e" | "--engine" => { options.engine = Some(wake_take_value(argv, index, arg, wake_validate_target_value)?); 2 }
        "--name" => { options.name = Some(wake_take_value(argv, index, "--name", wake_validate_slug)?); 2 }
        "--repo-path" => { options.repo_path = Some(std::path::PathBuf::from(wake_take_value(argv, index, "--repo-path", wake_validate_target_value)?)); 2 }
        "--on-ready" => { options.on_ready.push(wake_take_text(argv, index, "--on-ready")?); 2 }
        _ => return wake_parse_equals_arg(arg, options),
    };
    Ok(Some(consumed))
}

fn wake_parse_equals_arg(arg: &str, options: &mut WakeOptionsNative) -> Result<Option<usize>, String> {
    for (flag, setter) in wake_equals_setters() {
        if let Some(value) = arg.strip_prefix(flag) {
            setter(options, value)?;
            return Ok(Some(1));
        }
    }
    Ok(None)
}

fn wake_equals_setters() -> Vec<(&'static str, WakeEqualsSetter)> {
    vec![
        ("--task=", |o, v| { wake_validate_text(v, "--task")?; o.task = Some(v.to_owned()); Ok(()) }),
        ("--wt=", |o, v| { wake_validate_slug(v, "--wt")?; o.wt = Some(v.to_owned()); Ok(()) }),
        ("--prompt=", |o, v| { wake_validate_text(v, "--prompt")?; o.prompt = Some(v.to_owned()); Ok(()) }),
        ("--repo=", |o, v| { wake_validate_repo(v, "--repo")?; o.repo = Some(v.to_owned()); Ok(()) }),
        ("--issue=", |o, v| { wake_validate_issue(v, "--issue")?; o.issue = Some(v.to_owned()); Ok(()) }),
        ("--pr=", |o, v| { wake_validate_issue(v, "--pr")?; o.pr = Some(v.to_owned()); Ok(()) }),
        ("--incubate=", |o, v| { wake_validate_repo(v, "--incubate")?; o.incubate = Some(v.to_owned()); Ok(()) }),
        ("--parent=", |o, v| { wake_validate_target_value(v, "--parent")?; o.parent = Some(v.to_owned()); Ok(()) }),
        ("--peer=", |o, v| { wake_validate_target_value(v, "--peer")?; o.peer = Some(v.to_owned()); Ok(()) }),
        ("--from=", |o, v| { wake_validate_target_value(v, "--from")?; o.from = Some(v.to_owned()); Ok(()) }),
        ("--layout=", |o, v| { wake_validate_layout(v, "--layout")?; o.layout = Some(v.to_owned()); Ok(()) }),
        ("--snapshot=", |o, v| { wake_validate_target_value(v, "--snapshot")?; o.snapshot = Some(v.to_owned()); Ok(()) }),
        ("--engine=", |o, v| { wake_validate_target_value(v, "--engine")?; o.engine = Some(v.to_owned()); Ok(()) }),
        ("--name=", |o, v| { wake_validate_slug(v, "--name")?; o.name = Some(v.to_owned()); Ok(()) }),
        ("--on-ready=", |o, v| { wake_validate_text(v, "--on-ready")?; o.on_ready.push(v.to_owned()); Ok(()) }),
    ]
}

fn wake_parse_bool_arg(arg: &str, options: &mut WakeOptionsNative) -> Result<bool, String> {
    match arg {
        "--all" => options.all = true,
        "all" => { options.all = true; if options.target.is_empty() { "all".clone_into(&mut options.target); } }
        "--all-local" => options.all_local = true,
        "--attach" | "-a" => { options.attach = true; options.no_attach = false; }
        "--no-attach" => { options.attach = false; options.no_attach = true; }
        "--dry-run" => options.dry_run = true,
        "--fresh" => options.fresh = true,
        "--from-snapshot" => options.from_snapshot = true,
        "--kill" => options.kill = true,
        "--list" => options.list = true,
        "--main" => { options.main = true; options.solo = true; }
        "--new" => options.new_window = true,
        "--pick" => options.pick = true,
        "--resume" => options.resume = true,
        "--solo" => options.solo = true,
        "--split" => options.split = true,
        "--bud" => options.bud = true,
        "--channels" => options.channels = true,
        "--wait" => options.wait = true,
        "--yes" | "-y" => options.yes = true,
        "-h" | "--help" => return Err(wake_usage()),
        _ => return Ok(false),
    }
    Ok(true)
}

fn wake_set_peer_or_from(options: &mut WakeOptionsNative, flag: &str, value: &str) {
    if flag == "--peer" { options.peer = Some(value.to_owned()); } else { options.from = Some(value.to_owned()); }
}

fn wake_take_value(
    argv: &[String],
    index: usize,
    flag: &str,
    validate: fn(&str, &str) -> Result<(), String>,
) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("wake: missing {flag} value"))?;
    validate(value, flag)?;
    Ok(value.clone())
}

fn wake_take_text(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("wake: missing {flag} value"))?;
    wake_validate_text(value, flag)?;
    Ok(value.clone())
}

fn wake_finalize_options(mut options: WakeOptionsNative, positionals: &[String]) -> Result<WakeOptionsNative, String> {
    if options.all && positionals.is_empty() { return Ok(options); }
    if positionals.len() != 1 { return Err(wake_usage()); }
    options.target.clone_from(&positionals[0]);
    Ok(options)
}

fn wake_usage() -> String {
    "usage: maw wake <target|all> [--task <slug>|--wt <slug>] [--repo <org/repo>] [--prompt <text>] [--on-ready <cmd>] [--all --all-local --attach|-a --no-attach --dry-run --fresh --from-snapshot --kill --layout <nested|legacy> --list --main --new --parent <session> --peer <node> --pick --resume --snapshot <id> --solo --split --yes|-y]".to_owned()
}

fn wake_help_value_flags() -> &'static [&'static str] {
    &[
        "--task",
        "--wt",
        "--prompt",
        "--repo",
        "--issue",
        "--pr",
        "--incubate",
        "--parent",
        "--session",
        "--peer",
        "--from",
        "--layout",
        "--snapshot",
        "-e",
        "--engine",
        "--name",
        "--repo-path",
        "--on-ready",
    ]
}

fn wake_validate_target_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') { return Err(format!("wake: {label} must not start with '-'")); }
    if value.contains('\0') || value.contains('\n') || value.contains('\r') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.starts_with('-') { return Err(format!("wake: {label} must not start with '-'")); }
    if value.contains('\0') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_slug(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/')) {
        return Err(format!("wake: invalid {label}"));
    }
    Ok(())
}

fn wake_validate_repo(value: &str, label: &str) -> Result<(), String> {
    wake_validate_slug(value, label)?;
    if value.contains("..") { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_issue(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if !value.chars().all(|ch| ch.is_ascii_digit() || ch == '#') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_layout(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if matches!(value, "nested" | "legacy") { Ok(()) } else { Err(format!("wake: invalid {label}")) }
}

fn wake_validate_tmux_name(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if value.contains(':') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_tmux_target(value: &str) -> Result<(), String> {
    wake_validate_target_value(value, "tmux target")?;
    if !value.contains(':') { return Err("wake: invalid tmux target".to_owned()); }
    Ok(())
}

fn wake_validate_cwd(path: &std::path::Path) -> Result<(), String> {
    if !path.is_dir() { return Err(format!("wake: cwd missing: {}", path.display())); }
    Ok(())
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
    let fleet_entries = fleet_load_entries();
    let initial_oracle = wake_oracle(options)?;
    let typed = wake_typed_resolution(options, &initial_oracle, &fleet_entries)?;
    let typed_session_hint = typed.as_ref().and_then(|resolution| resolution.session_hint.clone());
    let oracle = typed.as_ref().map_or_else(|| initial_oracle.clone(), |resolution| resolution.oracle.clone());
    let repo = typed.map_or_else(|| wake_repo_path(options, &oracle, &fleet_entries), |resolution| Ok(resolution.repo))?;
    let repo_path = repo.path;
    let session_hint = typed_session_hint.or_else(|| wake_registry_session_hint(&initial_oracle, &repo_path, &fleet_entries));
    let session = options
        .parent
        .clone()
        .or_else(|| wake_detect_session(&oracle, sessions))
        .or(session_hint)
        .or_else(|| wake_detect_session_from_fleet_registry(&oracle, &repo_path, &fleet_entries))
        .unwrap_or_else(|| wake_session_name(&oracle, sessions));
    let window = wake_window_name(options, &oracle);
    let target = format!("{session}:{window}");
    let command = wake_command(&window, &repo_path, options);
    Ok(WakeResolvedNative {
        oracle,
        session,
        window,
        repo_path,
        repo_fuzzy_match: repo.fuzzy_match,
        repo_warning: repo.warning,
        command,
        target,
    })
}

fn wake_oracle(options: &WakeOptionsNative) -> Result<String, String> {
    let slug = workon_github_slug(&options.target);
    let raw = options
        .name
        .as_deref()
        .or_else(|| slug.as_deref().and_then(|value| value.rsplit('/').next()))
        .or_else(|| options.target.trim_end_matches('/').split('/').next_back())
        .unwrap_or(&options.target);
    let raw = raw.strip_suffix(".git").unwrap_or(raw);
    let oracle = raw.strip_suffix("-oracle").unwrap_or(raw).trim();
    wake_validate_slug(oracle, "oracle")?;
    Ok(oracle.to_owned())
}

fn wake_typed_resolution(options: &WakeOptionsNative, oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<Option<WakeTypedResolution>, String> {
    if wake_should_bypass_typed_resolution(options) { return Ok(None); }
    if let Some(resolution) = wake_resolve_exact_registry_session(&options.target, fleet_entries)? { return Ok(Some(resolution)); }
    if let Some(resolution) = wake_resolve_registry_target(&options.target, fleet_entries)? { return Ok(Some(resolution)); }
    wake_resolve_repo_target(oracle, fleet_entries).map(Some)
}

fn wake_should_bypass_typed_resolution(options: &WakeOptionsNative) -> bool {
    options.repo_path.is_some()
        || options.repo.is_some()
        || options.incubate.is_some()
        || workon_github_slug(&options.target).is_some()
        || options.target == "."
        || options.target.starts_with("./")
        || options.target.starts_with('/')
}

fn wake_repo_path(options: &WakeOptionsNative, oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<WakeRepoResolution, String> {
    // `--repo-path <dir>` is an explicit filesystem override (used by `team up`
    // to point at the bound worktree) — it bypasses ghq/fleet resolution.
    if let Some(repo_path) = &options.repo_path {
        return wake_normalize_repo_path(repo_path).map(wake_exact_repo_resolution);
    }
    if let Some(repo) = &options.repo { return wake_resolve_workon_repo(repo); }
    if let Some(repo) = &options.incubate { return wake_resolve_workon_repo(repo); }
    if workon_github_slug(&options.target).is_some()
        || options.target == "."
        || options.target.starts_with("./")
        || options.target.starts_with('/')
    {
        return wake_resolve_workon_repo(&options.target);
    }
    wake_find_repo(oracle, fleet_entries)
}

fn wake_normalize_repo_path(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|error| format!("wake: cannot resolve repo path: {error}"))?
            .join(path)
    };
    Ok(absolute.canonicalize().unwrap_or(absolute))
}

fn wake_ghq_root() -> std::path::PathBuf { ghq_root() }

fn wake_exact_repo_resolution(path: std::path::PathBuf) -> WakeRepoResolution {
    WakeRepoResolution { path, fuzzy_match: None, warning: None }
}

fn wake_resolve_workon_repo(input: &str) -> Result<WakeRepoResolution, String> {
    let repo = workon_resolve_repo(input).map_err(|error| format!("wake: {error}"))?;
    Ok(wake_exact_repo_resolution(repo.repo_path))
}

fn wake_find_repo(oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<WakeRepoResolution, String> {
    if let Some(path) = wake_registry_repo_for_oracle(oracle, fleet_entries) {
        if path.is_dir() { return Ok(wake_exact_repo_resolution(path)); }
        if let Some((_, fallback)) = wake_oracles_repo_fallback(&[oracle]) { return Ok(fallback); }
        return Err(wake_registry_missing_repo_message(oracle, &path));
    }
    wake_resolve_repo_target(oracle, fleet_entries).map(|resolution| resolution.repo)
}

fn wake_resolve_exact_registry_session(target: &str, fleet_entries: &[NativeFleetEntry]) -> Result<Option<WakeTypedResolution>, String> {
    let matches = fleet_entries
        .iter()
        .filter(|entry| entry.session.name == target || entry.file == target)
        .collect::<Vec<_>>();
    let Some(entry) = matches.first() else { return Ok(None); };
    if matches.len() > 1 {
        return Err(format!("wake: ambiguous registry session for {target}"));
    }
    let stem = maw_identity::parse_session_name(&entry.session.name).stem;
    let Some(window) = wake_primary_registry_window(entry, &stem) else { return Ok(None); };
    let Some(path) = native_fleet_repo_path(&window.repo) else { return Ok(None); };
    let oracle = wake_oracle_from_repo_slug(&window.repo).unwrap_or_else(|| stem.clone());
    let (oracle, repo) = if path.is_dir() {
        (oracle, wake_exact_repo_resolution(path))
    } else {
        wake_oracles_repo_fallback(&[&stem, &oracle])
            .ok_or_else(|| wake_registry_missing_repo_message(&entry.session.name, &path))?
    };
    Ok(Some(WakeTypedResolution {
        oracle,
        repo,
        session_hint: Some(entry.session.name.clone()),
    }))
}

fn wake_primary_registry_window<'a>(entry: &'a NativeFleetEntry, stem: &str) -> Option<&'a NativeFleetWindow> {
    entry
        .session
        .windows
        .iter()
        .find(|window| window.name == stem)
        .or_else(|| entry.session.windows.first())
}

fn wake_resolve_registry_target(target: &str, fleet_entries: &[NativeFleetEntry]) -> Result<Option<WakeTypedResolution>, String> {
    let candidates = wake_typed_registry_candidates(fleet_entries);
    let typed = candidates.iter().map(|candidate| candidate.candidate.clone()).collect::<Vec<_>>();
    match maw_matcher::resolve_typed_target(target, &typed) {
        maw_matcher::ResolveTypedResult::None => Ok(None),
        maw_matcher::ResolveTypedResult::Match { matched } => {
            let candidate = candidates
                .into_iter()
                .find(|candidate| candidate.candidate == matched.candidate)
                .ok_or_else(|| format!("wake: internal resolver mismatch for {target}"))?;
            let stem = maw_identity::parse_session_name(&candidate.session).stem;
            let (oracle, repo) = if candidate.repo_path.is_dir() {
                (candidate.oracle, wake_exact_repo_resolution(candidate.repo_path))
            } else {
                wake_oracles_repo_fallback(&[target, &stem, &candidate.oracle])
                    .ok_or_else(|| wake_registry_missing_repo_message(&candidate.session, &candidate.repo_path))?
            };
            Ok(Some(WakeTypedResolution {
                oracle,
                repo,
                session_hint: Some(candidate.session),
            }))
        }
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => Err(format!(
            "wake: ambiguous registry target for {target}: {}",
            candidates.into_iter().map(|candidate| candidate.candidate.name).collect::<Vec<_>>().join(", ")
        )),
    }
}

fn wake_resolve_repo_target(oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Result<WakeTypedResolution, String> {
    let candidates = wake_typed_repo_candidates(fleet_entries);
    let typed = candidates.iter().map(|candidate| candidate.candidate.clone()).collect::<Vec<_>>();
    match maw_matcher::resolve_typed_target(oracle, &typed) {
        maw_matcher::ResolveTypedResult::Match { matched } => {
            let candidate = candidates
                .into_iter()
                .find(|candidate| candidate.candidate == matched.candidate)
                .ok_or_else(|| format!("wake: internal resolver mismatch for {oracle}"))?;
            let fuzzy_match = (matched.rank == maw_matcher::ResolveMatchRank::Fuzzy).then_some(candidate.candidate.name);
            let oracle = wake_oracle_from_repo_path(&candidate.path).unwrap_or_else(|| oracle.to_owned());
            Ok(WakeTypedResolution {
                oracle,
                repo: WakeRepoResolution { path: candidate.path, fuzzy_match, warning: None },
                session_hint: None,
            })
        }
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => Err(format!(
            "wake: ambiguous fuzzy repo for {oracle}: {}",
            candidates.into_iter().map(|candidate| candidate.candidate.name).collect::<Vec<_>>().join(", ")
        )),
        maw_matcher::ResolveTypedResult::None => Err(wake_repo_not_found_message(oracle, &typed)),
    }
}

fn wake_repo_not_found_message(oracle: &str, candidates: &[maw_matcher::ResolveTypedCandidate]) -> String {
    let mut all = candidates.to_vec();
    all.extend(deadend_oracle_candidates());
    let suggestions = deadend_suggestion_matches(oracle, &all);
    let mut out = deadend_suggestions_text("wake", oracle, &suggestions);
    out.push_str("  next: maw oracle scan  # refresh oracles.json\n  next: maw ls -a        # inspect live/sleeping sessions\n");
    out
}

fn deadend_oracle_candidates() -> Vec<maw_matcher::ResolveTypedCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(cache) = locate_load_registry_cache() {
        for oracle in cache.oracles {
            if oracle.name.is_empty() || !seen.insert(oracle.name.to_lowercase()) { continue; }
            let aliases = [oracle.repo, oracle.local_path].into_iter().filter(|value| !value.is_empty()).collect::<Vec<_>>();
            candidates.push(maw_matcher::ResolveTypedCandidate { kind: maw_matcher::ResolveCandidateKind::Oracle, name: oracle.name, aliases });
        }
    }
    for repo in wake_repo_candidates(&[]) {
        let name = wake_oracle_from_repo_path(&repo.path).unwrap_or(repo.name);
        if name.is_empty() || !seen.insert(name.to_lowercase()) { continue; }
        candidates.push(maw_matcher::ResolveTypedCandidate { kind: maw_matcher::ResolveCandidateKind::Oracle, name, aliases: Vec::new() });
    }
    candidates
}

fn deadend_suggestion_matches(target: &str, candidates: &[maw_matcher::ResolveTypedCandidate]) -> Vec<maw_matcher::ResolveMatch> {
    match maw_matcher::resolve_typed_target(target, candidates) {
        maw_matcher::ResolveTypedResult::Match { matched } => vec![matched],
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => candidates.into_iter().take(5).collect(),
        maw_matcher::ResolveTypedResult::None => deadend_closest_matches(target, candidates),
    }
}

fn deadend_closest_matches(target: &str, candidates: &[maw_matcher::ResolveTypedCandidate]) -> Vec<maw_matcher::ResolveMatch> {
    let targets = maw_matcher::normalized_match_names(target);
    let mut scored = Vec::<(usize, String, maw_matcher::ResolveTypedCandidate)>::new();
    for candidate in candidates {
        let names = std::iter::once(candidate.name.as_str()).chain(candidate.aliases.iter().map(String::as_str)).flat_map(maw_matcher::normalized_match_names).collect::<Vec<_>>();
        let Some(distance) = targets.iter().flat_map(|target| names.iter().map(|name| deadend_edit_distance(target, name))).min() else { continue; };
        if distance <= 2 || (target.len() > 6 && distance <= 3) {
            scored.push((distance, candidate.name.clone(), candidate.clone()));
        }
    }
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut seen = BTreeSet::new();
    scored.into_iter().filter_map(|(_, name, candidate)| seen.insert(name).then_some(maw_matcher::ResolveMatch { rank: maw_matcher::ResolveMatchRank::Fuzzy, candidate })).take(5).collect()
}

fn deadend_edit_distance(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut costs = (0..=right_chars.len()).collect::<Vec<_>>();
    for (row, left_char) in left.chars().enumerate() {
        let mut previous = costs[0];
        costs[0] = row + 1;
        for (col, right_char) in right_chars.iter().enumerate() {
            let insert = costs[col + 1] + 1;
            let delete = costs[col] + 1;
            let replace = previous + usize::from(left_char != *right_char);
            previous = costs[col + 1];
            costs[col + 1] = insert.min(delete).min(replace);
        }
    }
    *costs.last().unwrap_or(&0)
}

fn deadend_suggestions_text(command: &str, target: &str, candidates: &[maw_matcher::ResolveMatch]) -> String {
    use std::fmt::Write as _;

    let mut out = if command == "wake" {
        format!("wake: repo not found for {target}\n")
    } else {
        format!("{command}: '{target}' not found\n")
    };
    if !candidates.is_empty() {
        out.push_str("Did you mean:\n");
        for matched in candidates {
            let action = match (command, matched.candidate.kind) {
                ("attach", maw_matcher::ResolveCandidateKind::LiveSession | maw_matcher::ResolveCandidateKind::Window) => format!("maw attach {}", matched.candidate.name),
                ("attach", maw_matcher::ResolveCandidateKind::FleetSquad) => format!("maw fleet wake {}", matched.candidate.name),
                ("attach", _) => format!("maw wake {} --attach", matched.candidate.name),
                ("wake", _) => format!("maw wake {}", matched.candidate.name),
                _ => matched.candidate.name.clone(),
            };
            let _ = writeln!(out, "  - {} → {action}", matched.candidate.name);
        }
    }
    out
}

fn wake_registry_repo_for_oracle(oracle: &str, fleet_entries: &[NativeFleetEntry]) -> Option<std::path::PathBuf> {
    let mut repos = BTreeSet::new();
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let repo = window.repo.strip_prefix("github.com/").unwrap_or(&window.repo);
            let Some(name) = repo.rsplit('/').next() else { continue; };
            if !wake_repo_name_matches(name, oracle) {
                continue;
            }
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            let _ = repos.insert(wake_canonicalize_path(&path));
        }
    }
    if repos.len() == 1 {
        repos.into_iter().next()
    } else {
        None
    }
}

fn wake_oracles_repo_fallback(names: &[&str]) -> Option<(String, WakeRepoResolution)> {
    let entry = locate_load_registry_cache()?.oracles.into_iter().find(|entry| {
        names.iter().any(|name| entry.name.eq_ignore_ascii_case(name))
    })?;
    let path = std::path::PathBuf::from(entry.local_path.trim());
    if !path.is_dir() { return None; }
    let path = wake_canonicalize_path(&path);
    let warning = format!("registry repo stale, using oracles.json: {}", path.display());
    Some((entry.name, WakeRepoResolution { path, fuzzy_match: None, warning: Some(warning) }))
}

fn wake_registry_session_hint(oracle: &str, repo_path: &std::path::Path, fleet_entries: &[NativeFleetEntry]) -> Option<String> {
    wake_resolve_registry_target(oracle, fleet_entries)
        .ok()
        .flatten()
        .filter(|resolution| wake_canonicalize_path(&resolution.repo.path) == wake_canonicalize_path(repo_path))
        .and_then(|resolution| resolution.session_hint)
}

fn wake_typed_registry_candidates(fleet_entries: &[NativeFleetEntry]) -> Vec<WakeTypedRegistryCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            let oracle = wake_oracle_from_repo_slug(&window.repo).unwrap_or_else(|| window.name.clone());
            let name = format!("{}:{}", entry.session.name, window.name);
            if !seen.insert((name.clone(), path.clone())) { continue; }
            candidates.push(WakeTypedRegistryCandidate {
                candidate: maw_matcher::ResolveTypedCandidate {
                    kind: maw_matcher::ResolveCandidateKind::SleepingRegistry,
                    name,
                    aliases: wake_registry_aliases(window, &oracle),
                },
                oracle,
                session: entry.session.name.clone(),
                repo_path: path,
            });
        }
    }
    candidates
}

fn wake_registry_aliases(window: &NativeFleetWindow, oracle: &str) -> Vec<String> {
    let mut aliases = vec![window.name.clone(), oracle.to_owned()];
    if let Some(repo_name) = window.repo.rsplit('/').next().filter(|name| !name.is_empty()) { aliases.push(repo_name.to_owned()); }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn wake_typed_repo_candidates(fleet_entries: &[NativeFleetEntry]) -> Vec<WakeTypedRepoCandidate> {
    wake_repo_candidates(fleet_entries)
        .into_iter()
        .map(|candidate| WakeTypedRepoCandidate {
            candidate: maw_matcher::ResolveTypedCandidate {
                kind: maw_matcher::ResolveCandidateKind::Repo,
                name: candidate.name,
                aliases: Vec::new(),
            },
            path: candidate.path,
        })
        .collect()
}

fn wake_repo_candidates(fleet_entries: &[NativeFleetEntry]) -> Vec<WakeRepoCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    let root = wake_ghq_root().join("github.com");
    if let Ok(orgs) = std::fs::read_dir(root) {
        for org in orgs.flatten() { wake_collect_repo_candidates(&org.path(), &mut candidates, &mut seen); }
    }
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            wake_push_repo_candidate(path, &mut candidates, &mut seen);
        }
    }
    candidates.sort_by(|left, right| left.path.cmp(&right.path));
    candidates
}

fn wake_collect_repo_candidates(
    org_path: &std::path::Path,
    candidates: &mut Vec<WakeRepoCandidate>,
    seen: &mut BTreeSet<std::path::PathBuf>,
) {
    let Ok(repos) = std::fs::read_dir(org_path) else { return; };
    for repo in repos.flatten() {
        let path = repo.path();
        if path.is_dir() { wake_push_repo_candidate(path, candidates, seen); }
    }
}

fn wake_push_repo_candidate(
    path: std::path::PathBuf,
    candidates: &mut Vec<WakeRepoCandidate>,
    seen: &mut BTreeSet<std::path::PathBuf>,
) {
    if !path.is_dir() || !seen.insert(path.clone()) { return; }
    let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { return; };
    candidates.push(WakeRepoCandidate { name: name.to_owned(), path });
}

fn wake_repo_name_matches(name: &str, oracle: &str) -> bool {
    name == oracle || name == format!("{oracle}-oracle") || name.trim_end_matches("-oracle") == oracle
}

fn wake_oracle_from_repo_slug(repo: &str) -> Option<String> {
    let name = repo.rsplit('/').next()?.trim();
    (!name.is_empty()).then(|| name.strip_suffix("-oracle").unwrap_or(name).to_owned())
}

fn wake_oracle_from_repo_path(path: &std::path::Path) -> Option<String> {
    path.file_name()
        .and_then(std::ffi::OsStr::to_str)
        .and_then(|name| (!name.is_empty()).then(|| name.strip_suffix("-oracle").unwrap_or(name).to_owned()))
}

fn wake_registry_missing_repo_message(name: &str, path: &std::path::Path) -> String {
    format!(
        "wake: registry entry for {name} exists, but its repo is not cloned under {}; probed {}",
        wake_ghq_root().display(),
        path.display()
    )
}

fn wake_detect_session(oracle: &str, sessions: &[TmuxSession]) -> Option<String> {
    sessions.iter().find(|session| wake_session_matches(&session.name, oracle)).map(|session| session.name.clone())
}

fn wake_detect_session_from_fleet_registry(oracle: &str, repo_path: &std::path::Path, fleet_entries: &[NativeFleetEntry]) -> Option<String> {
    let canonical = wake_canonicalize_path(repo_path);
    let mut sessions = Vec::new();
    for entry in fleet_entries {
        for window in &entry.session.windows {
            let repo_name = window.repo.rsplit('/').next().unwrap_or_default();
            if !wake_repo_name_matches(repo_name, oracle) {
                continue;
            }
            let Some(path) = native_fleet_repo_path(&window.repo) else { continue; };
            if wake_canonicalize_path(&path) == canonical {
                sessions.push(entry.session.name.clone());
            }
        }
    }
    sessions.sort();
    sessions.dedup();
    if sessions.len() == 1 { Some(sessions[0].clone()) } else { None }
}

fn wake_session_matches(name: &str, oracle: &str) -> bool {
    name == oracle || name.ends_with(&format!("-{oracle}")) || name.ends_with(&format!("-{oracle}-oracle"))
}

fn wake_session_name(oracle: &str, sessions: &[TmuxSession]) -> String {
    let start = wake_slot(oracle);
    let mut slot = start;
    for _ in 0..80 {
        if !wake_session_slot_occupied(slot, sessions) {
            return format!("{slot:02}-{oracle}");
        }
        slot = (slot % 89) + 1;
        if slot < 10 {
            slot = 10;
        }
    }
    format!("{start:02}-{oracle}")
}

fn wake_session_slot_occupied(slot: u32, sessions: &[TmuxSession]) -> bool {
    let prefix = format!("{slot:02}-");
    sessions.iter().any(|session| session.name.starts_with(&prefix))
}

fn wake_canonicalize_path(path: &std::path::Path) -> std::path::PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn wake_slot(oracle: &str) -> u32 {
    let mut hash = 0_u32;
    for byte in oracle.bytes() { hash = hash.wrapping_mul(33).wrapping_add(u32::from(byte)); }
    10 + (hash % 80)
}

fn wake_window_name(options: &WakeOptionsNative, oracle: &str) -> String {
    let suffix = options.wt.as_deref().or(options.task.as_deref()).map(wake_sanitize_branch);
    suffix.map_or_else(|| oracle.to_owned(), |task| format!("{oracle}-{task}"))
}

fn wake_sanitize_branch(value: &str) -> String {
    value.chars().map(|ch| if ch.is_ascii_alphanumeric() || ch == '-' { ch } else { '-' }).collect()
}

/// Resolve an engine alias through merged maw config `commands` (matches
/// `workon` + maw-js): custom engines like `omx-1` expand to their full shell
/// command; real binaries (codex/claude) fall through to the literal name.
/// Fixes the fleet codex-team recipe (omx-N) that previously ran a bare `omx-1`.
fn wake_resolve_engine_command(engine: &str) -> String {
    let config = merged_config_value();
    let command = config
        .get("commands")
        .and_then(|commands| {
            commands
                .get(engine)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| engine.to_owned());
    workon_prefix_zai_pool(&config, command)
}

fn wake_default_engine(options: &WakeOptionsNative) -> String {
    if let Some(engine) = &options.engine {
        return engine.clone();
    }
    if options.resume {
        return "codex".to_owned();
    }
    merged_config_value()
        .get("commands")
        .and_then(|commands| commands.get("default"))
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map_or_else(|| "codex".to_owned(), |_| "default".to_owned())
}

fn wake_command(window: &str, cwd: &std::path::Path, options: &WakeOptionsNative) -> String {
    let engine = wake_default_engine(options);
    let mut engine_command = wake_resolve_engine_command(&engine);
    if options.resume { engine_command.push_str(" resume"); }
    if options.channels { engine_command.push_str(" --channels plugin:discord@claude-plugins-official"); }
    if let Some(prompt) = &options.prompt { let _ = write!(engine_command, " {}", wake_shell_quote(prompt)); }
    let cwd_arg = wake_shell_quote(&cwd.display().to_string());
    let cwd_label = wake_shell_quote(&cwd.display().to_string());
    let command = format!(
        "cd {cwd_arg} && {{ {engine_command}; _maw_wake_status=$?; if [ $_maw_wake_status -ne 0 ]; then printf '\\nmaw wake: engine exited with status %s\\n' \"$_maw_wake_status\" >&2; fi; }} || {{ printf '\\nmaw wake: failed to cd %s; engine not started\\n' {cwd_label} >&2; }}"
    );
    format!("MAW_SESSION_WINDOW={} {}", wake_shell_quote(window), command)
}

fn wake_shell_quote(value: &str) -> String {
    if value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' | '=')) { return value.to_owned(); }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn wake_render_dry_run(options: &WakeOptionsNative, resolved: &WakeResolvedNative) -> String {
    let mut out = String::new();
    if let Some(warning) = &resolved.repo_warning { let _ = writeln!(out, "\x1b[33mwarning:\x1b[0m {warning}"); }
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
    let hooks = wake_post_wake_hooks(options);
    wake_run_post_wake_hooks(&resolved.oracle, &resolved.session, &resolved.window, &hooks);
    wake_record_phase(resolved, "post-wake-hooks", wake_elapsed_ms(started), out, false);
    Ok(())
}


fn wake_post_wake_hooks(options: &WakeOptionsNative) -> Vec<String> {
    let mut hooks = wake_config_post_wake_hooks();
    hooks.extend(options.on_ready.iter().cloned());
    hooks
}

fn wake_config_post_wake_hooks() -> Vec<String> {
    let config = merged_config_value();
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
    if options.attach {
        let _ = writeln!(out, "\x1b[32m+\x1b[0m created session '{}' (main: {})", resolved.session, resolved.window);
        return Ok(true);
    }
    tmux.wake_send_text(&resolved.target, &resolved.command)?;
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
    if !options.new_window && windows.iter().any(|window| window.name == resolved.window) {
        let _ = writeln!(out, "\x1b[32m⚡\x1b[0m '{}' running in {}", resolved.window, resolved.session);
        return Ok(false);
    }
    tmux.wake_new_window(&resolved.session, &resolved.window, &resolved.repo_path)?;
    if options.attach {
        let _ = writeln!(out, "\x1b[32m✅\x1b[0m woke '{}' in {} → {}", resolved.window, resolved.session, resolved.repo_path.display());
        return Ok(true);
    }
    tmux.wake_send_text(&resolved.target, &resolved.command)?;
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

#[cfg(test)]
mod wake_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct WakeMockTmux {
        sessions: Vec<TmuxSession>,
        actions: Vec<String>,
        fail_select: bool,
        detached_delay_ms: u64,
        detached_finished: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl WakeTmuxNative for WakeMockTmux {
        fn wake_list(&mut self) -> Vec<TmuxSession> { self.sessions.clone() }
        fn wake_has_session(&mut self, name: &str) -> bool { self.sessions.iter().any(|session| session.name == name) }
        fn wake_new_session(&mut self, name: &str, window: &str, cwd: &std::path::Path) -> Result<(), String> {
            self.actions.push(format!("new-session {name} {window} {}", cwd.display()));
            self.sessions.push(TmuxSession { name: name.to_owned(), windows: vec![maw_tmux::TmuxWindow { index: 0, name: window.to_owned(), active: true, cwd: Some(cwd.display().to_string()) }] });
            Ok(())
        }
        fn wake_new_window(&mut self, session: &str, window: &str, cwd: &std::path::Path) -> Result<(), String> {
            self.actions.push(format!("new-window {session} {window} {}", cwd.display()));
            if let Some(existing) = self.sessions.iter_mut().find(|item| item.name == session) {
                existing.windows.push(maw_tmux::TmuxWindow {
                    index: u32::try_from(existing.windows.len()).unwrap_or(u32::MAX),
                    name: window.to_owned(),
                    active: false,
                    cwd: Some(cwd.display().to_string()),
                });
            }
            Ok(())
        }
        fn wake_send_text(&mut self, target: &str, text: &str) -> Result<(), String> {
            self.actions.push(format!("send {target} {text}"));
            Ok(())
        }
        fn wake_send_text_detached(&mut self, target: String, text: String) -> Result<Option<std::thread::JoinHandle<()>>, String> {
            self.actions.push(format!("send-detached {target} {text}"));
            let delay_ms = self.detached_delay_ms;
            let finished = std::sync::Arc::clone(&self.detached_finished);
            Ok(Some(std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                finished.store(true, std::sync::atomic::Ordering::SeqCst);
            })))
        }
        fn wake_select_window(&mut self, target: &str) -> Result<(), String> {
            self.actions.push(format!("select {target}"));
            if self.fail_select { Err("mock attach failed".to_owned()) } else { Ok(()) }
        }
    }

    fn wake_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn wake_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-wake-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    struct CwdRestore {
        previous: std::path::PathBuf,
    }

    impl CwdRestore {
        fn enter(path: &std::path::Path) -> Self {
            let previous = std::env::current_dir().expect("current dir before test");
            std::env::set_current_dir(path).expect("set test cwd");
            Self { previous }
        }
    }

    impl Drop for CwdRestore {
        fn drop(&mut self) {
            std::env::set_current_dir(&self.previous).expect("restore test cwd");
        }
    }

    fn wake_with_fixture<F>(test: F)
    where
        F: FnOnce(&std::path::Path),
    {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = EnvVarRestore::capture("HOME");
        let _xdg = EnvVarRestore::capture("XDG_CONFIG_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _maw_home = EnvVarRestore::capture("MAW_HOME");
        let _state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _tmux = EnvVarRestore::capture("TMUX");
        let root = wake_temp_root("fixture");
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle")).expect("repo");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_STATE_DIR", root.join("state"));
        std::env::set_var("GHQ_ROOT", root.join("ghq/github.com"));
        std::env::remove_var("TMUX");
        test(&root);
    }

    #[test]
    fn wake_parse_flags_and_guard_option_injection() {
        let options = wake_parse_args(&wake_strings(&["neo", "--task", "issue-134", "--dry-run", "--no-attach", "--layout=legacy", "--fresh"])).expect("parse");
        assert_eq!(options.target, "neo");
        assert_eq!(options.task.as_deref(), Some("issue-134"));
        assert!(options.dry_run && options.no_attach && options.fresh);
        assert!(wake_parse_args(&wake_strings(&["neo", "-a"])).expect("parse -a").attach);
        assert!(wake_parse_args(&wake_strings(&["neo", "--yes"])).expect("parse yes").yes);
        assert!(wake_parse_args(&wake_strings(&["--", "neo"])).expect_err("separator guard").contains("unknown argument"));
        assert!(wake_parse_args(&wake_strings(&["neo", "--task", "-bad"])).expect_err("value guard").contains("must not start"));
    }

    #[test]
    fn wake_post_wake_hooks_write_marker_env() {
        wake_with_fixture(|root| {
            let session = wake_session_name("neo", &[]);
            let expected = format!("neo|{session}|neo");
            let cli_marker = root.join("cli-ready.txt");
            let cli_hook = format!(
                "printf '%s|%s|%s' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" > {}",
                wake_shell_quote(&cli_marker.display().to_string())
            );
            let mut tmux = WakeMockTmux::default();
            let (code, _stdout) = wake_run(
                &wake_strings(&["neo", "--no-attach", "--on-ready", "false", "--on-ready", &cli_hook]),
                &mut tmux,
            )
            .expect("wake with cli hooks");
            assert_eq!(code, 0);
            assert_eq!(std::fs::read_to_string(&cli_marker).expect("cli marker"), expected);

            let config_marker = root.join("config-ready.txt");
            let config_hook = format!(
                "printf '%s|%s|%s' \"$MAW_ORACLE\" \"$MAW_SESSION\" \"$MAW_WINDOW\" > {}",
                wake_shell_quote(&config_marker.display().to_string())
            );
            std::fs::write(
                root.join("config/maw.config.50.json"),
                serde_json::to_string(&serde_json::json!({"hooks":{"postWake":[config_hook]}})).expect("json"),
            )
            .expect("write config hook");
            let mut tmux = WakeMockTmux { sessions: tmux.sessions, ..WakeMockTmux::default() };
            let (code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("wake with config hook");
            assert_eq!(code, 0);
            assert_eq!(std::fs::read_to_string(&config_marker).expect("config marker"), expected);
        });
    }

    #[test]
    fn wake_short_e_flag_and_config_commands_engine_resolution() {
        // short `-e` is accepted as an alias of `--engine`
        let options = wake_parse_args(&wake_strings(&["neo", "-e", "omx-1"])).expect("parse -e");
        assert_eq!(options.engine.as_deref(), Some("omx-1"));

        // custom engines resolve to their full command from merged config `commands`;
        // real binaries not in the map fall through to the literal name.
        wake_with_fixture(|_| {
            let dir = active_config_dir();
            std::fs::create_dir_all(&dir).expect("config dir");
            std::fs::write(
                dir.join("maw.config.50.json"),
                r#"{"commands":{"omx-1":"bun codex-setup.ts 1 && CODEX_HOME=$PWD/.codex omx --direct --madmax","default":"claude"}}"#,
            )
            .expect("write config");
            assert!(!dir.join("maw.config.json").exists());
            assert_eq!(
                wake_resolve_engine_command("omx-1"),
                "bun codex-setup.ts 1 && CODEX_HOME=$PWD/.codex omx --direct --madmax"
            );
            assert_eq!(wake_resolve_engine_command("codex"), "codex");
        });
    }

    #[test]
    fn wake_fresh_default_uses_config_default_but_explicit_and_resume_keep_codex() {
        wake_with_fixture(|_| {
            let dir = active_config_dir();
            std::fs::create_dir_all(&dir).expect("config dir");
            std::fs::write(dir.join("maw.config.50.json"), r#"{"commands":{"default":"claude"}}"#)
                .expect("write config");

            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("fresh");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.contains("{ claude;"), "{send}");
            assert!(!send.contains("{ codex;"), "{send}");

            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "-e", "codex"]), &mut tmux).expect("explicit");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.contains("{ codex;"), "{send}");

            let mut tmux = WakeMockTmux::default();
            let (_code, _stdout) = wake_run(&wake_strings(&["neo", "--no-attach", "--resume"]), &mut tmux).expect("resume");
            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.contains("{ codex resume;"), "{send}");
        });
    }

    #[test]
    fn wake_repo_path_flag_overrides_repo_resolution() {
        // `team up` passes `--repo-path <worktree>`; wake must accept it and use it
        // directly, bypassing ghq/fleet lookup.
        let options = wake_parse_args(&wake_strings(&[
            "coder-1", "--repo-path", "/tmp/wt/coder-1", "-e", "codex", "--no-attach",
        ]))
        .expect("parse --repo-path");
        assert_eq!(options.repo_path.as_deref(), Some(std::path::Path::new("/tmp/wt/coder-1")));
        assert_eq!(
            wake_repo_path(&options, "coder-1", &fleet_load_entries()).expect("resolve").path,
            std::path::PathBuf::from("/tmp/wt/coder-1")
        );
    }

    #[test]
    fn wake_reuses_workon_github_url_resolver_without_double_prefix_or_peer_route() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/Soul-Brews-Studio/maw-fleetpad");
            std::fs::create_dir_all(&repo).expect("repo");
            let args = wake_strings(&[
                "https://github.com/Soul-Brews-Studio/maw-fleetpad",
                "--dry-run",
                "--no-attach",
            ]);
            let options = wake_parse_args(&args).expect("parse");

            assert!(!wake_should_use_peer_target(&options));
            assert_eq!(wake_oracle(&options).expect("oracle"), "maw-fleetpad");
            assert_eq!(wake_repo_path(&options, "maw-fleetpad", &fleet_load_entries()).expect("resolve").path, repo);

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&args, &mut tmux).expect("run");
            assert_eq!(code, 0);
            assert!(stdout.contains("Soul-Brews-Studio/maw-fleetpad"), "{stdout}");
            assert!(!stdout.contains("github.com/github.com"), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_reuses_workon_github_host_slug_resolver_without_double_prefix() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/Soul-Brews-Studio/maw-fleetpad");
            std::fs::create_dir_all(&repo).expect("repo");
            let options = wake_parse_args(&wake_strings(&[
                "github.com/Soul-Brews-Studio/maw-fleetpad",
                "--dry-run",
            ]))
            .expect("parse");

            assert_eq!(wake_repo_path(&options, "maw-fleetpad", &fleet_load_entries()).expect("resolve").path, repo);
        });
    }

    #[test]
    fn wake_fuzzy_resolves_middle_repo_segment_and_reports_match() {
        wake_with_fixture(|root| {
            let repo = root.join("ghq/github.com/laris-co/DustBoy-Phd-Oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let mut tmux = WakeMockTmux::default();

            let (code, stdout) = wake_run(&wake_strings(&["phd-oracle", "--dry-run"]), &mut tmux)
                .expect("fuzzy wake");

            assert_eq!(code, 0);
            assert!(stdout.contains("fuzzy match: DustBoy-Phd-Oracle"), "{stdout}");
            assert!(stdout.contains(&repo.display().to_string()), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_relative_repo_path_is_absolute_before_send() {
        wake_with_fixture(|root| {
            let cwd = root.join("workspace");
            let repo = cwd.join("agents/1-codex-1");
            std::fs::create_dir_all(&repo).expect("worktree");
            let _cwd = CwdRestore::enter(&cwd);

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(
                &wake_strings(&[
                    "coder-1",
                    "--repo-path",
                    "agents/1-codex-1",
                    "-e",
                    "codex",
                    "--no-attach",
                ]),
                &mut tmux,
            )
            .expect("wake");
            assert_eq!(code, 0);
            assert!(stdout.contains("created session"));

            let expected = repo.canonicalize().expect("canonical worktree");
            let new_session = tmux.actions.iter().find(|action| action.starts_with("new-session")).expect("new-session action");
            assert!(new_session.contains(&expected.display().to_string()), "{new_session}");

            let send = tmux.actions.iter().find(|action| action.starts_with("send ")).expect("send action");
            assert!(send.contains(&format!("cd {}", expected.display())), "{send}");
            assert!(!send.contains("cd agents/1-codex-1"), "{send}");
            assert!(send.contains("maw wake: failed to cd"), "{send}");
            assert!(send.contains("engine not started"), "{send}");
            assert!(send.contains("maw wake: engine exited with status"), "{send}");
        });
    }

    #[test]
    fn wake_reuses_registry_session_name_after_reboot() {
        wake_with_fixture(|root| {
            let session = "99-mother";
            let repo = root.join("ghq/github.com/laris-co/mother-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join(format!("{session}.json")),
                r#"{"name":"99-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with(&format!("new-session {session}"))), "{stdout}");
            assert!(stdout.contains(&format!("created session '{session}'")));
        });
    }

    #[test]
    fn wake_full_numeric_registry_name_resolves_via_typed_resolver() {
        wake_with_fixture(|root| {
            let session = "41-arra-oracle-v3";
            let repo = root.join("ghq/github.com/laris-co/arra-oracle-v3");
            std::fs::create_dir_all(&repo).expect("repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"41-arra-oracle-v3","windows":[{"name":"arra-oracle-v3","repo":"github.com/laris-co/arra-oracle-v3"}]}"#,
            )
            .expect("write registry");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&[session, "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains(&format!("created session '{session}'")), "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with(&format!("new-session {session} arra-oracle-v3"))), "{tmux:?}");
            assert!(tmux.actions.iter().any(|action| action.contains(&repo.display().to_string())), "{tmux:?}");
        });
    }

    #[test]
    fn wake_exact_session_name_with_multiple_windows_is_not_ambiguous() {
        wake_with_fixture(|root| {
            let session = "41-arra-oracle-v3";
            let main_repo = root.join("ghq/github.com/laris-co/arra-oracle-v3");
            let task_repo = root.join("ghq/github.com/laris-co/arra-oracle-v3-task");
            std::fs::create_dir_all(&main_repo).expect("main repo");
            std::fs::create_dir_all(&task_repo).expect("task repo");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"41-arra-oracle-v3","windows":[{"name":"arra-oracle-v3","repo":"github.com/laris-co/arra-oracle-v3"},{"name":"arra-oracle-v3-task","repo":"github.com/laris-co/arra-oracle-v3-task"}]}"#,
            )
            .expect("write registry");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&[session, "--dry-run"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains("found"), "{stdout}");
            assert!(stdout.contains("arra-oracle-v3"), "{stdout}");
            assert!(stdout.contains(&main_repo.display().to_string()), "{stdout}");
            assert!(stdout.contains(&format!("would wake window 'arra-oracle-v3' in session '{session}'")), "{stdout}");
            assert!(!stdout.contains("ambiguous registry target"), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_unknown_name_reports_not_found_without_tmux_mutation() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&["does-not-exist", "--no-attach"]), &mut tmux).expect_err("not found");
            assert!(err.contains("wake: repo not found for does-not-exist"), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_typo_near_miss_reports_did_you_mean_and_next_steps() {
        wake_with_fixture(|root| {
            std::fs::create_dir_all(root.join("ghq/github.com/acme/mascot-oracle")).expect("repo");
            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&["mascott", "--no-attach"]), &mut tmux).expect_err("not found");
            assert!(err.contains("wake: repo not found for mascott"), "{err}");
            assert!(err.contains("Did you mean"), "{err}");
            assert!(err.contains("mascot"), "{err}");
            assert!(err.contains("maw oracle scan"), "{err}");
            assert!(err.contains("maw ls -a"), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_exact_fleet_squad_uses_universal_non_tty_picker() {
        wake_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/01-3e.json"),
                r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"},{"handle":"drift"}]}"#,
            )
            .expect("squad registry");

            let output = run_wake_command(&wake_strings(&["3e"]));

            assert_eq!(output.code, 1, "{}{}", output.stdout, output.stderr);
            assert!(output.stdout.contains("fleet squad 3e (2 members)"), "{}", output.stdout);
            assert!(output.stdout.contains("maw fleet wake 3e"), "{}", output.stdout);
            assert!(output.stderr.is_empty(), "{}", output.stderr);
        });
    }

    #[test]
    fn wake_fleet_squad_yes_and_dry_run_execute_in_process_bridge() {
        wake_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/01-3e.json"),
                r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"}]}"#,
            )
            .expect("squad registry");
            let mut tmux = WakeMockTmux::default();
            let mut calls = Vec::<Vec<String>>::new();
            let mut fleet_wake = |args: &[String]| {
                calls.push(args.to_vec());
                CliOutput { code: 0, stdout: "fleet bridge\n".to_owned(), stderr: String::new() }
            };

            let yes = run_wake_command_with(&wake_strings(&["3e", "--yes"]), &mut tmux, &mut fleet_wake);
            let dry_run = run_wake_command_with(&wake_strings(&["3e", "--dry-run"]), &mut tmux, &mut fleet_wake);

            assert_eq!(yes.code, 0, "{}{}", yes.stdout, yes.stderr);
            assert_eq!(dry_run.code, 0, "{}{}", dry_run.stdout, dry_run.stderr);
            assert_eq!(calls, vec![wake_strings(&["wake", "3e"]), wake_strings(&["wake", "3e", "--dry-run"])]);
            assert!(tmux.actions.is_empty(), "{:?}", tmux.actions);
        });
    }

    #[test]
    fn wake_near_fleet_squad_uses_same_picker_without_auto_action() {
        wake_with_fixture(|root| {
            std::fs::write(
                root.join("config/fleet/01-3e.json"),
                r#"{"name":"01-3e","squadName":"3e","windows":[],"members":[{"handle":"alpha"}]}"#,
            )
            .expect("squad registry");
            let mut tmux = WakeMockTmux::default();
            let mut called = false;
            let mut fleet_wake = |_: &[String]| {
                called = true;
                CliOutput { code: 0, stdout: String::new(), stderr: String::new() }
            };

            let output = run_wake_command_with(&wake_strings(&["3f"]), &mut tmux, &mut fleet_wake);

            assert_eq!(output.code, 1, "{}{}", output.stdout, output.stderr);
            assert!(output.stdout.contains("fleet squad 3e"), "{}", output.stdout);
            assert!(output.stdout.contains("maw fleet wake 3e"), "{}", output.stdout);
            assert!(!called);
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_revived_session_reregisters_into_its_own_registry_entry() {
        // #312 revive + #299 upsert guard interaction: the entry that named
        // the revived session lives in the config fleet dir, not the default
        // ~/.maw/fleet write dir. Re-registration after the wake must update
        // that entry in place instead of minting a duplicate file.
        wake_with_fixture(|root| {
            let session = "99-mother";
            let repo = root.join("ghq/github.com/laris-co/mother-oracle");
            std::fs::create_dir_all(&repo).expect("repo");
            let entry = root.join("config/fleet").join(format!("{session}.json"));
            std::fs::write(
                &entry,
                r#"{"name":"99-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains(&format!("created session '{session}'")));
            assert!(!root.join("home/.maw/fleet").join(format!("{session}.json")).exists(), "duplicate entry minted: {stdout}");
            let value = serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&entry).expect("entry")).expect("json");
            assert_eq!(value["name"], "99-mother");
            assert_eq!(value["created_by"], "maw wake");
        });
    }

    #[test]
    fn wake_session_name_avoids_slot_collision_with_live_session() {
        wake_with_fixture(|root| {
            let oracle = "turso";
            let _ = std::fs::create_dir_all(root.join("ghq/github.com/acme/turso-oracle"));
            let occupied_slot = wake_slot(oracle);
            let mut tmux = WakeMockTmux {
                sessions: vec![TmuxSession {
                    name: format!("{occupied_slot:02}-esp32"),
                    windows: vec![maw_tmux::TmuxWindow { index: 0, name: "esp32".to_owned(), active: true, cwd: None }],
                }],
                ..WakeMockTmux::default()
            };
            let (code, stdout) = wake_run(&wake_strings(&[oracle, "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert!(tmux.actions.iter().any(|action| action.starts_with("new-session")));
            assert!(
                !tmux.actions.iter().any(|action| action.starts_with(&format!("new-session {occupied_slot:02}-{oracle}"))),
                "{stdout}"
            );
        });
    }

    #[test]
    fn wake_repo_not_found_reports_registry_gap() {
        wake_with_fixture(|root| {
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join("88-mother.json"),
                r#"{"name":"88-mother","windows":[{"name":"mother","repo":"github.com/laris-co/mother-oracle"}]}"#,
            )
            .expect("write");

            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&["mother", "--no-attach"]), &mut tmux).expect_err("not found");
            assert!(err.contains("registry entry for 88-mother exists"), "{err}");
            assert!(err.contains("not cloned under"), "{err}");
            assert!(err.contains("probed"), "{err}");
            assert!(err.contains(&wake_ghq_root().display().to_string()), "{err}");
            assert!(err.contains(&root.join("ghq/github.com/laris-co/mother-oracle").display().to_string()), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_stale_registry_repo_falls_back_to_oracles_local_path() {
        wake_with_fixture(|root| {
            let canonical = root.join("repos/token-oracle");
            std::fs::create_dir_all(&canonical).expect("canonical repo");
            let canonical = canonical.canonicalize().expect("canonical path");
            let fleet = root.join("home/.maw/fleet");
            std::fs::create_dir_all(&fleet).expect("fleet");
            std::fs::write(
                fleet.join("59-token.json"),
                r#"{"name":"59-token","windows":[{"name":"token","repo":"github.com/Soul-Brews-Studio/token-oracle-oracle"}]}"#,
            )
            .expect("stale registry");
            let cache = serde_json::json!({
                "schema": 1,
                "oracles": [{
                    "org": "laris-co",
                    "repo": "token-oracle",
                    "name": "token",
                    "local_path": canonical,
                    "has_psi": true,
                    "has_fleet_config": true
                }]
            });
            std::fs::write(root.join("home/.maw/oracles.json"), cache.to_string()).expect("oracles cache");

            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["token", "--dry-run"]), &mut tmux).expect("fallback");

            assert_eq!(code, 0, "{stdout}");
            assert!(stdout.contains(&format!("registry repo stale, using oracles.json: {}", canonical.display())), "{stdout}");
            assert_eq!(stdout.matches(&canonical.display().to_string()).count(), 2, "{stdout}");
            assert!(!stdout.contains("token-oracle-oracle"), "{stdout}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_full_registry_name_reports_missing_clone_path() {
        wake_with_fixture(|root| {
            let session = "41-arra-oracle-v3";
            let probed = root.join("ghq/github.com/laris-co/arra-oracle-v3");
            std::fs::write(
                root.join("config/fleet").join(format!("{session}.json")),
                r#"{"name":"41-arra-oracle-v3","windows":[{"name":"arra-oracle-v3","repo":"github.com/laris-co/arra-oracle-v3"}]}"#,
            )
            .expect("write registry");

            let mut tmux = WakeMockTmux::default();
            let err = wake_run(&wake_strings(&[session, "--no-attach"]), &mut tmux).expect_err("missing clone");
            assert!(err.contains(&format!("registry entry for {session} exists")), "{err}");
            assert!(err.contains(&format!("probed {}", probed.display())), "{err}");
            assert!(!err.contains("repo not found for"), "{err}");
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_dry_run_is_hermetic_and_matches_golden() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--dry-run", "--task", "issue-134"]), &mut tmux).expect("run");
            assert_eq!(code, 0);
            assert!(stdout.contains("dry-run — no tmux sessions/windows will be changed"));
            assert!(stdout.contains("would wake window 'neo-issue-134'"));
            assert!(tmux.actions.is_empty());
        });
    }

    #[test]
    fn wake_apply_uses_seeded_repo_and_mock_tmux_only() {
        wake_with_fixture(|root| {
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0);
            assert!(stdout.contains("created session"));
            assert!(stdout.contains("attach: maw a"));
            assert!(tmux.actions.iter().any(|action| action.starts_with("new-session")));
            assert!(tmux.actions.iter().any(|action| action.contains(&root.join("ghq/github.com/acme/neo-oracle").display().to_string())));
            assert!(!tmux.actions.iter().any(|action| action.starts_with("select")));
        });
    }

    #[test]
    fn wake_attach_selects_before_post_attach_work_and_audits_phases() {
        wake_with_fixture(|root| {
            let mut tmux = WakeMockTmux::default();
            let (code, stdout) = wake_run(&wake_strings(&["neo", "--attach"]), &mut tmux).expect("run");
            assert_eq!(code, 0, "{stdout}");
            assert_eq!(tmux.actions[0].split_whitespace().next(), Some("new-session"));
            assert_eq!(tmux.actions[1].split_whitespace().next(), Some("send-detached"));
            assert_eq!(tmux.actions[2].split_whitespace().next(), Some("select"));
            let audit = std::fs::read_to_string(root.join("state/audit.jsonl")).expect("audit");
            assert!(audit.contains(r#""event":"wake.phase""#), "{audit}");
            assert!(audit.contains(r#""phase":"first-window""#), "{audit}");
            let first = audit.find(r#""phase":"first-window""#).expect("first-window phase");
            let attach = audit.find(r#""phase":"attach""#).expect("attach phase");
            let fleet = audit.find(r#""phase":"fleet-upsert""#).expect("fleet phase");
            assert!(first < attach && attach < fleet, "{audit}");
            assert!(audit.contains(r#""phase":"fleet-upsert""#), "{audit}");
        });
    }

    #[test]
    fn wake_fast_attach_failure_waits_for_detached_engine_send() {
        wake_with_fixture(|_| {
            let mut tmux = WakeMockTmux {
                fail_select: true,
                detached_delay_ms: 50,
                ..WakeMockTmux::default()
            };

            let error = wake_run(&wake_strings(&["neo", "--attach"]), &mut tmux).expect_err("attach failure");

            assert!(error.contains("mock attach failed"), "{error}");
            assert!(tmux.detached_finished.load(std::sync::atomic::Ordering::SeqCst));
        });
    }

    #[test]
    fn wake_auto_registers_fleet_json_and_merges_new_windows() {
        wake_with_fixture(|root| {
            let _now = EnvVarRestore::capture("MAW_RS_FLEET_REGISTRY_NOW");
            std::env::set_var("MAW_RS_FLEET_REGISTRY_NOW", "2026-07-03T02:03:04.000Z");
            let mut tmux = WakeMockTmux::default();

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--no-attach"]), &mut tmux).expect("first wake");
            assert_eq!(code, 0, "{stdout}");
            let session = tmux.sessions.first().expect("session").name.clone();
            let path = root.join("home/.maw/fleet").join(format!("{session}.json"));
            let first: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).expect("registry")).expect("json");
            assert_eq!(first["name"], session);
            assert_eq!(first["created_at"], "2026-07-03T02:03:04.000Z");
            assert_eq!(first["created_by"], "maw wake");
            assert_eq!(first["auto_registered"], true);
            assert_eq!(first["windows"].as_array().expect("windows").len(), 1);
            assert_eq!(first["windows"][0]["name"], "neo");
            assert_eq!(first["windows"][0]["repo"], "acme/neo-oracle");
            assert_eq!(first["windows"][0]["kind"], "project");

            let (code, stdout) = wake_run(&wake_strings(&["neo", "--task", "issue-90", "--no-attach"]), &mut tmux).expect("task wake");
            assert_eq!(code, 0, "{stdout}");
            let updated: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("updated registry")).expect("json");
            let windows = updated["windows"].as_array().expect("windows");
            assert_eq!(windows.len(), 2);
            assert!(windows.iter().any(|window| window["name"] == "neo"));
            assert!(windows.iter().any(|window| window["name"] == "neo-issue-90"));
            assert!(windows.iter().all(|window| window["kind"] == "project"));
            assert_eq!(updated["created_at"], "2026-07-03T02:03:04.000Z");
        });
    }

    #[test]
    fn wake_list_reads_mock_sessions_without_real_tmux() {
        let mut tmux = WakeMockTmux { sessions: vec![TmuxSession { name: "12-neo".to_owned(), windows: vec![maw_tmux::TmuxWindow { index: 0, name: "neo".to_owned(), active: true, cwd: None }] }], ..WakeMockTmux::default() };
        let (code, stdout) = wake_run(&wake_strings(&["neo", "--list"]), &mut tmux).expect("run");
        assert_eq!(code, 0);
        assert!(stdout.contains("12-neo (1 windows)"));
        assert!(tmux.actions.is_empty());
    }
}
