const DISPATCH_52: &[DispatcherEntry] = &[DispatcherEntry {
    command: "mega",
    handler: Handler::Sync(run_mega_command),
}];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MegaSubcommand {
    Help,
    Ls,
    Status,
    Stop,
    Kill,
    Tree,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MegaOptions {
    subcommand: MegaSubcommand,
    team_lead: bool,
    yes: bool,
    targets: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MegaWindow {
    index: i32,
    name: String,
    active: bool,
    panes: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MegaSessionStatus {
    name: String,
    windows: Vec<MegaWindow>,
    fleet_windows: Vec<String>,
}

fn run_mega_command(argv: &[String]) -> CliOutput {
    match mega_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn mega_run_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, String> {
    let options = mega_parse_args(argv)?;
    if options.subcommand == MegaSubcommand::Help {
        return Ok(mega_usage_text());
    }

    match options.subcommand {
        MegaSubcommand::Help => Ok(mega_usage_text()),
        MegaSubcommand::Ls => Ok(mega_render_ls(&options)),
        MegaSubcommand::Status => mega_render_status(&options, runner),
        MegaSubcommand::Tree => mega_render_tree(&options, runner),
        MegaSubcommand::Stop => mega_stop_or_kill(&options, runner, "stop"),
        MegaSubcommand::Kill => mega_stop_or_kill(&options, runner, "kill"),
    }
}

fn mega_usage_text() -> String {
    concat!(
        "usage: maw mega <ls|status|stop|kill|tree> [--team-lead] [--yes] [target...]\n",
        "\n",
        "Native mega overview/control for fleet tmux sessions.\n",
        "\n",
        "Subcommands:\n",
        "  ls                  list configured fleet sessions\n",
        "  status              show live tmux status for targets\n",
        "  tree                show session → window tree\n",
        "  stop                stop target sessions (requires --yes)\n",
        "  kill                alias for stop/kill-session (requires --yes)\n",
        "\n",
        "Team-lead variants: add --team-lead/--lead or prefix with team-lead.\n"
    )
    .to_owned()
}

fn mega_parse_args(argv: &[String]) -> Result<MegaOptions, String> {
    let mut subcommand = None::<MegaSubcommand>;
    let mut team_lead = false;
    let mut yes = false;
    let mut targets = Vec::<String>::new();

    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" | "help" => subcommand = Some(MegaSubcommand::Help),
            "ls" | "list" => subcommand = Some(MegaSubcommand::Ls),
            "status" | "stat" => subcommand = Some(MegaSubcommand::Status),
            "tree" => subcommand = Some(MegaSubcommand::Tree),
            "stop" => subcommand = Some(MegaSubcommand::Stop),
            "kill" => subcommand = Some(MegaSubcommand::Kill),
            "team-lead" | "teamlead" | "lead" | "tl" | "--team-lead" | "--teamlead" | "--lead" => {
                team_lead = true;
            }
            "team-lead-ls" | "lead-ls" | "tl-ls" => {
                team_lead = true;
                subcommand = Some(MegaSubcommand::Ls);
            }
            "team-lead-status" | "lead-status" | "tl-status" => {
                team_lead = true;
                subcommand = Some(MegaSubcommand::Status);
            }
            "team-lead-stop" | "lead-stop" | "tl-stop" => {
                team_lead = true;
                subcommand = Some(MegaSubcommand::Stop);
            }
            "team-lead-kill" | "lead-kill" | "tl-kill" => {
                team_lead = true;
                subcommand = Some(MegaSubcommand::Kill);
            }
            "team-lead-tree" | "lead-tree" | "tl-tree" => {
                team_lead = true;
                subcommand = Some(MegaSubcommand::Tree);
            }
            "--yes" | "-y" => yes = true,
            value if value.starts_with('-') => return Err(format!("mega: unknown argument {value}")),
            value => {
                mega_validate_target_arg(value, "target")?;
                targets.push(value.to_owned());
            }
        }
    }

    Ok(MegaOptions {
        subcommand: subcommand.unwrap_or(MegaSubcommand::Ls),
        team_lead,
        yes,
        targets,
    })
}

fn mega_render_ls(options: &MegaOptions) -> String {
    let sessions = mega_target_fleet_sessions(options);
    if sessions.is_empty() {
        return "mega: no configured fleet sessions\n".to_owned();
    }
    let mut out = String::from("\x1b[36mmega fleet\x1b[0m\n");
    for session in sessions {
        let marker = if mega_is_team_lead_session(&session) { " lead" } else { "" };
        let _ = writeln!(
            out,
            "  {}{}  {} window{}",
            session.name,
            marker,
            session.windows.len(),
            if session.windows.len() == 1 { "" } else { "s" }
        );
    }
    out
}

fn mega_render_status<R: maw_tmux::TmuxRunner>(
    options: &MegaOptions,
    runner: &mut R,
) -> Result<String, String> {
    let statuses = mega_statuses(options, runner)?;
    if statuses.is_empty() {
        return Ok("mega: no matching sessions\n".to_owned());
    }
    let mut out = String::from("\x1b[36mmega status\x1b[0m\n");
    for status in statuses {
        let live = if status.windows.is_empty() { "down" } else { "live" };
        let _ = writeln!(
            out,
            "  {}  {}  {} live / {} configured window{}",
            status.name,
            live,
            status.windows.len(),
            status.fleet_windows.len(),
            if status.fleet_windows.len() == 1 { "" } else { "s" }
        );
    }
    Ok(out)
}

fn mega_render_tree<R: maw_tmux::TmuxRunner>(
    options: &MegaOptions,
    runner: &mut R,
) -> Result<String, String> {
    let statuses = mega_statuses(options, runner)?;
    if statuses.is_empty() {
        return Ok("mega: no matching sessions\n".to_owned());
    }
    let mut out = String::from("\x1b[36mmega tree\x1b[0m\n");
    for status in statuses {
        let _ = writeln!(out, "{}", status.name);
        if status.windows.is_empty() {
            out.push_str("  \x1b[90m(down)\x1b[0m\n");
        } else {
            for window in status.windows {
                let active = if window.active { " *" } else { "" };
                let _ = writeln!(
                    out,
                    "  ├─ {}:{}{}  {} pane{}",
                    window.index,
                    window.name,
                    active,
                    window.panes,
                    if window.panes == 1 { "" } else { "s" }
                );
            }
        }
    }
    Ok(out)
}

fn mega_stop_or_kill<R: maw_tmux::TmuxRunner>(
    options: &MegaOptions,
    runner: &mut R,
    verb: &str,
) -> Result<String, String> {
    if !options.yes {
        return Err(format!("mega: refusing to {verb} sessions without --yes"));
    }
    let sessions = mega_target_session_names(options);
    if sessions.is_empty() {
        return Ok("mega: no matching sessions\n".to_owned());
    }
    let mut out = format!("\x1b[36mmega {verb}\x1b[0m\n");
    for session in sessions {
        mega_validate_tmux_target(&session)?;
        match runner.run("kill-session", &["-t".to_owned(), session.clone()]) {
            Ok(_) => {
                let _ = writeln!(out, "  \x1b[32m✓\x1b[0m {session}");
            }
            Err(error) => {
                let _ = writeln!(out, "  \x1b[33m⚠\x1b[0m {session}: {}", error.message);
            }
        }
    }
    Ok(out)
}

fn mega_statuses<R: maw_tmux::TmuxRunner>(
    options: &MegaOptions,
    runner: &mut R,
) -> Result<Vec<MegaSessionStatus>, String> {
    let sessions = mega_target_fleet_sessions(options);
    let mut statuses = Vec::with_capacity(sessions.len());
    for session in sessions {
        mega_validate_tmux_target(&session.name)?;
        let windows = match runner.run(
            "list-windows",
            &[
                "-t".to_owned(),
                session.name.clone(),
                "-F".to_owned(),
                "#{window_index}\t#{window_name}\t#{window_active}\t#{window_panes}".to_owned(),
            ],
        ) {
            Ok(raw) => mega_parse_windows(&raw),
            Err(_) => Vec::new(),
        };
        statuses.push(MegaSessionStatus {
            name: session.name,
            windows,
            fleet_windows: session.windows.into_iter().map(|window| window.name).collect(),
        });
    }
    Ok(statuses)
}

fn mega_target_session_names(options: &MegaOptions) -> Vec<String> {
    mega_target_fleet_sessions(options)
        .into_iter()
        .map(|session| session.name)
        .collect()
}

fn mega_target_fleet_sessions(options: &MegaOptions) -> Vec<NativeFleetSession> {
    let mut sessions = load_native_fleet();
    if options.team_lead {
        sessions.retain(mega_is_team_lead_session);
    }
    if !options.targets.is_empty() {
        let targets = options.targets.iter().map(String::as_str).collect::<Vec<_>>();
        sessions.retain(|session| mega_matches_any_target(&session.name, &targets));
    }
    sessions.sort_by(|left, right| left.name.cmp(&right.name));
    sessions
}

fn mega_matches_any_target(session: &str, targets: &[&str]) -> bool {
    targets.iter().any(|target| {
        session == *target
            || mega_oracle_name(session) == *target
            || session.ends_with(&format!("-{target}"))
            || session.contains(&format!("-{target}-"))
    })
}

fn mega_is_team_lead_session(session: &NativeFleetSession) -> bool {
    let name = session.name.to_ascii_lowercase();
    name.contains("team-lead")
        || name.contains("teamlead")
        || session
            .windows
            .iter()
            .any(|window| window.name.to_ascii_lowercase().contains("team-lead"))
}

fn mega_oracle_name(session_name: &str) -> &str {
    session_name
        .split_once('-')
        .filter(|(prefix, suffix)| prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.is_empty())
        .map_or(session_name, |(_, suffix)| suffix)
}

fn mega_parse_windows(raw: &str) -> Vec<MegaWindow> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.split('\t');
            MegaWindow {
                index: parts.next().and_then(|value| value.parse().ok()).unwrap_or(0),
                name: parts.next().unwrap_or_default().to_owned(),
                active: parts.next() == Some("1"),
                panes: parts.next().and_then(|value| value.parse().ok()).unwrap_or(0),
            }
        })
        .collect()
}

