const DISPATCH_79: &[DispatcherEntry] = &[DispatcherEntry { command: "tile", handler: Handler::Sync(run_tile_command) }];

const TILE_USAGE: &str = "usage: maw tile [N] [--wt <name>] [--layout nested|legacy] [--path <dir>] [--cmd <cmd>] [--shell] [--engine <name>] [--parent-session-id <id>] [--session-id <id>]";
const TILE_PANE_FORMAT: &str = "#{pane_id}|||#{pane_title}|||#{@maw_tile}";
const TILE_SWAP_FORMAT: &str = "#{pane_index}|||#{pane_id}|||#{pane_title}|||#{pane_top}";
const TILE_HEIGHT_FORMAT: &str = "#{pane_height}";
const TILE_COLORS: &[(&str, &str, &str)] = &[("blue", "34", "blue"), ("green", "32", "green"), ("yellow", "33", "yellow"), ("cyan", "36", "cyan"), ("magenta", "35", "magenta"), ("red", "31", "red"), ("white", "37", "white"), ("orange", "38;5;208", "colour208")];

#[derive(Debug, Clone, PartialEq, Eq)]
struct TileOptions { action: TileAction, count: usize, path: Option<String>, cmd: Option<String>, shell: bool, engine: Option<String>, layout: Option<String>, wt: TileWt, parent_session_id: Option<String>, session_id: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
enum TileAction { Help, Clean, Swap(String, String), Spawn }

#[derive(Debug, Clone, PartialEq, Eq)]
enum TileWt { None, Anonymous, Named(String) }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TilePaneRow { index: String, pane_id: String, title: String, top: i64 }

#[derive(Debug, Clone, PartialEq, Eq)]
struct TileCleanRow { pane_id: String, title: String, marker: String }

struct TileSplitRequest<'a> { anchor: &'a str, pane_ids: &'a [String], role: &'a str, cwd: &'a str, opts: &'a TileOptions, parent: &'a str, window: &'a str, tile_index: usize, total: usize }

fn run_tile_command(argv: &[String]) -> CliOutput {
    match tile_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, stdout, stderr)) => CliOutput { code, stdout, stderr: format!("{stderr}\n") },
    }
}

fn tile_run_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<String, (i32, String, String)> {
    if std::env::var_os("TMUX").is_none() { return Err((1, String::new(), "\x1b[33m⚠\x1b[0m tile requires tmux".to_owned())); }
    let options = tile_parse_args(argv)?;
    match &options.action {
        TileAction::Help => Ok(tile_help_text()),
        TileAction::Clean => tile_clean(runner).map_err(tile_err),
        TileAction::Swap(left, right) => tile_swap(left, right, runner).map_err(tile_err),
        TileAction::Spawn => tile_spawn(&options, runner).map_err(tile_err),
    }
}

fn tile_parse_args(argv: &[String]) -> Result<TileOptions, (i32, String, String)> {
    let (cli_args, wt) = tile_extract_wt_arg(argv)?;
    let mut opts = TileOptions { action: TileAction::Spawn, count: 0, path: None, cmd: None, shell: false, engine: None, layout: None, wt, parent_session_id: None, session_id: None };
    let mut pos = Vec::<String>::new();
    let mut i = 0usize;
    while i < cli_args.len() {
        let arg = &cli_args[i];
        match arg.as_str() {
            "--" => return Err(tile_usage_err("tile: -- separator is not supported")),
            "--help" | "-h" => opts.action = TileAction::Help,
            "--shell" => opts.shell = true,
            "--path" | "-p" => { i += 1; opts.path = Some(tile_take_value(&cli_args, i, arg)?); },
            "--cmd" | "-c" => { i += 1; opts.cmd = Some(tile_take_value(&cli_args, i, arg)?); },
            "--engine" | "-e" => { i += 1; opts.engine = Some(tile_safe_token(&tile_take_value(&cli_args, i, arg)?, "engine")?); },
            "--layout" => { i += 1; opts.layout = Some(tile_take_value(&cli_args, i, arg)?); },
            "--parent" | "--parent-session-id" => { i += 1; opts.parent_session_id = Some(tile_safe_token(&tile_take_value(&cli_args, i, arg)?, "parent session")?); },
            "--session-id" => { i += 1; opts.session_id = Some(tile_safe_token(&tile_take_value(&cli_args, i, arg)?, "session id")?); },
            value if value.starts_with('-') => return Err(tile_usage_err(&format!("tile: unknown argument {value}"))),
            value => pos.push(value.to_owned()),
        }
        i += 1;
    }
    tile_action_from_positionals(&mut opts, &pos)?;
    tile_validate_options(&opts)?;
    Ok(opts)
}

