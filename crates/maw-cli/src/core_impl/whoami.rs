const DISPATCH_36: &[DispatcherEntry] = &[
    DispatcherEntry { command: "whoami", handler: Handler::Sync(run_whoami_command) },
];

fn run_whoami_command(argv: &[String]) -> CliOutput {
    let mut runner = maw_tmux::CommandTmuxRunner::new();
    run_whoami_command_with(argv, std::env::var_os("TMUX").is_some(), &mut runner)
}

fn run_whoami_command_with<R>(argv: &[String], in_tmux: bool, runner: &mut R) -> CliOutput
where
    R: maw_tmux::TmuxRunner,
{
    if !in_tmux {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: "maw whoami requires an active tmux session — run 'maw wake <oracle>' or attach to tmux first\n".to_owned(),
        };
    }

    let short = argv.iter().any(|arg| matches!(arg.as_str(), "--short" | "-s"));
    let json = argv.iter().any(|arg| arg == "--json");

    if short {
        return match display_message(runner, "#S") {
            Ok(raw) => CliOutput {
                code: 0,
                stdout: format!("{}\n", raw.trim()),
                stderr: String::new(),
            },
            Err(error) => whoami_tmux_error(&error.to_string()),
        };
    }

    match display_message(runner, "#S\t#W\t#{window_id}\t#{pane_title}\t#{pane_id}") {
        Ok(raw) => render_whoami_address(raw.trim(), json),
        Err(error) => whoami_tmux_error(&error.to_string()),
    }
}

fn display_message<R>(runner: &mut R, format: &str) -> Result<String, maw_tmux::TmuxError>
where
    R: maw_tmux::TmuxRunner,
{
    runner.run("display-message", &["-p".to_owned(), format.to_owned()])
}

fn render_whoami_address(raw: &str, json: bool) -> CliOutput {
    let parts = raw.split('\t').collect::<Vec<_>>();
    let session = parts.first().copied().unwrap_or_default();
    let window = parts.get(1).copied().unwrap_or_default();
    let window_id = parts.get(2).copied().unwrap_or_default();
    let pane_title = parts.get(3).copied().unwrap_or_default();
    let pane_id = parts.get(4).copied().unwrap_or_default();

    CliOutput {
        code: 0,
        stdout: if json {
            format!(
                "{{\"session\":{},\"window\":{},\"window_id\":{},\"pane_title\":{},\"pane_id\":{},\"target\":{}}}\n",
                json_string(session),
                json_string(window),
                json_string(window_id),
                json_string(pane_title),
                json_string(pane_id),
                json_string(&format!("{session}:{window}.{}", pane_id.trim_start_matches('%')))
            )
        } else {
            format!(
                "session  {session}\nwindow   {window}  \u{1b}[90m({window_id})\u{1b}[0m\npane     {pane_title}  \u{1b}[90m({pane_id})\u{1b}[0m\ntarget   \u{1b}[36m{session}:{window}\u{1b}[0m  (or {pane_id} for the exact pane)\n"
            )
        },
        stderr: String::new(),
    }
}

fn whoami_tmux_error(message: &str) -> CliOutput {
    CliOutput {
        code: 1,
        stdout: String::new(),
        stderr: format!("whoami: tmux display-message failed: {message}\n"),
    }
}
