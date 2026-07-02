const DISPATCH_88: &[DispatcherEntry] = &[DispatcherEntry {
    command: "bg",
    handler: Handler::Sync(bg_run_command),
}];

const BG_PREFIX: &str = "maw-bg-";
const BG_HELP: &str = "maw bg — run long commands in detached tmux without blocking the current pane\n\nusage:\n  maw bg \"<cmd>\" [--name X]              spawn detached tmux session\n  maw bg ls [--json]                     list active maw-bg-* sessions\n  maw bg tail <slug> [--lines N] [--follow]\n                                         sample last N lines (default 200)\n  maw bg attach <slug>                   attach (or switch-client inside tmux)\n  maw bg kill <slug> | --all             reap session(s)\n  maw bg gc [--dry-run] [--older-than DUR]\n                                         reap stale \"done\" sessions (default 24h)\n\nslug refs accept full slug, hash suffix (4 hex), or unique stem prefix.\n";
const BG_LIST_FORMAT: &str = "#{session_name}\t#{session_created}\t#{pane_current_command}";
const BG_DEFAULT_TAIL_LINES: u32 = 200;
const BG_DEFAULT_GC_SECONDS: u64 = 24 * 60 * 60;
const BG_FLAG_FOLLOW: u8 = 1 << 0;
const BG_FLAG_DRY_RUN: u8 = 1 << 1;
const BG_FLAG_ALL: u8 = 1 << 2;
const BG_FLAG_JSON: u8 = 1 << 3;
const BG_FLAG_HELP: u8 = 1 << 4;

type BgNow = fn() -> u64;
type BgInsideTmux = fn() -> bool;