fn tile_extract_wt_arg(argv: &[String]) -> Result<(Vec<String>, TileWt), (i32, String, String)> {
    let mut normalized = Vec::new();
    let mut wt = TileWt::None;
    let mut i = 0usize;
    while i < argv.len() {
        let arg = &argv[i];
        if arg == "--wt" {
            if argv.get(i + 1).is_some_and(|next| !next.starts_with('-')) { i += 1; wt = TileWt::Named(tile_safe_token(&argv[i], "worktree")?); }
            else { wt = TileWt::Anonymous; }
        } else if let Some(value) = arg.strip_prefix("--wt=") { wt = TileWt::Named(tile_safe_token(value, "worktree")?); }
        else { normalized.push(arg.clone()); }
        i += 1;
    }
    Ok((normalized, wt))
}

fn tile_action_from_positionals(opts: &mut TileOptions, pos: &[String]) -> Result<(), (i32, String, String)> {
    if matches!(opts.action, TileAction::Help) { return Ok(()); }
    match pos.first().map(String::as_str) {
        Some("clean") => opts.action = TileAction::Clean,
        Some("swap") => {
            let Some(left) = pos.get(1) else { return Err((1, "usage: maw tile swap <pane-a> <pane-b>\n".to_owned(), "two pane targets required".to_owned())); };
            let Some(right) = pos.get(2) else { return Err((1, "usage: maw tile swap <pane-a> <pane-b>\n".to_owned(), "two pane targets required".to_owned())); };
            opts.action = TileAction::Swap(tile_validate_pane_spec(left)?, tile_validate_pane_spec(right)?);
        }
        Some(value) => opts.count = tile_parse_count(value)?,
        None => opts.count = 0,
    }
    Ok(())
}

fn tile_validate_options(opts: &TileOptions) -> Result<(), (i32, String, String)> {
    if let Some(layout) = &opts.layout {
        if layout != "nested" && layout != "legacy" { return Err((1, "\x1b[33m⚠\x1b[0m tile: --layout must be nested or legacy\n".to_owned(), "invalid layout".to_owned())); }
    }
    if opts.count > 10 { return Err(tile_msg_err(&format!("tile: max 10 panes (got {})", opts.count))); }
    if opts.cmd.as_ref().is_some_and(|cmd| cmd.trim().is_empty()) { return Err(tile_msg_err("tile: --cmd cannot be empty")); }
    if opts.path.as_ref().is_some_and(|path| path.trim().is_empty()) { return Err(tile_msg_err("tile: --path cannot be empty")); }
    Ok(())
}

fn tile_parse_count(value: &str) -> Result<usize, (i32, String, String)> {
    let trimmed = value.trim();
    if trimmed.starts_with('-') || trimmed.is_empty() { return Err(tile_invalid_count(value)); }
    let digits = trimmed.chars().take_while(char::is_ascii_digit).collect::<String>();
    if digits.is_empty() { return Err(tile_invalid_count(value)); }
    digits.parse::<usize>().map_err(|_| tile_invalid_count(value))
}

fn tile_invalid_count(value: &str) -> (i32, String, String) { (1, format!("\x1b[33m⚠\x1b[0m tile: expected a number, got '{value}'\n"), "invalid count".to_owned()) }

