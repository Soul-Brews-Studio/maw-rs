const DISPATCH_134: &[DispatcherEntry] = &[DispatcherEntry {
    command: "peek",
    handler: Handler::Sync(peek_run_command),
}];

const PEEK_USAGE: &str = "usage: maw peek <tmux-target> [--lines N] [--history]\n       maw peek [--lines N]\n\nNote: peek is local-only; remote agents should be checked with `maw hey <agent> pong`.\n";

#[derive(Debug, Clone, PartialEq, Eq)]
struct PeekOptions {
    target: Option<String>,
    lines: u32,
    history: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PeekWindow {
    session: String,
    index: String,
    name: String,
    active: bool,
}

fn peek_run_command(argv: &[String]) -> CliOutput {
    match peek_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) => output,
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn peek_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, (i32, String)> {
    peek_with_runner_and_config(argv, runner, &load_hey_config().route)
}

fn peek_with_runner_and_config<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
    config: &RouteConfig,
) -> Result<CliOutput, (i32, String)> {
    let options = peek_parse(argv)?;
    if let Some(target) = options.target.as_deref() {
        peek_validate_tmux_target(target).map_err(|message| (1, message))?;
        if let Some(message) = peek_remote_unknown_message(target, config) {
            return Ok(CliOutput {
                code: 0,
                stdout: message,
                stderr: String::new(),
            });
        }
        let content = peek_capture(runner, target, options.lines, options.history)
            .map_err(|message| (1, message))?;
        return Ok(CliOutput {
            code: 0,
            stdout: format!("\x1b[36m--- {target} ---\x1b[0m\n{content}"),
            stderr: String::new(),
        });
    }

    let windows = peek_list_windows(runner).map_err(|message| (1, message))?;
    Ok(CliOutput {
        code: 0,
        stdout: peek_render_overview(runner, &windows)?,
        stderr: String::new(),
    })
}

fn peek_remote_unknown_message(query: &str, config: &RouteConfig) -> Option<String> {
    let (node, target) = peek_remote_route(query, config)?;
    Some(format!(
        "\x1b[36m--- {query} ---\x1b[0m\nremote/unknown: target is on node '{node}', but `maw peek` is local-only and cannot inspect remote tmux.\nUse `maw hey {target} pong` to verify the remote agent is alive.\n"
    ))
}

fn peek_remote_route(query: &str, config: &RouteConfig) -> Option<(String, String)> {
    match resolve_route_target(query, config, &[]) {
        RouteResult::Peer { node, target, .. } => Some((node, target)),
        RouteResult::Error { .. } => peek_remote_route_from_agents_map(query, config),
        RouteResult::Local { .. } | RouteResult::SelfNode { .. } => None,
    }
}

fn peek_remote_route_from_agents_map(query: &str, config: &RouteConfig) -> Option<(String, String)> {
    let self_node = config.node.as_deref().unwrap_or("local");
    let node = config
        .agents
        .get(query)
        .or_else(|| query.strip_suffix("-oracle").and_then(|name| config.agents.get(name)))?;
    if node == self_node || node == "local" {
        return None;
    }
    Some((node.clone(), query.to_owned()))
}

fn peek_parse(argv: &[String]) -> Result<PeekOptions, (i32, String)> {
    let mut lines = 30_u32;
    let mut history = false;
    let mut positionals = Vec::new();
    let mut iter = argv.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, PEEK_USAGE.trim_end().to_owned())),
            "--history" => history = true,
            "--lines" => {
                let Some(value) = iter.next() else { return Err((2, "peek: --lines requires a positive number".to_owned())); };
                lines = peek_parse_lines(value)?;
            }
            value if value.starts_with("--lines=") => lines = peek_parse_lines(&value[8..])?,
            "--" => return Err((2, "peek: -- separator is not supported".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("peek: unknown flag '{value}'"))),
            value => positionals.push(value.to_owned()),
        }
    }
    if positionals.len() > 1 {
        return Err((2, "peek: expected at most one tmux target".to_owned()));
    }
    Ok(PeekOptions {
        target: positionals.pop(),
        lines,
        history,
    })
}