#[derive(Debug, Clone, PartialEq, Eq)]
struct BgTmuxResult {
    status: i32,
    stdout: String,
    stderr: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BgSession {
    slug: String,
    session: String,
    age_seconds: u64,
    status: BgSessionStatus,
    last_line: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BgSessionStatus {
    Running,
    Done,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BgFlags {
    name: Option<String>,
    lines: Option<u32>,
    older_than: Option<String>,
    bits: u8,
    positionals: Vec<String>,
}

trait BgTmux {
    fn bg_run(&mut self, subcommand: &str, args: &[String]) -> Result<BgTmuxResult, String>;
    fn bg_attach(&mut self, args: &[String]) -> Result<i32, String>;
}

struct BgSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

impl BgSystemTmux {
    fn bg_new() -> Self {
        Self {
            runner: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl BgTmux for BgSystemTmux {
    fn bg_run(&mut self, subcommand: &str, args: &[String]) -> Result<BgTmuxResult, String> {
        bg_validate_tmux_subcommand(subcommand)?;
        bg_validate_tmux_args(args)?;
        match maw_tmux::TmuxRunner::run(&mut self.runner, subcommand, args) {
            Ok(stdout) => Ok(BgTmuxResult {
                status: 0,
                stdout,
                stderr: String::new(),
            }),
            Err(error) => Ok(BgTmuxResult {
                status: 1,
                stdout: String::new(),
                stderr: error.message,
            }),
        }
    }

    fn bg_attach(&mut self, args: &[String]) -> Result<i32, String> {
        bg_validate_tmux_args(args)?;
        let Some(subcommand) = args.first() else {
            return Err("bg: missing attach tmux subcommand".to_owned());
        };
        let rest = args[1..].to_vec();
        Ok(self.bg_run(subcommand, &rest)?.status)
    }
}

fn bg_run_command(argv: &[String]) -> CliOutput {
    bg_run_command_with(argv, &mut BgSystemTmux::bg_new(), bg_now_seconds, bg_inside_tmux_env)
}

fn bg_run_command_with(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
    inside_tmux: BgInsideTmux,
) -> CliOutput {
    match bg_run(argv, tmux, now, inside_tmux) {
        Ok((code, stdout)) => CliOutput {
            code,
            stdout,
            stderr: String::new(),
        },
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("Error: {message}\n"),
        },
    }
}

fn bg_run(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
    inside_tmux: BgInsideTmux,
) -> Result<(i32, String), (i32, String)> {
    if argv.is_empty() || argv[0] == "--help" || argv[0] == "-h" {
        return Ok((0, BG_HELP.to_owned()));
    }
    let sub = argv[0].as_str();
    let rest = &argv[1..];
    match sub {
        "ls" | "list" => bg_run_list(rest, tmux, now),
        "tail" => bg_run_tail(rest, tmux, now),
        "attach" => bg_run_attach(rest, tmux, now, inside_tmux),
        "kill" => bg_run_kill(rest, tmux, now),
        "gc" => bg_run_gc(rest, tmux, now),
        _ => bg_run_spawn(argv, tmux),
    }
}

fn bg_run_spawn(argv: &[String], tmux: &mut impl BgTmux) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    if bg_flags_has(&flags, BG_FLAG_HELP) {
        return Ok((0, BG_HELP.to_owned()));
    }
    let command = bg_command_from_positionals(&flags.positionals).map_err(|message| (1, message))?;
    bg_validate_command(&command).map_err(|message| (1, message))?;
    let slug = bg_spawn_slug(&command, flags.name.as_deref()).map_err(|message| (1, message))?;
    if bg_session_exists(&slug, tmux).map_err(|message| (1, message))? {
        return Err((2, format!("bg: already running: {slug}")));
    }
    let session = bg_session_name(&slug);
    let tmux_args = bg_new_session_args(&session, &command).map_err(|message| (1, message))?;
    let result = tmux.bg_run("new-session", &tmux_args).map_err(|message| (3, message))?;
    if result.status != 0 {
        return Err((3, bg_tmux_failure("new-session", result.status, &result.stderr)));
    }
    Ok((0, format!("{slug}\t{session}\n")))
}

fn bg_run_list(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    let sessions = bg_list_sessions(tmux, now).map_err(|message| (1, message))?;
    if bg_flags_has(&flags, BG_FLAG_JSON) {
        return bg_list_json(&sessions).map(|stdout| (0, stdout)).map_err(|message| (1, message));
    }
    Ok((0, bg_format_list(&sessions)))
}

fn bg_run_tail(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    let slug_ref = flags.positionals.first().ok_or_else(|| (1, "bg tail: missing <slug>".to_owned()))?;
    bg_validate_ref(slug_ref).map_err(|message| (1, message))?;
    let lines = flags.lines.unwrap_or(BG_DEFAULT_TAIL_LINES);
    let resolved = bg_resolve_slug(slug_ref, &bg_list_slugs(tmux, now).map_err(|message| (1, message))?)
        .map_err(|message| (1, message))?;
    let out = bg_tail_resolved(&resolved, lines, tmux).map_err(|message| (1, message))?;
    Ok((0, bg_tail_output(out, bg_flags_has(&flags, BG_FLAG_FOLLOW))))
}

fn bg_run_attach(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
    inside_tmux: BgInsideTmux,
) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    let slug_ref = flags.positionals.first().ok_or_else(|| (1, "bg attach: missing <slug>".to_owned()))?;
    bg_validate_ref(slug_ref).map_err(|message| (1, message))?;
    let resolved = bg_resolve_slug(slug_ref, &bg_list_slugs(tmux, now).map_err(|message| (1, message))?)
        .map_err(|message| (1, message))?;
    let tmux_args = bg_attach_args(&resolved, inside_tmux()).map_err(|message| (1, message))?;
    let code = tmux.bg_attach(&tmux_args).map_err(|message| (3, message))?;
    Ok((code, String::new()))
}

fn bg_run_kill(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    let killed = bg_kill(flags.positionals.first(), bg_flags_has(&flags, BG_FLAG_ALL), tmux, now).map_err(|message| (1, message))?;
    if killed.is_empty() {
        Ok((0, "(no sessions to kill)\n".to_owned()))
    } else {
        Ok((0, format!("killed: {}\n", killed.join(", "))))
    }
}

fn bg_run_gc(
    argv: &[String],
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<(i32, String), (i32, String)> {
    let flags = bg_parse_flags(argv).map_err(|message| (1, message))?;
    let threshold = match flags.older_than.as_deref() {
        Some(value) => bg_parse_duration(value).map_err(|message| (1, message))?,
        None => BG_DEFAULT_GC_SECONDS,
    };
    let sessions = bg_list_sessions(tmux, now).map_err(|message| (1, message))?;
    let mut reaped = Vec::new();
    let mut kept = Vec::new();
    for session in sessions {
        if session.status == BgSessionStatus::Done && session.age_seconds >= threshold {
            if !bg_flags_has(&flags, BG_FLAG_DRY_RUN) {
                bg_kill_session(&session.slug, tmux).map_err(|message| (1, message))?;
            }
            reaped.push(session.slug);
        } else {
            kept.push(session.slug);
        }
    }
    Ok((0, bg_gc_output(bg_flags_has(&flags, BG_FLAG_DRY_RUN), &reaped, &kept, threshold)))
}

fn bg_parse_flags(argv: &[String]) -> Result<BgFlags, String> {
    let mut flags = BgFlags::default();
    let mut index = 0usize;
    while index < argv.len() {
        let token = &argv[index];
        if token == "--" {
            flags.positionals.extend(argv[index + 1..].iter().cloned());
            break;
        }
        if !token.starts_with('-') {
            flags.positionals.push(token.clone());
            index += 1;
            continue;
        }
        index = bg_parse_flag_token(argv, index, &mut flags)?;
    }
    Ok(flags)
}

fn bg_parse_flag_token(argv: &[String], index: usize, flags: &mut BgFlags) -> Result<usize, String> {
    let token = &argv[index];
    let (key, inline) = bg_split_flag(token);
    match key.as_str() {
        "--follow" | "--dry-run" | "--all" | "--json" | "--help" | "-h" => {
            bg_assign_bool(flags, &key);
            Ok(index + 1)
        }
        "--name" | "--lines" | "--older-than" => {
            let (value, next) = bg_flag_value(argv, index, inline.as_deref(), &key)?;
            bg_assign_string(flags, &key, &value)?;
            Ok(next)
        }
        _ => {
            flags.positionals.push(token.clone());
            Ok(index + 1)
        }
    }
}

fn bg_split_flag(token: &str) -> (String, Option<String>) {
    if let Some((key, value)) = token.split_once('=') {
        (key.to_owned(), Some(value.to_owned()))
    } else {
        (token.to_owned(), None)
    }
}

fn bg_flag_value(
    argv: &[String],
    index: usize,
    inline: Option<&str>,
    key: &str,
) -> Result<(String, usize), String> {
    if let Some(value) = inline {
        return Ok((value.to_owned(), index + 1));
    }
    let Some(next) = argv.get(index + 1) else {
        return Err(format!("flag {key} requires a value"));
    };
    if next.starts_with('-') {
        return Err(format!("flag {key} requires a value"));
    }
    Ok((next.clone(), index + 2))
}

fn bg_assign_bool(flags: &mut BgFlags, key: &str) {
    match key {
        "--follow" => bg_flags_set(flags, BG_FLAG_FOLLOW),
        "--dry-run" => bg_flags_set(flags, BG_FLAG_DRY_RUN),
        "--all" => bg_flags_set(flags, BG_FLAG_ALL),
        "--json" => bg_flags_set(flags, BG_FLAG_JSON),
        "--help" | "-h" => bg_flags_set(flags, BG_FLAG_HELP),
        _ => {}
    }
}

fn bg_flags_set(flags: &mut BgFlags, bit: u8) {
    flags.bits |= bit;
}

fn bg_flags_has(flags: &BgFlags, bit: u8) -> bool {
    flags.bits & bit != 0
}

fn bg_assign_string(flags: &mut BgFlags, key: &str, value: &str) -> Result<(), String> {
    match key {
        "--name" => flags.name = Some(value.to_owned()),
        "--lines" => flags.lines = Some(bg_parse_lines(value)?),
        "--older-than" => flags.older_than = Some(value.to_owned()),
        _ => {}
    }
    Ok(())
}

fn bg_command_from_positionals(positionals: &[String]) -> Result<String, String> {
    if positionals.is_empty() {
        return Err("bg: missing command (usage: maw bg \"<cmd>\")".to_owned());
    }
    Ok(positionals.join(" ").trim().to_owned())
}

fn bg_spawn_slug(command: &str, name: Option<&str>) -> Result<String, String> {
    if let Some(name) = name {
        bg_validate_name(name)?;
        Ok(name.to_owned())
    } else {
        bg_derive_slug(command)
    }
}

fn bg_derive_slug(command: &str) -> Result<String, String> {
    let trimmed = command.trim();
    if trimmed.is_empty() {
        return Err("bg: command cannot be empty".to_owned());
    }
    let first = trimmed.split_whitespace().next().unwrap_or_default();
    let mut stem = bg_slug_stem(first);
    if stem.is_empty() {
        stem.clear();
        stem.push_str("cmd");
    }
    let hash = hash_body(Some(command.as_bytes()));
    Ok(format!("{}-{}", stem, &hash[..4]))
}

fn bg_slug_stem(first: &str) -> String {
    let mut out = String::new();
    let mut last_dash = false;
    for ch in first.to_lowercase().chars() {
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() {
            out.push(ch);
            last_dash = false;
        } else if ch == '-' && !last_dash && !out.is_empty() {
            out.push('-');
            last_dash = true;
        }
        if out.len() >= 16 {
            break;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    out
}

fn bg_validate_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.len() > 32 || name.starts_with('-') || name == "--" {
        return Err(format!("bg: invalid --name \"{name}\" (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)"));
    }
    let mut chars = name.chars();
    if !chars.next().is_some_and(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit()) {
        return Err(format!("bg: invalid --name \"{name}\" (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)"));
    }
    if !chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-') {
        return Err(format!("bg: invalid --name \"{name}\" (must match ^[a-z0-9][a-z0-9-]{{0,31}}$)"));
    }
    Ok(())
}

fn bg_validate_command(command: &str) -> Result<(), String> {
    if command.is_empty() || command.starts_with('-') || command == "--" {
        return Err("bg: command must be non-empty and not start with '-'".to_owned());
    }
    if command.chars().any(|ch| ch == '\0' || ch.is_control()) {
        return Err("bg: command must not contain NUL/control characters".to_owned());
    }
    Ok(())
}

fn bg_validate_ref(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value == "--" || value.trim() != value {
        return Err("bg ref must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("bg ref must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn bg_validate_session_name(value: &str) -> Result<(), String> {
    if !value.starts_with(BG_PREFIX) {
        return Err(format!("bg: refusing non-bg session {value}"));
    }
    bg_validate_tmux_target(value)
}

fn bg_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.trim() != value {
        return Err("bg tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("bg tmux target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn bg_validate_tmux_subcommand(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value == "--" || value.contains(char::is_whitespace) {
        return Err("bg tmux subcommand must be a safe token".to_owned());
    }
    Ok(())
}

fn bg_validate_tmux_args(args: &[String]) -> Result<(), String> {
    for pair in args.windows(2) {
        if pair[0] == "-t" || pair[0] == "-s" {
            bg_validate_tmux_target(&pair[1])?;
        }
    }
    Ok(())
}

fn bg_session_name(slug: &str) -> String {
    format!("{BG_PREFIX}{slug}")
}

fn bg_session_slug(session: &str) -> Option<String> {
    session.strip_prefix(BG_PREFIX).map(str::to_owned)
}

fn bg_new_session_args(session: &str, command: &str) -> Result<Vec<String>, String> {
    bg_validate_session_name(session)?;
    bg_validate_command(command)?;
    Ok(vec![
        "-d".to_owned(),
        "-s".to_owned(),
        session.to_owned(),
        "-n".to_owned(),
        "bg".to_owned(),
        "/bin/sh".to_owned(),
        "-c".to_owned(),
        bg_holds_open(command),
    ])
}

fn bg_holds_open(command: &str) -> String {
    format!("{command}; rc=$?; printf '\\n[done — exit %d]\\n' \"$rc\"; while :; do read -r _ 2>/dev/null || sleep 3600; done")
}

fn bg_session_exists(slug: &str, tmux: &mut impl BgTmux) -> Result<bool, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let args = vec!["-t".to_owned(), session];
    Ok(tmux.bg_run("has-session", &args)?.status == 0)
}

fn bg_list_sessions(tmux: &mut impl BgTmux, now: BgNow) -> Result<Vec<BgSession>, String> {
    let args = vec!["-F".to_owned(), BG_LIST_FORMAT.to_owned()];
    let result = tmux.bg_run("list-sessions", &args)?;
    if result.status != 0 && bg_list_error_is_empty(&result) {
        return Ok(Vec::new());
    }
    let mut sessions = Vec::new();
    for line in result.stdout.lines() {
        if let Some(session) = bg_session_from_line(line, tmux, now)? {
            sessions.push(session);
        }
    }
    Ok(sessions)
}

fn bg_session_from_line(
    line: &str,
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<Option<BgSession>, String> {
    let mut fields = line.split('\t');
    let name = fields.next().unwrap_or_default();
    if !name.starts_with(BG_PREFIX) {
        return Ok(None);
    }
    bg_validate_session_name(name)?;
    let created = fields.next().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or_else(now);
    let command = fields.next().unwrap_or_default();
    let slug = bg_session_slug(name).unwrap_or_default();
    bg_validate_ref(&slug)?;
    Ok(Some(BgSession {
        slug: slug.clone(),
        session: name.to_owned(),
        age_seconds: now().saturating_sub(created),
        status: bg_status_from_pane_command(command),
        last_line: bg_last_line_of(&slug, tmux).unwrap_or_default(),
    }))
}

fn bg_list_error_is_empty(result: &BgTmuxResult) -> bool {
    result.stdout.trim().is_empty()
        || result.stderr.contains("no server running")
        || result.stderr.contains("no current session")
}

fn bg_status_from_pane_command(command: &str) -> BgSessionStatus {
    match command.trim().to_ascii_lowercase().as_str() {
        "" | "read" | "sleep" | "sh" => BgSessionStatus::Done,
        _ => BgSessionStatus::Running,
    }
}

fn bg_last_line_of(slug: &str, tmux: &mut impl BgTmux) -> Result<String, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let args = vec![
        "-p".to_owned(),
        "-J".to_owned(),
        "-t".to_owned(),
        session,
        "-S".to_owned(),
        "-1".to_owned(),
        "-E".to_owned(),
        "-1".to_owned(),
    ];
    let result = tmux.bg_run("capture-pane", &args)?;
    if result.status == 0 {
        Ok(result.stdout.trim_end_matches('\n').trim().to_owned())
    } else {
        Ok(String::new())
    }
}

fn bg_list_slugs(tmux: &mut impl BgTmux, now: BgNow) -> Result<Vec<String>, String> {
    Ok(bg_list_sessions(tmux, now)?.into_iter().map(|session| session.slug).collect())
}

fn bg_resolve_slug(reference: &str, live: &[String]) -> Result<String, String> {
    bg_validate_ref(reference)?;
    if live.iter().any(|slug| slug == reference) {
        return Ok(reference.to_owned());
    }
    if bg_is_hash_ref(reference) {
        let hits = live.iter().filter(|slug| slug.ends_with(&format!("-{reference}"))).cloned().collect::<Vec<_>>();
        return bg_resolve_hits(reference, &hits, "hash");
    }
    let hits = live.iter().filter(|slug| slug.starts_with(reference)).cloned().collect::<Vec<_>>();
    bg_resolve_hits(reference, &hits, "ref")
}

fn bg_resolve_hits(reference: &str, hits: &[String], kind: &str) -> Result<String, String> {
    match hits {
        [hit] => Ok(hit.clone()),
        [] => Err(format!("bg: no session matching \"{reference}\"")),
        _ if kind == "hash" => Err(format!("bg: hash \"{reference}\" matches {} sessions: {}", hits.len(), hits.join(", "))),
        _ => Err(format!("bg: ref \"{reference}\" matches {} sessions: {}", hits.len(), hits.join(", "))),
    }
}

fn bg_is_hash_ref(value: &str) -> bool {
    value.len() == 4 && value.chars().all(|ch| ch.is_ascii_hexdigit() && !ch.is_ascii_uppercase())
}

fn bg_tail_resolved(slug: &str, lines: u32, tmux: &mut impl BgTmux) -> Result<String, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let args = vec!["-p".to_owned(), "-J".to_owned(), "-t".to_owned(), session, "-S".to_owned(), format!("-{lines}")];
    let result = tmux.bg_run("capture-pane", &args)?;
    if result.status != 0 {
        return Err(format!("bg: capture-pane failed for {slug}: {}", bg_stderr_or_placeholder(&result.stderr)));
    }
    Ok(result.stdout.trim_end_matches('\n').to_owned())
}

fn bg_tail_output(mut output: String, follow: bool) -> String {
    if follow {
        let _ = writeln!(output, "\n[bg: follow is single-snapshot in maw-rs native]");
    }
    output
}

fn bg_attach_args(slug: &str, inside_tmux: bool) -> Result<Vec<String>, String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    if inside_tmux {
        Ok(vec!["switch-client".to_owned(), "-t".to_owned(), session])
    } else {
        Ok(vec!["attach-session".to_owned(), "-t".to_owned(), session])
    }
}

fn bg_kill(
    slug: Option<&String>,
    all: bool,
    tmux: &mut impl BgTmux,
    now: BgNow,
) -> Result<Vec<String>, String> {
    if all {
        let slugs = bg_list_slugs(tmux, now)?;
        for slug in &slugs {
            bg_kill_session(slug, tmux)?;
        }
        return Ok(slugs);
    }
    let slug_ref = slug.ok_or_else(|| "bg kill: missing <slug> (or --all)".to_owned())?;
    bg_validate_ref(slug_ref)?;
    let resolved = bg_resolve_slug(slug_ref, &bg_list_slugs(tmux, now)?)?;
    bg_kill_session(&resolved, tmux)?;
    Ok(vec![resolved])
}

fn bg_kill_session(slug: &str, tmux: &mut impl BgTmux) -> Result<(), String> {
    bg_validate_ref(slug)?;
    let session = bg_session_name(slug);
    bg_validate_session_name(&session)?;
    let result = tmux.bg_run("kill-session", &["-t".to_owned(), session])?;
    if result.status != 0 {
        return Err(format!("bg: kill-session failed for {slug}: {}", bg_stderr_or_placeholder(&result.stderr)));
    }
    Ok(())
}

fn bg_parse_duration(value: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    let Some(unit) = trimmed.chars().last() else {
        return Err(bg_bad_duration(value));
    };
    let number = &trimmed[..trimmed.len() - unit.len_utf8()];
    let parsed = number.parse::<u64>().map_err(|_| bg_bad_duration(value))?;
    match unit {
        's' => Ok(parsed),
        'm' => Ok(parsed.saturating_mul(60)),
        'h' => Ok(parsed.saturating_mul(3_600)),
        'd' => Ok(parsed.saturating_mul(86_400)),
        _ => Err(bg_bad_duration(value)),
    }
}

fn bg_bad_duration(value: &str) -> String {
    format!("bg gc: invalid --older-than \"{value}\" (expected NNs/NNm/NNh/NNd)")
}

fn bg_parse_lines(value: &str) -> Result<u32, String> {
    let parsed = value.parse::<u32>().map_err(|_| format!("--lines must be a positive number, got {value}"))?;
    if parsed == 0 {
        return Err(format!("--lines must be a positive number, got {value}"));
    }
    Ok(parsed)
}

fn bg_format_list(sessions: &[BgSession]) -> String {
    if sessions.is_empty() {
        return "(no maw-bg sessions)\n".to_owned();
    }
    let rows = sessions.iter().map(bg_format_row_parts).collect::<Vec<_>>();
    let widths = bg_widths(&rows);
    let mut out = String::new();
    for row in rows {
        let _ = writeln!(
            out,
            "{:<w0$}  {:<w1$}  {:<w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
    }
    out
}

fn bg_format_row_parts(session: &BgSession) -> [String; 4] {
    [
        session.slug.clone(),
        bg_status_text(&session.status).to_owned(),
        bg_format_age(session.age_seconds),
        bg_truncate_last_line(&session.last_line),
    ]
}

fn bg_widths(rows: &[[String; 4]]) -> [usize; 3] {
    let mut widths = [0usize; 3];
    for row in rows {
        for index in 0..3 {
            widths[index] = widths[index].max(row[index].len());
        }
    }
    widths
}

fn bg_status_text(status: &BgSessionStatus) -> &'static str {
    match status {
        BgSessionStatus::Running => "running",
        BgSessionStatus::Done => "done",
    }
}

fn bg_format_age(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3_600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h", seconds / 3_600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn bg_truncate_last_line(line: &str) -> String {
    if line.len() > 60 {
        format!("{}...", &line[..57])
    } else {
        line.to_owned()
    }
}

fn bg_list_json(sessions: &[BgSession]) -> Result<String, String> {
    let values = sessions
        .iter()
        .map(|session| {
            serde_json::json!({
                "slug": session.slug,
                "session": session.session,
                "ageSeconds": session.age_seconds,
                "status": bg_status_text(&session.status),
                "lastLine": session.last_line,
            })
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&values)
        .map(|text| format!("{text}\n"))
        .map_err(|error| error.to_string())
}

fn bg_gc_output(dry_run: bool, reaped: &[String], kept: &[String], threshold: u64) -> String {
    let mut out = String::new();
    let verb = if dry_run { "would reap" } else { "reaped" };
    let _ = writeln!(out, "{verb}: {}", bg_join_or_none(reaped));
    let _ = writeln!(out, "kept:    {}", bg_join_or_none(kept));
    let _ = writeln!(out, "threshold: {threshold}s");
    out
}

fn bg_join_or_none(values: &[String]) -> String {
    if values.is_empty() {
        "(none)".to_owned()
    } else {
        values.join(", ")
    }
}

fn bg_tmux_failure(action: &str, status: i32, stderr: &str) -> String {
    format!("bg: tmux {action} failed (status {status}): {}", bg_stderr_or_placeholder(stderr))
}

fn bg_stderr_or_placeholder(stderr: &str) -> String {
    let trimmed = stderr.trim();
    if trimmed.is_empty() {
        "(no stderr)".to_owned()
    } else {
        trimmed.to_owned()
    }
}

fn bg_now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn bg_inside_tmux_env() -> bool {
    std::env::var_os("TMUX").is_some()
}

#[cfg(test)]
mod bg_tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct BgCall {
        subcommand: String,
        args: Vec<String>,
    }

    #[derive(Debug, Default)]
    struct BgFakeTmux {
        calls: Vec<BgCall>,
        attach_calls: Vec<Vec<String>>,
        responses: std::collections::VecDeque<BgTmuxResult>,
    }

    impl BgFakeTmux {
        fn bg_with_responses(responses: Vec<BgTmuxResult>) -> Self {
            Self {
                responses: responses.into(),
                ..Default::default()
            }
        }
    }

    impl BgTmux for BgFakeTmux {
        fn bg_run(&mut self, subcommand: &str, args: &[String]) -> Result<BgTmuxResult, String> {
            self.calls.push(BgCall {
                subcommand: subcommand.to_owned(),
                args: args.to_vec(),
            });
            Ok(self.responses.pop_front().unwrap_or_else(bg_ok_empty))
        }

        fn bg_attach(&mut self, args: &[String]) -> Result<i32, String> {
            self.attach_calls.push(args.to_vec());
            Ok(0)
        }
    }

    fn bg_ok(stdout: &str) -> BgTmuxResult {
        BgTmuxResult {
            status: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    fn bg_ok_empty() -> BgTmuxResult {
        bg_ok("")
    }

    fn bg_fail(stderr: &str) -> BgTmuxResult {
        BgTmuxResult {
            status: 1,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    fn bg_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn bg_now() -> u64 {
        1_700_000_000
    }

    fn bg_not_tmux() -> bool {
        false
    }

    fn bg_in_tmux() -> bool {
        true
    }

    #[test]
    fn bg_dispatch_registers_bg() {
        assert_eq!(DISPATCH_88[0].command, "bg");
    }

    #[test]
    fn bg_spawn_builds_safe_new_session_after_has_session() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![bg_fail("missing"), bg_ok_empty()]);
        let output = bg_run(&bg_strings(&["cargo", "test", "--name", "cargo-test"]), &mut tmux, bg_now, bg_not_tmux)
            .expect("spawn");
        assert_eq!(output.0, 0);
        assert_eq!(output.1, "cargo-test\tmaw-bg-cargo-test\n");
        assert_eq!(tmux.calls[0].subcommand, "has-session");
        assert_eq!(tmux.calls[1].subcommand, "new-session");
        assert!(tmux.calls[1].args.contains(&"/bin/sh".to_owned()));
        assert!(tmux.calls[1].args.contains(&"-c".to_owned()));
    }

    #[test]
    fn bg_rejects_leading_dash_command_before_spawn() {
        let mut tmux = BgFakeTmux::default();
        let error = bg_run(&bg_strings(&["--bad"]), &mut tmux, bg_now, bg_not_tmux).expect_err("bad");
        assert!(error.1.contains("command must"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn bg_rejects_bad_name_before_tmux() {
        let mut tmux = BgFakeTmux::default();
        let error = bg_run(&bg_strings(&["echo", "hi", "--name=-bad"]), &mut tmux, bg_now, bg_not_tmux)
            .expect_err("bad name");
        assert!(error.1.contains("invalid --name"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn bg_list_formats_sessions_and_captures_last_lines() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-build-a1b2\t1699999940\tcargo\nmaw-bg-done-b2c3\t1699996400\tsleep\nother\t1\tsh\n"),
            bg_ok("building\n"),
            bg_ok("[done — exit 0]\n"),
        ]);
        let output = bg_run(&bg_strings(&["ls"]), &mut tmux, bg_now, bg_not_tmux).expect("ls");
        assert!(output.1.contains("build-a1b2  running  1m"));
        assert!(output.1.contains("done-b2c3   done     1h"));
        assert_eq!(tmux.calls[0].subcommand, "list-sessions");
    }

    #[test]
    fn bg_json_list_is_camel_case_like_js() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![bg_ok("maw-bg-build-a1b2\t1699999990\tread\n"), bg_ok("tail\n")]);
        let output = bg_run(&bg_strings(&["list", "--json"]), &mut tmux, bg_now, bg_not_tmux).expect("json");
        assert!(output.1.contains("\"ageSeconds\": 10"));
        assert!(output.1.contains("\"status\": \"done\""));
    }

    #[test]
    fn bg_tail_resolves_hash_suffix_and_uses_lines_guard() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-build-a1b2\t1\tcargo\n"),
            bg_ok("last\n"),
            bg_ok("one\ntwo\n"),
        ]);
        let output = bg_run(&bg_strings(&["tail", "a1b2", "--lines", "2"]), &mut tmux, bg_now, bg_not_tmux).expect("tail");
        assert_eq!(output.1, "one\ntwo");
        let tail = tmux.calls.last().expect("tail call");
        assert_eq!(tail.subcommand, "capture-pane");
        assert!(tail.args.contains(&"-2".to_owned()));
    }

    #[test]
    fn bg_kill_all_validates_bg_targets_before_kill() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-one-a111\t1\tsleep\nmaw-bg-two-b222\t1\tread\n"),
            bg_ok("done\n"),
            bg_ok("done\n"),
            bg_ok_empty(),
            bg_ok_empty(),
        ]);
        let output = bg_run(&bg_strings(&["kill", "--all"]), &mut tmux, bg_now, bg_not_tmux).expect("kill");
        assert!(output.1.contains("killed: one-a111, two-b222"));
        assert_eq!(tmux.calls.iter().filter(|call| call.subcommand == "kill-session").count(), 2);
    }

    #[test]
    fn bg_gc_dry_run_does_not_kill() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-old-a111\t1699900000\tsleep\nmaw-bg-new-b222\t1699999990\tcargo\n"),
            bg_ok("old done\n"),
            bg_ok("new run\n"),
        ]);
        let output = bg_run(&bg_strings(&["gc", "--dry-run", "--older-than", "1h"]), &mut tmux, bg_now, bg_not_tmux)
            .expect("gc");
        assert!(output.1.contains("would reap: old-a111"));
        assert!(output.1.contains("kept:    new-b222"));
        assert!(!tmux.calls.iter().any(|call| call.subcommand == "kill-session"));
    }

    #[test]
    fn bg_attach_switches_inside_tmux_without_real_spawn() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![bg_ok("maw-bg-one-a111\t1\tcargo\n"), bg_ok("tail\n")]);
        let output = bg_run(&bg_strings(&["attach", "one"]), &mut tmux, bg_now, bg_in_tmux).expect("attach");
        assert_eq!(output.0, 0);
        assert_eq!(tmux.attach_calls[0][0], "switch-client");
        assert_eq!(tmux.attach_calls[0][2], "maw-bg-one-a111");
    }

    #[test]
    fn bg_resolve_ambiguous_prefix_is_error_before_kill() {
        let mut tmux = BgFakeTmux::bg_with_responses(vec![
            bg_ok("maw-bg-one-a111\t1\tcargo\nmaw-bg-one-b222\t1\tcargo\n"),
            bg_ok("a\n"),
            bg_ok("b\n"),
        ]);
        let error = bg_run(&bg_strings(&["kill", "one"]), &mut tmux, bg_now, bg_not_tmux).expect_err("ambiguous");
        assert!(error.1.contains("matches 2 sessions"));
        assert!(!tmux.calls.iter().any(|call| call.subcommand == "kill-session"));
    }
}