fn tile_spawn<R: maw_tmux::TmuxRunner>(opts: &TileOptions, runner: &mut R) -> Result<String, String> {
    let window = tile_get_window(runner)?;
    if opts.count == 0 { tile_select_layout(&window, "tiled", runner)?; return Ok("\x1b[32m✓\x1b[0m tiled\n".to_owned()); }
    let anchor = std::env::var("TMUX_PANE").unwrap_or_default();
    let parent = tile_parent_address(&anchor, runner);
    let window_address = tile_window_address(&anchor, &window, runner);
    let existing = tile_existing_count(&window, runner);
    let total = existing + opts.count;
    let cwd = tile_resolve_path(opts.path.as_deref())?;
    let mut pane_ids = Vec::<String>::new();
    let mut out = String::new();
    for i in 0..opts.count {
        let tile_index = existing + i + 1;
        let role = tile_role(&parent, tile_index);
        let request = TileSplitRequest { anchor: &anchor, pane_ids: &pane_ids, role: &role, cwd: &cwd, opts, parent: &parent, window: &window_address, tile_index, total };
        let pane_id = tile_split_pane(&request, runner)?;
        pane_ids.push(pane_id.clone());
        tile_style_pane(&pane_id, &role, i, runner)?;
        tile_tag_pane(&pane_id, &parent, &role, runner)?;
        if opts.cmd.is_none() { tile_send_engine(&pane_id, opts.engine.as_deref(), runner)?; }
        out.push_str(&tile_spawn_line(i, &role, &pane_id, &cwd, opts));
    }
    tile_layout_after_spawn(&window, runner)?;
    out.push_str(&tile_summary(opts, &cwd));
    Ok(out)
}

fn tile_clean<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, String> {
    let window = tile_get_window(runner)?;
    let my_pane = std::env::var("TMUX_PANE").unwrap_or_default();
    let raw = tile_tmux(runner, "list-panes", &["-t", &window, "-F", TILE_PANE_FORMAT])?;
    let mut killed = 0usize;
    let mut out = String::new();
    for row in raw.lines().filter_map(tile_parse_clean_row) {
        if row.pane_id == my_pane || !tile_is_tile_pane(&row) { continue; }
        tile_validate_tmux_target(&row.pane_id)?;
        if runner.run("kill-pane", &["-t".to_owned(), row.pane_id.clone()]).is_ok() {
            let _ = writeln!(out, "  \x1b[31m✗\x1b[0m {} ({})", row.title, row.pane_id);
            killed += 1;
        }
    }
    if killed == 0 { out.push_str("\x1b[90mno tile panes or worktrees to clean\x1b[0m\n"); }
    else { let _ = writeln!(out, "\x1b[32m✓\x1b[0m cleaned {killed} tiles"); }
    Ok(out)
}

fn tile_swap<R: maw_tmux::TmuxRunner>(left: &str, right: &str, runner: &mut R) -> Result<String, String> {
    let window = tile_get_window(runner)?;
    let raw = tile_tmux(runner, "list-panes", &["-t", &window, "-F", TILE_SWAP_FORMAT])?;
    let rows = raw.lines().filter_map(tile_parse_pane_row).collect::<Vec<_>>();
    let source = tile_resolve_pane(left, &rows).ok_or_else(|| format!("tile swap: could not resolve pane '{left}'"))?;
    let target = tile_resolve_pane(right, &rows).ok_or_else(|| format!("tile swap: could not resolve pane '{right}'"))?;
    if source.pane_id == target.pane_id { return Err("tile swap: source and target are the same pane".to_owned()); }
    tile_validate_tmux_target(&source.pane_id)?;
    tile_validate_tmux_target(&target.pane_id)?;
    tile_tmux(runner, "swap-pane", &["-s", &source.pane_id, "-t", &target.pane_id])?;
    Ok(format!("\x1b[32m✓\x1b[0m swapped {} ↔ {}\n", tile_display_name(&source), tile_display_name(&target)))
}

fn tile_get_window<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, String> {
    if let Ok(pane) = std::env::var("TMUX_PANE") {
        tile_validate_tmux_target(&pane)?;
        return Ok(tile_tmux(runner, "display-message", &["-t", &pane, "-p", "#{window_id}"])?.trim().to_owned());
    }
    Ok(tile_tmux(runner, "display-message", &["-p", "#{window_id}"])?.trim().to_owned())
}

fn tile_parent_address<R: maw_tmux::TmuxRunner>(anchor: &str, runner: &mut R) -> String {
    if anchor.is_empty() || tile_validate_tmux_target(anchor).is_err() { return anchor.to_owned(); }
    tile_tmux(runner, "display-message", &["-t", anchor, "-p", "#{session_name}:#{window_index}.#{pane_index}"]).map_or_else(|_| anchor.to_owned(), |s| s.trim().to_owned())
}

fn tile_window_address<R: maw_tmux::TmuxRunner>(anchor: &str, fallback: &str, runner: &mut R) -> String {
    if anchor.is_empty() || tile_validate_tmux_target(anchor).is_err() { return fallback.to_owned(); }
    tile_tmux(runner, "display-message", &["-t", anchor, "-p", "#{session_name}:#{window_index}"]).map_or_else(|_| fallback.to_owned(), |s| s.trim().to_owned())
}

