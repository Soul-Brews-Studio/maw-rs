#[derive(Clone, Copy)]
pub(crate) struct TmuxSubcommandEntry {
    names: &'static [&'static str],
    handler: NativeHandler,
}

const TMUX_BUILTIN_SUBS: &[TmuxSubcommandEntry] = &[
    TmuxSubcommandEntry { names: &["ls", "list"], handler: run_tmux_ls },
    TmuxSubcommandEntry { names: &["peek"], handler: run_tmux_peek },
    TmuxSubcommandEntry { names: &["split"], handler: run_tmux_split },
    TmuxSubcommandEntry { names: &["attach"], handler: run_attach_plan },
];

fn run_tmux_command(argv: &[String]) -> CliOutput {
    let Some(sub) = argv.first().map(String::as_str) else { return tmux_usage(); };
    if matches!(sub, "--help" | "-h") {
        return tmux_usage();
    }
    tmux_subcommand_entries()
        .find(|entry| entry.names.contains(&sub))
        .map_or_else(
            || CliOutput { code: 1, stdout: String::new(), stderr: format!("maw tmux: unknown subcommand {sub}\n") },
            |entry| (entry.handler)(&argv[1..]),
        )
}

fn tmux_subcommand_entries() -> impl Iterator<Item = &'static TmuxSubcommandEntry> {
    TMUX_BUILTIN_SUBS
        .iter()
        .chain(TMUX_SUB_FRAGMENTS.iter().copied().flatten())
}

fn tmux_usage() -> CliOutput { CliOutput { code: 0, stdout: "usage: maw tmux <ls|peek|split|attach|break> [...]\n".to_owned(), stderr: String::new() } }

fn run_tmux_ls(argv: &[String]) -> CliOutput {
    let json = argv.iter().any(|arg| arg == "--json");
    let mut client = TmuxClient::local();
    let sessions = client.list_all();
    if json { return CliOutput { code: 0, stdout: serde_json::to_string(&sessions.iter().map(|s| serde_json::json!({"name": s.name, "windows": s.windows.iter().map(|w| serde_json::json!({"index": w.index, "name": w.name, "active": w.active})).collect::<Vec<_>>() })).collect::<Vec<_>>()).unwrap_or_else(|_| "[]".to_owned()) + "\n", stderr: String::new() }; }
    let mut stdout = String::new();
    for session in sessions { let _ = writeln!(stdout, "{}", session.name); for window in session.windows { let _ = writeln!(stdout, "  {}:{}{}", window.index, window.name, if window.active { " *" } else { "" }); } }
    CliOutput { code: 0, stdout, stderr: String::new() }
}
fn run_tmux_peek(argv: &[String]) -> CliOutput {
    let Some(target) = argv.iter().find(|arg| !arg.starts_with('-')) else { return CliOutput { code: 2, stdout: String::new(), stderr: "usage: maw tmux peek <target> [--lines N]\n".to_owned() }; };
    let lines = flag_value(argv, "--lines").and_then(|v| v.parse::<u32>().ok()).unwrap_or(30);
    let mut client = TmuxClient::local();
    match client.capture(target, Some(lines)) { Ok(out) => CliOutput { code: 0, stdout: out, stderr: String::new() }, Err(error) => command_target_error("tmux peek", &error.message) }
}
fn run_tmux_split(argv: &[String]) -> CliOutput {
    let Some(target) = argv.iter().find(|arg| !arg.starts_with('-')) else { return CliOutput { code: 2, stdout: String::new(), stderr: "usage: maw tmux split <target> [-v|--vertical] [--pct N] [--cmd <cmd>] [--dry-run]\n".to_owned() }; };
    let vertical = argv.iter().any(|arg| matches!(arg.as_str(), "-v" | "--vertical"));
    let pct = flag_value(argv, "--pct").and_then(|v| v.parse::<f64>().ok()).unwrap_or(50.0);
    let command = flag_value(argv, "--cmd");
    if argv.iter().any(|arg| arg == "--dry-run") { return CliOutput { code: 0, stdout: format!("tmux split-window {} -l {}% -t {}{}\n", if vertical { "-v" } else { "-h" }, pct, target, command.as_ref().map(|c| format!(" -- {c}")).unwrap_or_default()), stderr: String::new() }; }
    let mut client = TmuxClient::local();
    let options = maw_tmux::TmuxSplitActionOptions { vertical, pct, command };
    match client.split_pane_action(target, &options) { Ok(()) => CliOutput { code: 0, stdout: format!("split → {target}\n"), stderr: String::new() }, Err(error) => command_target_error("tmux split", &error.message) }
}
