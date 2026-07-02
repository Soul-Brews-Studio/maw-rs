const DISPATCH_73: &[DispatcherEntry] = &[DispatcherEntry { command: "pane", handler: Handler::Sync(run_pane_command) }];

const PANE_USAGE: &str = "usage: maw pane swap <pane-a> <pane-b>  (see: maw panes to list, maw tile for grids)";
const PANE_LIST_FORMAT: &str = "#{pane_index}|||#{pane_id}|||#{pane_title}|||#{pane_top}";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneOptions { action: PaneAction, left: Option<String>, right: Option<String> }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaneAction { Help, Swap }

#[derive(Debug, Clone, PartialEq, Eq)]
struct PaneRow { index: String, pane_id: String, title: String, top: i64 }

fn run_pane_command(argv: &[String]) -> CliOutput {
    match pane_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn pane_run_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<String, (i32, String)> {
    if std::env::var_os("TMUX").is_none() { return Err((1, "\x1b[33m⚠\x1b[0m pane requires tmux".to_owned())); }
    let options = pane_parse_args(argv)?;
    match options.action {
        PaneAction::Help => Ok(pane_help_text()),
        PaneAction::Swap => pane_swap(&options, runner).map_err(|message| (1, message)),
    }
}

fn pane_parse_args(argv: &[String]) -> Result<PaneOptions, (i32, String)> {
    let Some(sub) = argv.first().map(String::as_str) else { return Ok(PaneOptions { action: PaneAction::Help, left: None, right: None }); };
    match sub.to_ascii_lowercase().as_str() {
        "--help" | "-h" => Ok(PaneOptions { action: PaneAction::Help, left: None, right: None }),
        "swap" => pane_parse_swap_args(&argv[1..]),
        other => Err((1, format!("unknown pane subcommand: {other}\n{PANE_USAGE}"))),
    }
}

fn pane_parse_swap_args(argv: &[String]) -> Result<PaneOptions, (i32, String)> {
    if argv.len() != 2 { return Err((1, format!("{PANE_USAGE}\ntwo pane targets required"))); }
    let left = pane_validate_spec(&argv[0]).map_err(|message| (1, message))?;
    let right = pane_validate_spec(&argv[1]).map_err(|message| (1, message))?;
    Ok(PaneOptions { action: PaneAction::Swap, left: Some(left), right: Some(right) })
}

fn pane_help_text() -> String {
    concat!(
        "usage: maw pane swap <pane-a> <pane-b>  (see: maw panes to list, maw tile for grids)\n",
        "  pane targets: index (1), pane id (%1), title prefix (tile-1), top, bottom\n"
    ).to_owned()
}

fn pane_validate_spec(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || trimmed.starts_with('-') || trimmed.chars().any(char::is_control) {
        return Err(format!("pane: invalid pane target {value:?}"));
    }
    if matches!(trimmed, "top" | "bottom") || trimmed.chars().all(|ch| ch.is_ascii_digit()) { return Ok(trimmed.to_owned()); }
    if let Some(rest) = trimmed.strip_prefix('%') {
        if !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()) { return Ok(trimmed.to_owned()); }
        return Err(format!("pane: invalid pane id {value:?}"));
    }
    if trimmed.chars().all(pane_is_safe_title_char) { return Ok(trimmed.to_owned()); }
    Err(format!("pane: invalid pane title prefix {value:?}"))
}