fn tile_existing_count<R: maw_tmux::TmuxRunner>(window: &str, runner: &mut R) -> usize {
    tile_tmux(runner, "list-panes", &["-t", window, "-F", TILE_PANE_FORMAT]).map_or(0, |raw| raw.lines().filter_map(tile_parse_clean_row).filter(tile_is_tile_pane).count())
}

fn tile_split_pane<R: maw_tmux::TmuxRunner>(req: &TileSplitRequest<'_>, runner: &mut R) -> Result<String, String> {
    let split_from = req.pane_ids.last().map_or(req.anchor, String::as_str);
    let shell = tile_shell_command(req);
    let mut args = Vec::new();
    if !split_from.is_empty() {
        tile_validate_tmux_target(split_from)?;
        args.extend(["-t".to_owned(), split_from.to_owned()]);
    }
    args.extend(["-h".to_owned(), "-P".to_owned(), "-F".to_owned(), "#{pane_id}".to_owned(), shell]);
    let pane = runner.run("split-window", &args).map_err(|error| error.message)?.trim().to_owned();
    tile_validate_tmux_target(&pane)?;
    Ok(pane)
}

fn tile_shell_command(req: &TileSplitRequest<'_>) -> String {
    let command = req.opts.cmd.as_deref().unwrap_or_default();
    let mut envs = vec![
        format!("MAW_TILE_PARENT={}", tile_shell_quote(req.parent)),
        format!("MAW_TILE_ROLE={}", tile_shell_quote(req.role)),
        format!("MAW_TILE_INDEX={}", tile_shell_quote(&req.tile_index.to_string())),
        format!("MAW_TILE_TOTAL={}", tile_shell_quote(&req.total.to_string())),
        format!("MAW_TILE_WINDOW={}", tile_shell_quote(req.window)),
    ];
    if let Some(parent_id) = &req.opts.parent_session_id { envs.push(format!("MAW_PARENT_SESSION_ID={}", tile_shell_quote(parent_id))); }
    if req.opts.count == 1 { if let Some(session_id) = &req.opts.session_id { envs.push(format!("MAW_SESSION_ID={}", tile_shell_quote(session_id))); } }
    let body = if command.is_empty() { "exec zsh".to_owned() } else { format!("exec zsh -ic {}", tile_shell_quote(&format!("{command}; exec zsh"))) };
    let mut shell = format!("export {}; {body}", envs.join(" "));
    if !req.cwd.is_empty() { shell = format!("cd {} || exit $?; {shell}", tile_shell_quote(req.cwd)); }
    shell
}

fn tile_style_pane<R: maw_tmux::TmuxRunner>(pane_id: &str, role: &str, index: usize, runner: &mut R) -> Result<(), String> {
    let (_, _, tmux_color) = TILE_COLORS[index % TILE_COLORS.len()];
    tile_tmux(runner, "select-pane", &["-t", pane_id, "-T", role])?;
    tile_tmux(runner, "set-option", &["-p", "-t", pane_id, "pane-border-format", &format!("#[fg={tmux_color},bold] #{{pane_title}}")])?;
    tile_tmux(runner, "set-option", &["-p", "-t", pane_id, "pane-active-border-style", &format!("fg={tmux_color}")])?;
    Ok(())
}

fn tile_tag_pane<R: maw_tmux::TmuxRunner>(pane_id: &str, parent: &str, role: &str, runner: &mut R) -> Result<(), String> {
    tile_tmux(runner, "set-option", &["-p", "-t", pane_id, "@maw_tile", "1"])?;
    tile_tmux(runner, "set-option", &["-p", "-t", pane_id, "@maw_tile_parent", parent])?;
    tile_tmux(runner, "set-option", &["-p", "-t", pane_id, "@maw_tile_role", role])?;
    Ok(())
}

fn tile_send_engine<R: maw_tmux::TmuxRunner>(pane_id: &str, engine: Option<&str>, runner: &mut R) -> Result<(), String> {
    let Some(engine) = engine.filter(|value| !value.is_empty()) else { return Ok(()); };
    tile_tmux(runner, "send-keys", &["-t", pane_id, "C-u"])?;
    tile_tmux(runner, "send-keys", &["-t", pane_id, "-l", engine])?;
    tile_tmux(runner, "send-keys", &["-t", pane_id, "Enter"])?;
    Ok(())
}

