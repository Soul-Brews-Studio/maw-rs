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