fn pane_is_safe_title_char(ch: char) -> bool { ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') }

fn pane_swap<R: maw_tmux::TmuxRunner>(options: &PaneOptions, runner: &mut R) -> Result<String, String> {
    let window = pane_current_anchor()?;
    pane_swap_in_window(options, &window, runner)
}

fn pane_swap_in_window<R: maw_tmux::TmuxRunner>(options: &PaneOptions, window: &str, runner: &mut R) -> Result<String, String> {
    let left = options.left.as_deref().ok_or_else(|| "two pane targets required".to_owned())?;
    let right = options.right.as_deref().ok_or_else(|| "two pane targets required".to_owned())?;
    let rows = pane_list_rows(window, runner)?;
    let source = pane_resolve(left, &rows).ok_or_else(|| format!("tile swap: could not resolve pane '{left}'"))?;
    let target = pane_resolve(right, &rows).ok_or_else(|| format!("tile swap: could not resolve pane '{right}'"))?;
    if source.pane_id == target.pane_id { return Err("tile swap: source and target are the same pane".to_owned()); }
    pane_validate_tmux_target(&source.pane_id)?;
    pane_validate_tmux_target(&target.pane_id)?;
    pane_run_swap(&source.pane_id, &target.pane_id, runner)?;
    Ok(pane_render_swap_success(&source, &target))
}

fn pane_current_anchor() -> Result<String, String> {
    let pane = std::env::var("TMUX_PANE").unwrap_or_else(|_| ":".to_owned());
    pane_validate_tmux_target(&pane)?;
    Ok(pane)
}

fn pane_list_rows<R: maw_tmux::TmuxRunner>(window: &str, runner: &mut R) -> Result<Vec<PaneRow>, String> {
    pane_validate_tmux_target(window)?;
    let args = vec!["-t".to_owned(), window.to_owned(), "-F".to_owned(), PANE_LIST_FORMAT.to_owned()];
    let raw = runner.run("list-panes", &args).map_err(|error| error.message)?;
    Ok(raw.lines().filter_map(pane_parse_row).collect())
}

fn pane_parse_row(line: &str) -> Option<PaneRow> {
    let mut parts = line.split("|||");
    let index = parts.next().unwrap_or_default().to_owned();
    let pane_id = parts.next().unwrap_or_default().to_owned();
    let title = parts.next().unwrap_or_default().to_owned();
    let top = parts.next().unwrap_or("0").parse::<i64>().unwrap_or(0);
    if pane_id.is_empty() { return None; }
    Some(PaneRow { index, pane_id, title, top })
}

fn pane_resolve(spec: &str, rows: &[PaneRow]) -> Option<PaneRow> {
    match spec {
        "top" => pane_top(rows, true),
        "bottom" => pane_top(rows, false),
        value if value.starts_with('%') => pane_resolve_pane_id(value, rows),
        value if value.chars().all(|ch| ch.is_ascii_digit()) => rows.iter().find(|row| row.index == value).cloned(),
        value => rows.iter().find(|row| row.title == value || row.title.starts_with(value)).cloned(),
    }
}

fn pane_top(rows: &[PaneRow], top_first: bool) -> Option<PaneRow> {
    let mut sorted = rows.to_vec();
    if top_first { sorted.sort_by(|a, b| a.top.cmp(&b.top).then(pane_index_num(&a.index).cmp(&pane_index_num(&b.index)))); }
    else { sorted.sort_by(|a, b| b.top.cmp(&a.top).then(pane_index_num(&b.index).cmp(&pane_index_num(&a.index)))); }
    sorted.into_iter().next()
}

fn pane_index_num(value: &str) -> i64 { value.parse::<i64>().unwrap_or(0) }

fn pane_resolve_pane_id(value: &str, rows: &[PaneRow]) -> Option<PaneRow> {
    rows.iter().find(|row| row.pane_id == value).cloned().or_else(|| Some(PaneRow { index: String::new(), pane_id: value.to_owned(), title: value.to_owned(), top: 0 }))
}

fn pane_run_swap<R: maw_tmux::TmuxRunner>(source: &str, target: &str, runner: &mut R) -> Result<(), String> {
    let args = vec!["-s".to_owned(), source.to_owned(), "-t".to_owned(), target.to_owned()];
    runner.run("swap-pane", &args).map(|_| ()).map_err(|error| error.message)
}

fn pane_render_swap_success(source: &PaneRow, target: &PaneRow) -> String {
    let left = pane_display_name(source);
    let right = pane_display_name(target);
    format!("\x1b[32m✓\x1b[0m swapped {left} ↔ {right}\n")
}

fn pane_display_name(row: &PaneRow) -> &str { if row.title.is_empty() { &row.pane_id } else { &row.title } }

fn pane_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod pane_tests {
    use super::*;

    fn pane_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn pane_parser_rejects_leading_dash_targets() {
        let err = pane_parse_args(&pane_strings(&["swap", "-t", "1"])).unwrap_err();
        assert_eq!(err.1, "pane: invalid pane target \"-t\"");
    }

    #[test]
    fn pane_resolver_matches_indices_titles_and_edges() {
        let rows = vec![
            PaneRow { index: "0".to_owned(), pane_id: "%1".to_owned(), title: "lead".to_owned(), top: 20 },
            PaneRow { index: "1".to_owned(), pane_id: "%2".to_owned(), title: "tile-1".to_owned(), top: 40 },
            PaneRow { index: "2".to_owned(), pane_id: "%3".to_owned(), title: "tile-2".to_owned(), top: 10 },
        ];
        assert_eq!(pane_resolve("0", &rows).unwrap().pane_id, "%1");
        assert_eq!(pane_resolve("tile", &rows).unwrap().pane_id, "%2");
        assert_eq!(pane_resolve("top", &rows).unwrap().pane_id, "%3");
        assert_eq!(pane_resolve("bottom", &rows).unwrap().pane_id, "%2");
        assert_eq!(pane_resolve("%99", &rows).unwrap().pane_id, "%99");
    }
}