fn peek_parse_lines(value: &str) -> Result<u32, (i32, String)> {
    if value.is_empty() || value.starts_with('-') || value == "--" {
        return Err((2, "peek: --lines requires a positive number".to_owned()));
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|lines| *lines > 0)
        .ok_or_else(|| (2, "peek: --lines requires a positive number".to_owned()))
}

fn peek_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') || target == "--" {
        return Err("peek target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if target.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) {
        return Err("peek target must not contain whitespace, NUL, or control characters".to_owned());
    }
    Ok(())
}

fn peek_capture<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
    lines: u32,
    history: bool,
) -> Result<String, String> {
    let start = if history { "-".to_owned() } else { format!("-{lines}") };
    runner
        .run(
            "capture-pane",
            &[
                "-p".to_owned(),
                "-t".to_owned(),
                target.to_owned(),
                "-S".to_owned(),
                start,
                "-J".to_owned(),
            ],
        )
        .map_err(|error| format!("peek capture failed for '{target}': {}", error.message))
}

fn peek_list_windows<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<Vec<PeekWindow>, String> {
    let raw = runner
        .run(
            "list-windows",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}\t#{window_index}\t#{window_name}\t#{window_active}".to_owned(),
            ],
        )
        .map_err(|error| format!("peek list-windows failed: {}", error.message))?;
    Ok(peek_parse_windows(&raw))
}

fn peek_parse_windows(raw: &str) -> Vec<PeekWindow> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let mut parts = line.splitn(4, '\t');
            let session = parts.next()?.to_owned();
            let index = parts.next()?.to_owned();
            let name = parts.next()?.to_owned();
            let active = parts.next() == Some("1");
            Some(PeekWindow {
                session,
                index,
                name,
                active,
            })
        })
        .collect()
}

fn peek_render_overview<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    windows: &[PeekWindow],
) -> Result<String, (i32, String)> {
    let mut stdout = String::new();
    for window in windows {
        let target = format!("{}:{}", window.session, window.index);
        peek_validate_tmux_target(&target).map_err(|message| (1, message))?;
        let summary = match peek_capture(runner, &target, 3, false) {
            Ok(content) => peek_last_nonempty_line(&content).unwrap_or_else(|| "(empty)".to_owned()),
            Err(_) => "(unreachable)".to_owned(),
        };
        let dot = if window.active { "\x1b[32m*\x1b[0m" } else { " " };
        let _ = writeln!(
            stdout,
            "{dot} \x1b[36m{:<22}\x1b[0m {}",
            window.name,
            peek_truncate_chars(&summary, 80)
        );
    }
    Ok(stdout)
}

fn peek_last_nonempty_line(content: &str) -> Option<String> {
    content
        .lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .map(ToOwned::to_owned)
}

