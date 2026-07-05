const DISPATCH_76: &[DispatcherEntry] = &[DispatcherEntry { command: "panes", handler: Handler::Sync(run_panes_command) }];

const PANES_USAGE: &str = "usage: maw panes [target] [--pid] [--all|-a]  (see: maw pane swap, maw tile)";
const PANES_BASE_FORMAT: &str = "#{session_name}:#{window_index}.#{pane_index}|||#{pane_width}x#{pane_height}|||#{pane_current_command}|||#{pane_title}";
const PANES_PID_FORMAT: &str = "#{session_name}:#{window_index}.#{pane_index}|||#{pane_width}x#{pane_height}|||#{pane_current_command}|||#{pane_title}|||#{pane_pid}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PanesOptions { target: Option<String>, pid: bool, all: bool }

#[derive(Debug, Clone, PartialEq, Eq)]
struct PanesRow { target: String, dims: String, command: String, title: String, pid: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
enum PanesFilter { Current, All, Target(String) }

#[derive(Debug, Clone, PartialEq, Eq)]
enum PanesResolve { Match(String), None { hints: Vec<String> }, Ambiguous(Vec<String>) }

fn run_panes_command(argv: &[String]) -> CliOutput {
    match panes_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn panes_run_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<String, String> {
    let options = panes_parse_args(argv)?;
    let (filter, mut stdout) = panes_filter(&options, runner)?;
    let rows = panes_fetch_rows(&filter, options.pid, runner)?;
    stdout.push_str(&panes_render_rows(&rows, options.pid));
    Ok(stdout)
}

fn panes_parse_args(argv: &[String]) -> Result<PanesOptions, String> {
    let mut target = None::<String>;
    let mut pid = false;
    let mut all = false;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err(PANES_USAGE.to_owned()),
            "--pid" => pid = true,
            "--all" | "-a" => all = true,
            value if value.starts_with('-') => return Err(format!("\"{value}\" looks like a flag, not a target.\n  {PANES_USAGE}")),
            value => {
                if target.is_some() { return Err(PANES_USAGE.to_owned()); }
                target = Some(panes_validate_target(value)?);
            }
        }
    }
    Ok(PanesOptions { target, pid, all })
}

fn panes_validate_target(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("panes: invalid tmux target {value:?}"));
    }
    if value.contains(' ') || value.contains('"') || value.contains('\'') || value.contains(';') || value.contains('|') || value.contains('&') || value.contains('`') || value.contains('$') || value.contains('\\') {
        return Err(format!("panes: invalid tmux target {value:?}"));
    }
    Ok(value.to_owned())
}

fn panes_filter<R: maw_tmux::TmuxRunner>(options: &PanesOptions, runner: &mut R) -> Result<(PanesFilter, String), String> {
    if options.all {
        let warning = options.target.as_ref().map_or_else(String::new, |_| "  \x1b[90m⚠ --all ignores target argument\x1b[0m\n".to_owned());
        return Ok((PanesFilter::All, warning));
    }
    let Some(target) = &options.target else { return Ok((PanesFilter::Current, String::new())); };
    if let Some((session, rest)) = target.split_once(':') {
        let matched = panes_resolve_session(session, runner)?;
        panes_validate_target(rest)?;
        return Ok((PanesFilter::Target(format!("{matched}:{rest}")), String::new()));
    }
    let matched = panes_resolve_session(target, runner)?;
    Ok((PanesFilter::Target(matched), String::new()))
}

fn panes_resolve_session<R: maw_tmux::TmuxRunner>(target: &str, runner: &mut R) -> Result<String, String> {
    panes_validate_target(target)?;
    let sessions = panes_list_sessions(runner)?;
    match panes_match_session(target, &sessions) {
        PanesResolve::Match(name) => Ok(name),
        PanesResolve::Ambiguous(candidates) => Err(panes_ambiguous_error(target, &candidates)),
        PanesResolve::None { hints } => Err(panes_missing_error(target, &hints)),
    }
}

fn panes_list_sessions<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<String>, String> {
    let args = vec!["-F".to_owned(), "#{session_name}".to_owned()];
    let raw = runner.run("list-sessions", &args).map_err(|error| error.message)?;
    let mut sessions = raw.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect::<Vec<_>>();
    sessions.sort();
    Ok(sessions)
}

fn panes_match_session(target: &str, sessions: &[String]) -> PanesResolve {
    if let Some(exact) = sessions.iter().find(|session| session.as_str() == target) { return PanesResolve::Match(exact.clone()); }
    let candidates = sessions.iter().filter(|session| session.contains(target)).cloned().collect::<Vec<_>>();
    match candidates.len() {
        0 => PanesResolve::None { hints: panes_hints(target, sessions) },
        1 => PanesResolve::Match(candidates[0].clone()),
        _ => PanesResolve::Ambiguous(candidates),
    }
}

fn panes_hints(target: &str, sessions: &[String]) -> Vec<String> {
    let lower = target.to_ascii_lowercase();
    sessions.iter().filter(|session| session.to_ascii_lowercase().contains(&lower) || lower.contains(&session.to_ascii_lowercase())).take(5).cloned().collect()
}

fn panes_ambiguous_error(target: &str, candidates: &[String]) -> String {
    let mut out = format!("  \x1b[31m✗\x1b[0m '{target}' is ambiguous — matches {} sessions:", candidates.len());
    for session in candidates { let _ = write!(out, "\n  \x1b[90m    • {session}\x1b[0m"); }
    let _ = write!(out, "\n'{target}' is ambiguous — matches {} sessions", candidates.len());
    out
}