fn mega_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains('\0')
        || value.contains("..")
    {
        Err(format!(
            "mega: {label} must be non-empty, unpadded, not start with '-', and not contain '..'"
        ))
    } else {
        Ok(())
    }
}

fn mega_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        Err("mega: tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod mega_tests {
    use super::*;

    fn mega_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn mega_parse_real_subcommands_and_team_lead_variants() {
        let parsed = mega_parse_args(&mega_strings(&["team-lead-status", "alpha"])).expect("parse");
        assert_eq!(parsed.subcommand, MegaSubcommand::Status);
        assert!(parsed.team_lead);
        assert_eq!(parsed.targets, mega_strings(&["alpha"]));

        let parsed = mega_parse_args(&mega_strings(&["tl", "tree"])).expect("parse");
        assert_eq!(parsed.subcommand, MegaSubcommand::Tree);
        assert!(parsed.team_lead);
    }

    #[test]
    fn mega_parse_windows_matches_tmux_format() {
        let windows = mega_parse_windows("0\talpha-main\t1\t1\n1\talpha-team-lead\t0\t2\n");
        assert_eq!(
            windows,
            vec![
                MegaWindow { index: 0, name: "alpha-main".to_owned(), active: true, panes: 1 },
                MegaWindow { index: 1, name: "alpha-team-lead".to_owned(), active: false, panes: 2 },
            ]
        );
    }

    #[test]
    fn mega_stop_refuses_before_reading_environment_or_calling_tmux() {
        let mut runner = maw_tmux::CommandTmuxRunner::with_program("/definitely/not/a/tmux");
        let error = mega_run_with_runner(&mega_strings(&["stop", "alpha"]), &mut runner)
            .expect_err("requires yes before config/tmux IO");
        assert_eq!(error, "mega: refusing to stop sessions without --yes");
    }

    #[test]
    fn mega_option_injection_guard_blocks_exec_targets() {
        let error = mega_parse_args(&mega_strings(&["status", "-Sbad"])).expect_err("guard");
        assert!(error.contains("unknown argument -Sbad"), "{error}");
        assert!(mega_validate_tmux_target("-Sbad").is_err());
        assert!(mega_validate_target_arg("../bad", "target").is_err());
    }

    #[test]
    fn mega_dispatcher_is_native() {
        assert_eq!(dispatcher_status("mega"), DispatchKind::Native);
    }
}