fn tile_layout_after_spawn<R: maw_tmux::TmuxRunner>(window: &str, runner: &mut R) -> Result<(), String> {
    let count = tile_tmux(runner, "list-panes", &["-t", window, "-F", "#{pane_id}"])?.lines().filter(|line| !line.is_empty()).count();
    let layout = if count == 2 { "even-horizontal" } else if count <= 4 { "main-vertical" } else { "tiled" };
    tile_select_layout(window, layout, runner)?;
    tile_enable_border_status(window, runner);
    Ok(())
}

fn tile_select_layout<R: maw_tmux::TmuxRunner>(window: &str, layout: &str, runner: &mut R) -> Result<(), String> { tile_tmux(runner, "select-layout", &["-t", window, layout]).map(|_| ()) }

fn tile_enable_border_status<R: maw_tmux::TmuxRunner>(window: &str, runner: &mut R) {
    let heights = tile_tmux(runner, "list-panes", &["-t", window, "-F", TILE_HEIGHT_FORMAT]).unwrap_or_default();
    let ok = heights.lines().filter_map(|line| line.trim().parse::<i64>().ok()).all(|height| height >= 4);
    if ok { let _ = tile_tmux(runner, "set-option", &["-w", "-t", window, "pane-border-status", "bottom"]); }
}

fn tile_spawn_line(index: usize, role: &str, pane_id: &str, cwd: &str, opts: &TileOptions) -> String {
    let (_, ansi, _) = TILE_COLORS[index % TILE_COLORS.len()];
    let mut extras = Vec::new();
    if !cwd.is_empty() { extras.push(format!("\x1b[90m{cwd}\x1b[0m")); }
    if opts.cmd.is_some() { extras.push("\x1b[90mcmd\x1b[0m".to_owned()); }
    if opts.engine.is_some() && opts.cmd.is_none() { extras.push(format!("\x1b[90m{}\x1b[0m", opts.engine.as_deref().unwrap_or_default())); }
    format!("  \x1b[{ansi}m●\x1b[0m {role} → {pane_id}{}\n", if extras.is_empty() { String::new() } else { format!("  {}", extras.join(" ")) })
}

fn tile_summary(opts: &TileOptions, cwd: &str) -> String {
    let mut flags = Vec::new();
    match &opts.wt { TileWt::None => {}, TileWt::Anonymous => flags.push("worktree".to_owned()), TileWt::Named(name) => flags.push(format!("worktree:{name}")), }
    if !cwd.is_empty() { flags.push("path".to_owned()); }
    if opts.cmd.is_some() { flags.push("cmd".to_owned()); }
    else if let Some(engine) = &opts.engine { flags.push(engine.clone()); }
    format!("\x1b[32m✓\x1b[0m {} panes tiled{}\n", opts.count, if flags.is_empty() { String::new() } else { format!(" ({})", flags.join(", ")) })
}

fn tile_resolve_path(raw: Option<&str>) -> Result<String, String> {
    let Some(raw) = raw else { return Ok(String::new()); };
    if raw.trim().is_empty() { return Err("tile: --path cannot be empty".to_owned()); }
    let path = if raw == "~" { std::env::var("HOME").unwrap_or_else(|_| raw.to_owned()) } else if let Some(rest) = raw.strip_prefix("~/") { format!("{}/{}", std::env::var("HOME").unwrap_or_else(|_| "~".to_owned()), rest) } else { raw.to_owned() };
    let full = std::path::PathBuf::from(path);
    let resolved = if full.is_absolute() { full } else { std::env::current_dir().map_err(|error| error.to_string())?.join(full) };
    if !resolved.exists() { return Err(format!("tile: path does not exist: {raw}")); }
    if !resolved.is_dir() { return Err(format!("tile: path is not a directory: {raw}")); }
    Ok(resolved.to_string_lossy().into_owned())
}

fn tile_parse_clean_row(line: &str) -> Option<TileCleanRow> {
    let mut parts = line.split("|||");
    let pane_id = parts.next().unwrap_or_default().to_owned();
    if pane_id.is_empty() { return None; }
    Some(TileCleanRow { pane_id, title: parts.next().unwrap_or_default().to_owned(), marker: parts.next().unwrap_or_default().to_owned() })
}

