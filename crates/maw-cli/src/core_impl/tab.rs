const DISPATCH_43: &[DispatcherEntry] = &[
    DispatcherEntry { command: "tab", handler: Handler::Sync(run_tab_command) },
];

#[allow(non_camel_case_types)]
type tab_Window = (i32, String, bool);

fn run_tab_command(argv: &[String]) -> CliOutput {
    match tab_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) => output,
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn tab_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, (i32, String)> {
    let session = tab_current_session(runner)?;
    tab_validate_tmux_target(&session).map_err(|message| (1, message))?;
    let tab_num = argv.first().and_then(|value| parse_js_i32_prefix(value));

    if tab_num.is_none() {
        let tabs = tab_list_windows(runner, &session)?;
        return Ok(CliOutput {
            code: 0,
            stdout: tab_render_list(&session, &tabs),
            stderr: String::new(),
        });
    }

    let tab_num = tab_num.expect("checked above");
    let tabs = tab_list_windows(runner, &session)?;
    let Some(tab) = tabs.iter().find(|tab| tab.0 == tab_num) else {
        return Err((
            1,
            format!(
                "available: {}\ntab {tab_num} not found in session {session}",
                tabs.iter()
                    .map(|tab| tab.0.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    };
    tab_validate_tmux_target(tab.1.as_str()).map_err(|message| (1, message))?;

    let remaining = argv
        .iter()
        .skip(1)
        .filter(|arg| !matches!(arg.as_str(), "--force" | "--talk"))
        .cloned()
        .collect::<Vec<_>>();
    let force = argv.iter().any(|arg| arg == "--force");
    let talk = argv.iter().any(|arg| arg == "--talk");

    if remaining.is_empty() {
        return tab_capture_target(runner, tab.1.as_str()).map(|content| CliOutput {
            code: 0,
            stdout: format!("\x1b[36m--- {} ---\x1b[0m\n{content}", tab.1),
            stderr: String::new(),
        });
    }

    let message = remaining.join(" ");
    tab_send_message(runner, tab.1.as_str(), &message, force, talk)
}

fn tab_current_session<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, (i32, String)> {
    runner
        .run(
            "display-message",
            &["-p".to_owned(), "#S".to_owned()],
        )
        .map(|session| session.trim().to_owned())
        .map_err(|_| (1, "not inside a tmux session".to_owned()))
}

fn tab_list_windows<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    session: &str,
) -> Result<Vec<tab_Window>, (i32, String)> {
    runner
        .run(
            "list-windows",
            &[
                "-t".to_owned(),
                session.to_owned(),
                "-F".to_owned(),
                "#{window_index}:#{window_name}:#{window_active}".to_owned(),
            ],
        )
        .map(|raw| tab_parse_windows(&raw))
        .map_err(|error| (1, format!("tab: {}", error.message)))
}

fn tab_parse_windows(raw: &str) -> Vec<tab_Window> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let mut parts = line.splitn(3, ':');
(
                parts.next().and_then(|value| value.parse().ok()).unwrap_or(0),
                parts.next().unwrap_or_default().to_owned(),
                parts.next() == Some("1"),
            )
        })
        .collect()
}

fn tab_render_list(session: &str, tabs: &[tab_Window]) -> String {
    let mut stdout = format!("\x1b[36m{session}\x1b[0m tabs:\n");
    for tab in tabs {
        let marker = if tab.2 { " \x1b[32m← you are here\x1b[0m" } else { "" };
        let _ = writeln!(stdout, "  {}: {}{marker}", tab.0, tab.1);
    }
    stdout
}

fn tab_capture_target<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
) -> Result<String, (i32, String)> {
    runner
        .run(
            "capture-pane",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-e".to_owned(),
                "-p".to_owned(),
                "-S".to_owned(),
                "-80".to_owned(),
            ],
        )
        .map_err(|error| (1, format!("tab: {}", error.message)))
}

fn tab_send_message<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
    message: &str,
    force: bool,
    talk: bool,
) -> Result<CliOutput, (i32, String)> {
    if !force {
        let command = runner
            .run(
                "list-panes",
                &[
                    "-t".to_owned(),
                    target.to_owned(),
                    "-F".to_owned(),
                    "#{pane_current_command}".to_owned(),
                ],
            )
            .map_err(|error| (1, format!("tab: {}", error.message)))?;
        if !tab_is_agent_command(command.lines().next().unwrap_or_default()) {
            return Err((
                1,
                format!("no active Claude session in {target} (use --force)"),
            ));
        }
    }

    runner
        .run(
            "send-keys",
            &maw_tmux::tmux_send_keys_literal_args(target, message),
        )
        .map_err(|error| (1, format!("tab: {}", error.message)))?;
    runner
        .run("send-keys", &maw_tmux::tmux_send_enter_args(target))
        .map_err(|error| (1, format!("tab: {}", error.message)))?;
    let verb = if talk { "talk" } else { "sent" };
    Ok(CliOutput {
        code: 0,
        stdout: format!("\x1b[32m{verb}\x1b[0m → {target}: {message}\n"),
        stderr: String::new(),
    })
}

