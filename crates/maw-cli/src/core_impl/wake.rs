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
}

type WakeEqualsSetter = fn(&mut WakeOptionsNative, &str) -> Result<(), String>;

#[derive(Debug, Clone, PartialEq, Eq)]
struct WakeResolvedNative {
    oracle: String,
    session: String,
    window: String,
    repo_path: std::path::PathBuf,
    command: String,
    target: String,
}

trait WakeTmuxNative {
    fn wake_list(&mut self) -> Vec<TmuxSession>;
    fn wake_has_session(&mut self, name: &str) -> bool;
    fn wake_new_session(&mut self, name: &str, window: &str, cwd: &std::path::Path) -> Result<(), String>;
    fn wake_new_window(&mut self, session: &str, window: &str, cwd: &std::path::Path) -> Result<(), String>;
    fn wake_send_text(&mut self, target: &str, text: &str) -> Result<(), String>;
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

    fn wake_select_window(&mut self, target: &str) -> Result<(), String> {
        wake_validate_tmux_target(target)?;
        TmuxClient::local().select_window(target);
        Ok(())
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
    match wake_run(argv, &mut WakeNativeTmux) {
        Ok((code, stdout)) => CliOutput { code, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn wake_run(argv: &[String], tmux: &mut impl WakeTmuxNative) -> Result<(i32, String), String> {
    let options = wake_parse_args(argv)?;
    let sessions = tmux.wake_list();
    if options.list { return Ok((0, wake_render_list(&options, &sessions))); }
    if options.all { return Ok((0, wake_render_all_plan(&options, &sessions))); }
    let resolved = wake_resolve(&options, &sessions)?;
    if options.dry_run { return Ok((0, wake_render_dry_run(&options, &resolved))); }
    let mut out = String::new();
    wake_apply(&options, &resolved, tmux, &mut out)?;
    Ok((0, out))
}

fn wake_should_use_peer_target(options: &WakeOptionsNative) -> bool {
    if options.dry_run || options.list || options.all || options.repo.is_some() || options.incubate.is_some() { return false; }
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
        name: None, repo_path: None, all: false, all_local: false, attach: true, dry_run: false, fresh: false,
        from_snapshot: false, kill: false, list: false, main: false, new_window: false, no_attach: false,
        pick: false, resume: false, solo: false, split: false, bud: false, channels: false, wait: false,
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
    ]
}

fn wake_parse_bool_arg(arg: &str, options: &mut WakeOptionsNative) -> Result<bool, String> {
    match arg {
        "--all" => options.all = true,
        "all" => { options.all = true; if options.target.is_empty() { "all".clone_into(&mut options.target); } }
        "--all-local" => options.all_local = true,
        "--attach" => { options.attach = true; options.no_attach = false; }
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
    "usage: maw wake <target|all> [--task <slug>|--wt <slug>] [--repo <org/repo>] [--prompt <text>] [--all --all-local --attach --no-attach --dry-run --fresh --from-snapshot --kill --layout <nested|legacy> --list --main --new --parent <session> --peer <node> --pick --resume --snapshot <id> --solo --split]".to_owned()
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
    let oracle = wake_oracle(options)?;
    let repo_path = wake_repo_path(options, &oracle)?;
    let session = options.parent.clone().or_else(|| wake_detect_session(&oracle, sessions)).unwrap_or_else(|| wake_session_name(&oracle));
    let window = wake_window_name(options, &oracle);
    let target = format!("{session}:{window}");
    let command = wake_command(&window, &repo_path, options);
    Ok(WakeResolvedNative { oracle, session, window, repo_path, command, target })
}

fn wake_oracle(options: &WakeOptionsNative) -> Result<String, String> {
    let raw = options.name.as_deref().or_else(|| options.target.split('/').next_back()).unwrap_or(&options.target);
    let oracle = raw.strip_suffix("-oracle").unwrap_or(raw).trim();
    wake_validate_slug(oracle, "oracle")?;
    Ok(oracle.to_owned())
}

fn wake_repo_path(options: &WakeOptionsNative, oracle: &str) -> Result<std::path::PathBuf, String> {
    // `--repo-path <dir>` is an explicit filesystem override (used by `team up`
    // to point at the bound worktree) — it bypasses ghq/fleet resolution.
    if let Some(repo_path) = &options.repo_path {
        return wake_normalize_repo_path(repo_path);
    }
    if let Some(repo) = &options.repo { return Ok(wake_ghq_root().join("github.com").join(repo)); }
    if let Some(repo) = &options.incubate { return Ok(wake_ghq_root().join("github.com").join(repo)); }
    if options.target.contains('/') { return Ok(wake_ghq_root().join("github.com").join(&options.target)); }
    wake_find_repo(oracle).ok_or_else(|| format!("wake: repo not found for {oracle}"))
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

fn wake_find_repo(oracle: &str) -> Option<std::path::PathBuf> {
    let root = wake_ghq_root().join("github.com");
    let Ok(orgs) = std::fs::read_dir(root) else { return None; };
    let mut matches = Vec::new();
    for org in orgs.flatten() { wake_collect_repo_match(&org.path(), oracle, &mut matches); }
    matches.sort();
    matches.into_iter().next()
}

fn wake_collect_repo_match(org_path: &std::path::Path, oracle: &str, matches: &mut Vec<std::path::PathBuf>) {
    let Ok(repos) = std::fs::read_dir(org_path) else { return; };
    for repo in repos.flatten() {
        let path = repo.path();
        let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { continue; };
        if path.is_dir() && wake_repo_name_matches(name, oracle) { matches.push(path); }
    }
}

fn wake_repo_name_matches(name: &str, oracle: &str) -> bool {
    name == oracle || name == format!("{oracle}-oracle") || name.trim_end_matches("-oracle") == oracle
}

fn wake_detect_session(oracle: &str, sessions: &[TmuxSession]) -> Option<String> {
    sessions.iter().find(|session| wake_session_matches(&session.name, oracle)).map(|session| session.name.clone())
}

fn wake_session_matches(name: &str, oracle: &str) -> bool {
    name == oracle || name.ends_with(&format!("-{oracle}")) || name.ends_with(&format!("-{oracle}-oracle"))
}

fn wake_session_name(oracle: &str) -> String { format!("{:02}-{oracle}", wake_slot(oracle)) }

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
    merged_config_value()
        .get("commands")
        .cloned()
        .and_then(|commands| {
            commands
                .get(engine)
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .unwrap_or_else(|| engine.to_owned())
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
    let _ = writeln!(out, "\x1b[36m→\x1b[0m found \x1b[1m{}\x1b[0m ({})", resolved.oracle, resolved.repo_path.display());
    out.push_str("\x1b[90mdry-run — no tmux sessions/windows will be changed\x1b[0m\n");
    let _ = writeln!(out, "\x1b[32m+\x1b[0m would wake window '{}' in session '{}'", resolved.window, resolved.session);
    if options.task.is_some() || options.wt.is_some() {
        let _ = writeln!(out, "\x1b[33m⚡\x1b[0m would wake worktree/task: {}", options.wt.as_deref().or(options.task.as_deref()).unwrap_or_default());
    }
    out
}

fn wake_apply(
    options: &WakeOptionsNative,
    resolved: &WakeResolvedNative,
    tmux: &mut impl WakeTmuxNative,
    out: &mut String,
) -> Result<(), String> {
    if !resolved.repo_path.is_dir() { return Err(format!("wake: repo path missing: {}", resolved.repo_path.display())); }
    let session_exists = tmux.wake_has_session(&resolved.session);
    if session_exists { wake_create_or_reuse_window(options, resolved, tmux, out)?; } else { wake_create_session(resolved, tmux, out)?; }
    wake_register_fleet_session(resolved, tmux)?;
    if options.attach { tmux.wake_select_window(&resolved.target)?; }
    Ok(())
}

fn wake_create_session(resolved: &WakeResolvedNative, tmux: &mut impl WakeTmuxNative, out: &mut String) -> Result<(), String> {
    tmux.wake_new_session(&resolved.session, &resolved.window, &resolved.repo_path)?;
    tmux.wake_send_text(&resolved.target, &resolved.command)?;
    let _ = writeln!(out, "\x1b[32m+\x1b[0m created session '{}' (main: {})", resolved.session, resolved.window);
    Ok(())
}

fn wake_create_or_reuse_window(
    options: &WakeOptionsNative,
    resolved: &WakeResolvedNative,
    tmux: &mut impl WakeTmuxNative,
    out: &mut String,
) -> Result<(), String> {
    let windows = tmux.wake_list().into_iter().find(|session| session.name == resolved.session).map(|session| session.windows).unwrap_or_default();
    if !options.new_window && windows.iter().any(|window| window.name == resolved.window) {
        let _ = writeln!(out, "\x1b[32m⚡\x1b[0m '{}' running in {}", resolved.window, resolved.session);
        return Ok(());
    }
    tmux.wake_new_window(&resolved.session, &resolved.window, &resolved.repo_path)?;
    tmux.wake_send_text(&resolved.target, &resolved.command)?;
    let _ = writeln!(out, "\x1b[32m✅\x1b[0m woke '{}' in {} → {}", resolved.window, resolved.session, resolved.repo_path.display());
    Ok(())
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

    #[derive(Default)]
    struct WakeMockTmux {
        sessions: Vec<TmuxSession>,
        actions: Vec<String>,
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
        fn wake_select_window(&mut self, target: &str) -> Result<(), String> {
            self.actions.push(format!("select {target}"));
            Ok(())
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
        let _ghq = EnvVarRestore::capture("GHQ_ROOT");
        let _tmux = EnvVarRestore::capture("TMUX");
        let root = wake_temp_root("fixture");
        std::fs::create_dir_all(root.join("ghq/github.com/acme/neo-oracle")).expect("repo");
        std::fs::create_dir_all(root.join("config/fleet")).expect("fleet");
        std::env::set_var("HOME", root.join("home"));
        std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
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
        assert!(wake_parse_args(&wake_strings(&["--", "neo"])).expect_err("separator guard").contains("unknown argument"));
        assert!(wake_parse_args(&wake_strings(&["neo", "--task", "-bad"])).expect_err("value guard").contains("must not start"));
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
            wake_repo_path(&options, "coder-1").expect("resolve"),
            std::path::PathBuf::from("/tmp/wt/coder-1")
        );
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
            assert!(tmux.actions.iter().any(|action| action.starts_with("new-session")));
            assert!(tmux.actions.iter().any(|action| action.contains(&root.join("ghq/github.com/acme/neo-oracle").display().to_string())));
            assert!(!tmux.actions.iter().any(|action| action.starts_with("select")));
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
            assert_eq!(first["windows"][0]["repo"], "github.com/acme/neo-oracle");
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
        let mut tmux = WakeMockTmux { sessions: vec![TmuxSession { name: "12-neo".to_owned(), windows: vec![maw_tmux::TmuxWindow { index: 0, name: "neo".to_owned(), active: true, cwd: None }] }], actions: Vec::new() };
        let (code, stdout) = wake_run(&wake_strings(&["neo", "--list"]), &mut tmux).expect("run");
        assert_eq!(code, 0);
        assert!(stdout.contains("12-neo (1 windows)"));
        assert!(tmux.actions.is_empty());
    }
}