fn peek_truncate_chars(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod peek_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct PeekFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        list: String,
        captures: std::collections::BTreeMap<String, String>,
    }

    impl maw_tmux::TmuxRunner for PeekFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-windows" => Ok(self.list.clone()),
                "capture-pane" => {
                    let target = args
                        .windows(2)
                        .find(|pair| pair[0] == "-t")
                        .map(|pair| pair[1].clone())
                        .unwrap_or_default();
                    self.captures
                        .get(&target)
                        .cloned()
                        .ok_or_else(|| maw_tmux::TmuxError::new("no pane"))
                }
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn peek_dispatch_is_native() {
        assert_eq!(DISPATCH_134[0].command, "peek");
        assert_eq!(dispatcher_status("peek"), DispatchKind::Native);
    }

    #[test]
    fn peek_single_target_uses_argv_vector_capture() {
        let mut runner = PeekFakeRunner::default();
        runner.captures.insert("sess:1.0".to_owned(), "pane output\n".to_owned());

        let output = peek_with_runner(&args(&["sess:1.0", "--lines", "12"]), &mut runner).expect("peek");

        assert_eq!(output.stdout, "\x1b[36m--- sess:1.0 ---\x1b[0m\npane output\n");
        assert_eq!(
            runner.calls[0],
            (
                "capture-pane".to_owned(),
                args(&["-p", "-t", "sess:1.0", "-S", "-12", "-J"]),
            )
        );
    }

    #[test]
    fn peek_remote_node_target_reports_unknown_without_tmux_capture() {
        let mut runner = PeekFakeRunner::default();
        let config = RouteConfig {
            node: Some("local-node".to_owned()),
            named_peers: vec![RouteNamedPeer {
                name: "remote-node".to_owned(),
                url: "http://remote.invalid:31745".to_owned(),
            }],
            peers: Vec::new(),
            agents: std::collections::HashMap::new(),
        };

        let output = peek_with_runner_and_config(
            &args(&["remote-node:nova", "--lines", "12"]),
            &mut runner,
            &config,
        )
        .expect("remote unknown");

        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("remote/unknown"));
        assert!(output.stdout.contains("maw peek` is local-only"));
        assert!(output.stdout.contains("maw hey nova pong"));
        assert!(runner.calls.is_empty(), "remote target must not call local tmux");
    }

    #[test]
    fn peek_remote_agents_map_without_peer_url_reports_unknown_without_tmux_capture() {
        let mut runner = PeekFakeRunner::default();
        let mut agents = std::collections::HashMap::new();
        agents.insert("nova".to_owned(), "remote-node".to_owned());
        let config = RouteConfig {
            node: Some("local-node".to_owned()),
            named_peers: Vec::new(),
            peers: Vec::new(),
            agents,
        };

        let output = peek_with_runner_and_config(&args(&["nova"]), &mut runner, &config)
            .expect("remote unknown");

        assert!(output.stdout.contains("remote/unknown"));
        assert!(output.stdout.contains("node 'remote-node'"));
        assert!(output.stdout.contains("maw hey nova pong"));
        assert!(runner.calls.is_empty(), "remote target must not call local tmux");
    }

    #[test]
    fn peek_history_uses_full_capture_and_rejects_injection_before_tmux() {
        let mut runner = PeekFakeRunner::default();
        let error = peek_with_runner(&args(&["-bad"]), &mut runner).expect_err("flag target rejected");
        assert_eq!(error.0, 2);
        assert!(runner.calls.is_empty());

        let error = peek_with_runner(&args(&["bad\npane"]), &mut runner).expect_err("control target rejected");
        assert_eq!(error.0, 1);
        assert!(runner.calls.is_empty());

        runner.captures.insert("%9".to_owned(), "history\n".to_owned());
        let _ = peek_with_runner(&args(&["%9", "--history"]), &mut runner).expect("history");
        assert_eq!(runner.calls[0].1, args(&["-p", "-t", "%9", "-S", "-", "-J"]));
    }

    #[test]
    fn peek_overview_lists_windows_and_summarizes_three_line_captures() {
        let mut runner = PeekFakeRunner {
            list: "s\t0\tactive\t1\ns\t1\tdead\t0\n".to_owned(),
            ..PeekFakeRunner::default()
        };
        runner.captures.insert("s:0".to_owned(), "old\nlast line\n".to_owned());

        let output = peek_with_runner(&args(&[]), &mut runner).expect("overview");

        assert!(output.stdout.contains("active"));
        assert!(output.stdout.contains("last line"));
        assert!(output.stdout.contains("dead"));
        assert!(output.stdout.contains("(unreachable)"));
        assert_eq!(runner.calls[1].1, args(&["-p", "-t", "s:0", "-S", "-3", "-J"]));
    }
}