fn tab_is_agent_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains("claude") || command.contains("codex") || command.contains("node")
}

fn tab_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tab_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct MockTmuxRunner {
        calls: Vec<(String, Vec<String>)>,
        session: String,
        windows: String,
        pane_command: String,
        capture: String,
    }

    impl maw_tmux::TmuxRunner for MockTmuxRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" => Ok(self.session.clone()),
                "list-windows" => Ok(self.windows.clone()),
                "list-panes" => Ok(self.pane_command.clone()),
                "capture-pane" => Ok(self.capture.clone()),
                "send-keys" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn tab_list_matches_maw_js_output() {
        let mut runner = MockTmuxRunner {
            session: "03-neo\n".to_owned(),
            windows: "0:zsh:0\n1:work:1\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let output = tab_with_runner(&[], &mut runner).expect("tab list");

        assert_eq!(output.code, 0);
        assert_eq!(
            output.stdout,
            "\x1b[36m03-neo\x1b[0m tabs:\n  0: zsh\n  1: work \x1b[32m← you are here\x1b[0m\n"
        );
        assert_eq!(
            runner.calls,
            vec![
                ("display-message".to_owned(), strings(&["-p", "#S"])),
                (
                    "list-windows".to_owned(),
                    strings(&["-t", "03-neo", "-F", "#{window_index}:#{window_name}:#{window_active}"])
                ),
            ]
        );
    }

    #[test]
    fn tab_peek_uses_js_parse_int_and_window_name_target() {
        let mut runner = MockTmuxRunner {
            session: "neo\n".to_owned(),
            windows: "1:work:0\n".to_owned(),
            capture: "hello\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let output = tab_with_runner(&strings(&["1abc"]), &mut runner).expect("peek");

        assert_eq!(output.stdout, "\x1b[36m--- work ---\x1b[0m\nhello\n");
        assert_eq!(runner.calls[2].0, "capture-pane");
        assert_eq!(runner.calls[2].1[0..2], strings(&["-t", "work"]));
    }

    #[test]
    fn tab_missing_prints_available_indexes_then_error() {
        let mut runner = MockTmuxRunner {
            session: "neo\n".to_owned(),
            windows: "0:zsh:0\n2:work:0\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let error = tab_with_runner(&strings(&["1"]), &mut runner).expect_err("missing");

        assert_eq!(error, (1, "available: 0, 2\ntab 1 not found in session neo".to_owned()));
    }

    #[test]
    fn tab_send_guards_non_agent_unless_forced() {
        let mut runner = MockTmuxRunner {
            session: "neo\n".to_owned(),
            windows: "1:work:0\n".to_owned(),
            pane_command: "bash\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let error = tab_with_runner(&strings(&["1", "hello"]), &mut runner).expect_err("guard");

        assert_eq!(error, (1, "no active Claude session in work (use --force)".to_owned()));
        assert_eq!(runner.calls.len(), 3);
    }

    #[test]
    fn tab_send_force_skips_agent_guard_and_filters_flags() {
        let mut runner = MockTmuxRunner {
            session: "neo\n".to_owned(),
            windows: "1:work:0\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let output = tab_with_runner(&strings(&["1", "--force", "--talk", "hi", "there"]), &mut runner)
            .expect("send");

        assert_eq!(output.stdout, "\x1b[32mtalk\x1b[0m → work: hi there\n");
        assert_eq!(runner.calls[2], ("send-keys".to_owned(), strings(&["-t", "work", "-l", "hi there"])));
        assert_eq!(runner.calls[3], ("send-keys".to_owned(), strings(&["-t", "work", "Enter"])));
    }

    #[test]
    fn tab_rejects_leading_dash_session_before_target_use() {
        let mut runner = MockTmuxRunner {
            session: "-Sbad\n".to_owned(),
            windows: "0:zsh:0\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let error = tab_with_runner(&strings(&["0"]), &mut runner).expect_err("guard");

        assert!(error.1.contains("target/session"));
        assert_eq!(runner.calls.len(), 1, "guard before list-windows -t target");
    }

    #[test]
    fn tab_rejects_leading_dash_window_before_peek_or_send() {
        let mut runner = MockTmuxRunner {
            session: "neo\n".to_owned(),
            windows: "1:-bad:0\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let error = tab_with_runner(&strings(&["1", "msg"]), &mut runner).expect_err("guard");

        assert!(error.1.contains("target/session"));
        assert_eq!(runner.calls.len(), 2, "guard before -t window target");
    }
}