fn panes_missing_error(target: &str, hints: &[String]) -> String {
    let mut out = String::new();
    if hints.is_empty() { out.push_str("  \x1b[90m  try: maw ls\x1b[0m\n"); }
    else {
        out.push_str("  \x1b[90m  did you mean:\x1b[0m");
        for session in hints { let _ = write!(out, "\n  \x1b[90m    • {session}\x1b[0m"); }
        out.push('\n');
    }
    let _ = write!(out, "session '{target}' not found");
    out
}

fn panes_fetch_rows<R: maw_tmux::TmuxRunner>(filter: &PanesFilter, pid: bool, runner: &mut R) -> Result<Vec<PanesRow>, String> {
    let args = panes_list_args(filter, pid)?;
    let raw = runner.run("list-panes", &args).map_err(|error| format!("list-panes failed: {}", error.message))?;
    Ok(raw.lines().filter(|line| !line.is_empty()).map(|line| panes_parse_row(line, pid)).collect())
}

fn panes_list_args(filter: &PanesFilter, pid: bool) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    match filter {
        PanesFilter::Current => {},
        PanesFilter::All => args.push("-a".to_owned()),
        PanesFilter::Target(target) => {
            panes_validate_target(target)?;
            args.push("-t".to_owned());
            args.push(target.clone());
        }
    }
    args.push("-F".to_owned());
    args.push(if pid { PANES_PID_FORMAT } else { PANES_BASE_FORMAT }.to_owned());
    Ok(args)
}

fn panes_parse_row(line: &str, pid: bool) -> PanesRow {
    let mut parts = line.split("|||");
    PanesRow { target: parts.next().unwrap_or_default().to_owned(), dims: parts.next().unwrap_or_default().to_owned(), command: parts.next().unwrap_or_default().to_owned(), title: parts.next().unwrap_or_default().to_owned(), pid: pid.then(|| parts.next().unwrap_or_default().to_owned()) }
}

fn panes_render_rows(rows: &[PanesRow], pid: bool) -> String {
    if rows.is_empty() { return "  \x1b[90m(no panes)\x1b[0m\n".to_owned(); }
    let widths = panes_widths(rows, pid);
    let mut out = String::new();
    panes_push_header(&mut out, &widths, pid);
    for row in rows { panes_push_row(&mut out, row, &widths, pid); }
    out
}

fn panes_widths(rows: &[PanesRow], pid: bool) -> (usize, usize, usize, usize) {
    let target = rows.iter().map(|row| row.target.len()).max().unwrap_or(0).max(6);
    let dims = rows.iter().map(|row| row.dims.len()).max().unwrap_or(0).max(6);
    let command = rows.iter().map(|row| row.command.len()).max().unwrap_or(0).max(7);
    let pid_width = if pid { rows.iter().map(|row| row.pid.as_deref().unwrap_or_default().len()).max().unwrap_or(0).max(3) } else { 0 };
    (target, dims, command, pid_width)
}

fn panes_push_header(out: &mut String, widths: &(usize, usize, usize, usize), pid: bool) {
    let (target, dims, command, pid_width) = *widths;
    if pid { let _ = writeln!(out, "  \x1b[90m{}  {}  {}  {}  TITLE\x1b[0m", panes_pad("TARGET", target), panes_pad("SIZE", dims), panes_pad("PID", pid_width), panes_pad("COMMAND", command)); }
    else { let _ = writeln!(out, "  \x1b[90m{}  {}  {}  TITLE\x1b[0m", panes_pad("TARGET", target), panes_pad("SIZE", dims), panes_pad("COMMAND", command)); }
}

fn panes_push_row(out: &mut String, row: &PanesRow, widths: &(usize, usize, usize, usize), pid: bool) {
    let (target, dims, command, pid_width) = *widths;
    if pid { let _ = writeln!(out, "  {}  {}  {}  {}  \x1b[90m{}\x1b[0m", panes_pad(&row.target, target), panes_pad(&row.dims, dims), panes_pad(row.pid.as_deref().unwrap_or_default(), pid_width), panes_pad(&row.command, command), row.title); }
    else { let _ = writeln!(out, "  {}  {}  {}  \x1b[90m{}\x1b[0m", panes_pad(&row.target, target), panes_pad(&row.dims, dims), panes_pad(&row.command, command), row.title); }
}

fn panes_pad(value: &str, width: usize) -> String { format!("{value:<width$}") }

#[cfg(test)]
mod panes_tests {
    use super::*;

    #[test]
    fn panes_parser_accepts_flags_and_rejects_leading_dash_target() {
        let opts = panes_parse_args(&["alpha".to_owned(), "--pid".to_owned(), "-a".to_owned()]).unwrap();
        assert_eq!(opts.target.as_deref(), Some("alpha"));
        assert!(opts.pid);
        assert!(opts.all);
        assert!(panes_parse_args(&["-bad".to_owned()]).unwrap_err().contains("looks like a flag"));
    }

    #[test]
    fn panes_render_pid_table_matches_shape() {
        let rows = vec![PanesRow { target: "s:0.0".to_owned(), dims: "80x24".to_owned(), command: "zsh".to_owned(), title: "lead".to_owned(), pid: Some("42".to_owned()) }];
        assert_eq!(panes_render_rows(&rows, true), "  \x1b[90mTARGET  SIZE    PID  COMMAND  TITLE\x1b[0m\n  s:0.0   80x24   42   zsh      \x1b[90mlead\x1b[0m\n");
    }
}