fn tile_parse_pane_row(line: &str) -> Option<TilePaneRow> {
    let mut parts = line.split("|||");
    let index = parts.next().unwrap_or_default().to_owned();
    let pane_id = parts.next().unwrap_or_default().to_owned();
    if pane_id.is_empty() { return None; }
    Some(TilePaneRow { index, pane_id, title: parts.next().unwrap_or_default().to_owned(), top: parts.next().unwrap_or("0").parse::<i64>().unwrap_or(0) })
}

fn tile_resolve_pane(spec: &str, rows: &[TilePaneRow]) -> Option<TilePaneRow> {
    match spec {
        "top" => tile_top(rows, true),
        "bottom" => tile_top(rows, false),
        value if value.starts_with('%') => rows.iter().find(|row| row.pane_id == value).cloned().or_else(|| Some(TilePaneRow { index: String::new(), pane_id: value.to_owned(), title: value.to_owned(), top: 0 })),
        value if value.chars().all(|ch| ch.is_ascii_digit()) => rows.iter().find(|row| row.index == value).cloned(),
        value => rows.iter().find(|row| row.title == value || row.title.starts_with(value)).cloned(),
    }
}

fn tile_top(rows: &[TilePaneRow], first: bool) -> Option<TilePaneRow> {
    let mut sorted = rows.to_vec();
    if first { sorted.sort_by(|a, b| a.top.cmp(&b.top).then(tile_index_num(&a.index).cmp(&tile_index_num(&b.index)))); }
    else { sorted.sort_by(|a, b| b.top.cmp(&a.top).then(tile_index_num(&b.index).cmp(&tile_index_num(&a.index)))); }
    sorted.into_iter().next()
}

fn tile_index_num(value: &str) -> i64 { value.parse::<i64>().unwrap_or(0) }

fn tile_display_name(row: &TilePaneRow) -> &str { if row.title.is_empty() { &row.pane_id } else { &row.title } }

fn tile_is_tile_pane(row: &TileCleanRow) -> bool { row.marker == "1" || tile_title_matches(&row.title) }

fn tile_title_matches(title: &str) -> bool {
    let base = title.strip_suffix(" 🌳").unwrap_or(title);
    base.rsplit_once("tile-").is_some_and(|(_, tail)| !tail.is_empty() && tail.chars().all(|ch| ch.is_ascii_digit()))
}

fn tile_role(parent: &str, index: usize) -> String {
    let scope = tile_scope(parent);
    if scope.is_empty() { format!("tile-{index}") } else { format!("{scope}-tile-{index}") }
}

fn tile_scope(parent: &str) -> String {
    let session = parent.split(':').next().unwrap_or_default();
    let mapped = session.chars().map(|ch| if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-') { ch } else { '-' }).collect::<String>();
    mapped.trim_matches('-').to_owned()
}

fn tile_validate_pane_spec(value: &str) -> Result<String, (i32, String, String)> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || trimmed.starts_with('-') || trimmed == "--" || trimmed.chars().any(char::is_control) { return Err(tile_msg_err(&format!("tile: invalid pane target {value:?}"))); }
    if matches!(trimmed, "top" | "bottom") || trimmed.chars().all(|ch| ch.is_ascii_digit()) { return Ok(trimmed.to_owned()); }
    if let Some(rest) = trimmed.strip_prefix('%') {
        if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) { return Ok(trimmed.to_owned()); }
        return Err(tile_msg_err(&format!("tile: invalid pane id {value:?}")));
    }
    if trimmed.chars().all(tile_safe_title_char) { return Ok(trimmed.to_owned()); }
    Err(tile_msg_err(&format!("tile: invalid pane title prefix {value:?}")))
}

fn tile_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" || value.chars().any(char::is_control) { return Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned()); }
    Ok(())
}

fn tile_safe_title_char(ch: char) -> bool { ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') }

fn tile_safe_token(value: &str, label: &str) -> Result<String, (i32, String, String)> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" || value.chars().any(char::is_control) { return Err(tile_msg_err(&format!("tile: invalid {label} {value:?}"))); }
    if !value.chars().all(tile_safe_title_char) { return Err(tile_msg_err(&format!("tile: invalid {label} {value:?}"))); }
    Ok(value.to_owned())
}

fn tile_take_value(args: &[String], index: usize, flag: &str) -> Result<String, (i32, String, String)> {
    let Some(value) = args.get(index) else { return Err(tile_usage_err(&format!("tile: {flag} requires a value"))); };
    if value == "--" || value.starts_with('-') { return Err(tile_usage_err(&format!("tile: {flag} requires a value"))); }
    Ok(value.clone())
}

fn tile_tmux<R: maw_tmux::TmuxRunner>(runner: &mut R, subcommand: &str, args: &[&str]) -> Result<String, String> {
    tile_validate_tmux_args(args)?;
    runner.run(subcommand, &args.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>()).map_err(|error| error.message)
}

fn tile_validate_tmux_args(args: &[&str]) -> Result<(), String> {
    let mut wants_target = false;
    for value in args {
        if wants_target { tile_validate_tmux_target(value)?; wants_target = false; continue; }
        wants_target = matches!(*value, "-t" | "-s");
    }
    if wants_target { return Err("tmux target/session option missing value".to_owned()); }
    Ok(())
}

fn tile_shell_quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

fn tile_help_text() -> String {
    concat!(
        "usage: maw tile [N] [--wt <name>] [--layout nested|legacy] [--path <dir>] [--cmd <cmd>] [--shell] [--engine <name>] [--parent-session-id <id>] [--session-id <id>]\n",
        "       maw tile clean\n",
        "       maw tile swap <a> <b>\n\n",
        "  maw tile              apply tiled layout to current window (pane grid)\n",
        "  maw tile 3            spawn 3 empty panes and tile them\n",
        "  maw tile 3 -p /repo   spawn 3 shells cd'd to /repo\n",
        "  maw tile 3 -p /repo -c \"bun test\"  cd then run a command in each pane\n",
        "  maw tile 3 --shell    explicit blank-shell mode (default)\n",
        "  maw tile 3 --wt feat  spawn 3 blank shells in a reusable worktree\n",
        "  maw tile 3 --wt feat --layout legacy  use the old sibling .wt-N-X layout\n",
        "  maw tile 3 --wt       spawn 3 worktree-backed panes, each with own branch\n",
        "  maw tile 3 -e claude  spawn 3 panes running claude, tiled\n",
        "  maw tile clean        kill tile panes + remove tile worktrees\n",
        "  maw tile swap 1 2     swap pane indices in the current window\n",
        "  maw tile swap top bottom | tile-1 tile-2 | %1 %2\n"
    ).to_owned()
}

fn tile_msg_err(message: &str) -> (i32, String, String) { (1, String::new(), message.to_owned()) }

fn tile_usage_err(message: &str) -> (i32, String, String) { (1, String::new(), format!("{message}\n{TILE_USAGE}")) }

fn tile_err(message: String) -> (i32, String, String) { (1, String::new(), message) }

#[cfg(test)]
mod tile_tests {
    use super::*;

    fn tile_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn tile_parse_flags_and_rejects_separator_and_bad_targets() {
        let opts = tile_parse_args(&tile_strings(&["3", "--wt", "feat", "--layout", "nested", "-e", "claude"])).unwrap();
        assert_eq!(opts.count, 3);
        assert_eq!(opts.wt, TileWt::Named("feat".to_owned()));
        assert_eq!(opts.engine.as_deref(), Some("claude"));
        assert!(tile_parse_args(&tile_strings(&["--", "3"])).unwrap_err().2.contains("-- separator"));
        assert!(tile_parse_args(&tile_strings(&["swap", "-t", "1"])).unwrap_err().2.contains("unknown argument -t"));
    }

    #[test]
    fn tile_resolve_swap_targets_like_js() {
        let rows = vec![TilePaneRow { index: "0".to_owned(), pane_id: "%1".to_owned(), title: "lead".to_owned(), top: 20 }, TilePaneRow { index: "1".to_owned(), pane_id: "%2".to_owned(), title: "tile-1".to_owned(), top: 40 }, TilePaneRow { index: "2".to_owned(), pane_id: "%3".to_owned(), title: "tile-2".to_owned(), top: 10 }];
        assert_eq!(tile_resolve_pane("0", &rows).unwrap().pane_id, "%1");
        assert_eq!(tile_resolve_pane("tile", &rows).unwrap().pane_id, "%2");
        assert_eq!(tile_resolve_pane("top", &rows).unwrap().pane_id, "%3");
        assert_eq!(tile_resolve_pane("bottom", &rows).unwrap().pane_id, "%2");
        assert_eq!(tile_resolve_pane("%99", &rows).unwrap().pane_id, "%99");
    }
}
