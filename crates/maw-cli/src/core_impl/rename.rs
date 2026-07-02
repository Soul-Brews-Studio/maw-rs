const DISPATCH_38: &[DispatcherEntry] = &[
    DispatcherEntry { command: "rename", handler: Handler::Sync(run_rename_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct RenameWindow {
    index: i32,
    name: String,
}

fn run_rename_command(argv: &[String]) -> CliOutput {
    match rename_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(stderr) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{stderr}\n"),
        },
    }
}

fn rename_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, String> {
    let Some(target) = argv.first() else {
        return Err(rename_usage());
    };
    let Some(new_name) = argv.get(1) else {
        return Err(rename_usage());
    };

    let session = rename_tmux_run(runner, "display-message", &["-p", "#S"])?;
    validate_rename_tmux_target(&session)?;
    let windows = list_rename_windows(runner, &session)?;
    let Some(window) = find_rename_window(&windows, target) else {
        let tabs = windows
            .iter()
            .map(|window| format!("{}:{}", window.index, window.name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("tabs: {tabs}\ntab {target} not found in {session}"));
    };

    let full_name = rename_auto_prefix(&session, new_name);
    rename_tmux_run(
        runner,
        "rename-window",
        &["-t", &format!("{session}:{}", window.index), &full_name],
    )?;
    Ok(format!(
        "\x1b[32m✓\x1b[0m tab {} \x1b[33m{}\x1b[0m → \x1b[33m{}\x1b[0m\n",
        window.index, window.name, full_name
    ))
}

fn rename_usage() -> String {
    "usage: maw rename <tab# or name> <new-name>  (see: maw tab to list tabs)".to_owned()
}

fn rename_tmux_run<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    subcommand: &str,
    args: &[&str],
) -> Result<String, String> {
    runner
        .run(
            subcommand,
            &args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>(),
        )
        .map(|out| out.trim().to_owned())
        .map_err(|error| error.message)
}

fn list_rename_windows<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    session: &str,
) -> Result<Vec<RenameWindow>, String> {
    let raw = rename_tmux_run(runner, "list-windows", &["-t", session, "-F", "#I:#W"])?;
    Ok(parse_rename_windows(&raw))
}

fn parse_rename_windows(raw: &str) -> Vec<RenameWindow> {
    raw.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let index = line.find(':').unwrap_or(line.len());
            RenameWindow {
                index: line[..index].parse().unwrap_or(0),
                name: line.get(index + 1..).unwrap_or_default().to_owned(),
            }
        })
        .collect()
}

fn find_rename_window<'a>(windows: &'a [RenameWindow], target: &str) -> Option<&'a RenameWindow> {
    parse_js_i32_prefix(target)
        .and_then(|index| windows.iter().find(|window| window.index == index))
        .or_else(|| windows.iter().find(|window| window.name == target))
}

fn parse_js_i32_prefix(value: &str) -> Option<i32> {
    let trimmed = value.trim_start();
    let (sign, digits) = trimmed
        .strip_prefix('-')
        .map_or((1_i32, trimmed), |tail| (-1_i32, tail));
    let digits = digits
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<i32>().ok().and_then(|number| number.checked_mul(sign)))
        .flatten()
}

fn rename_auto_prefix(session: &str, new_name: &str) -> String {
    let oracle = session
        .split_once('-')
        .filter(|(prefix, _)| prefix.chars().all(|ch| ch.is_ascii_digit()))
        .map_or(session, |(_, suffix)| suffix);
    let prefix = format!("{oracle}-");
    if new_name.starts_with(&prefix) {
        new_name.to_owned()
    } else {
        format!("{prefix}{new_name}")
    }
}

fn validate_rename_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct MockTmuxRunner {
        calls: Vec<(String, Vec<String>)>,
        session: String,
        windows: String,
        fail_on_rename: Option<String>,
    }

    impl maw_tmux::TmuxRunner for MockTmuxRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "display-message" => Ok(self.session.clone()),
                "list-windows" => Ok(self.windows.clone()),
                "rename-window" => self
                    .fail_on_rename
                    .clone()
                    .map_or_else(|| Ok(String::new()), |message| Err(maw_tmux::TmuxError::new(message))),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn rename_auto_prefix_strips_numeric_fleet_slot_once() {
        assert_eq!(rename_auto_prefix("03-neo", "work"), "neo-work");
        assert_eq!(rename_auto_prefix("neo", "neo-work"), "neo-work");
        assert_eq!(rename_auto_prefix("dev-neo", "work"), "dev-neo-work");
    }

    #[test]
    fn rename_by_number_matches_maw_js_success_output_and_tmux_args() {
        let mut runner = MockTmuxRunner {
            session: "03-neo\n".to_owned(),
            windows: "0:zsh\n1:old\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let stdout = rename_with_runner(&strings(&["1abc", "work"]), &mut runner).expect("rename");

        assert_eq!(stdout, "\x1b[32m✓\x1b[0m tab 1 \x1b[33mold\x1b[0m → \x1b[33mneo-work\x1b[0m\n");
        assert_eq!(runner.calls[0], ("display-message".to_owned(), strings(&["-p", "#S"])));
        assert_eq!(runner.calls[1], ("list-windows".to_owned(), strings(&["-t", "03-neo", "-F", "#I:#W"])));
        assert_eq!(runner.calls[2], ("rename-window".to_owned(), strings(&["-t", "03-neo:1", "neo-work"])));
    }

    #[test]
    fn rename_by_name_does_not_double_prefix() {
        let mut runner = MockTmuxRunner {
            session: "neo\n".to_owned(),
            windows: "0:zsh\n2:old:name\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let stdout = rename_with_runner(&strings(&["old:name", "neo-done"]), &mut runner).expect("rename");

        assert_eq!(stdout, "\x1b[32m✓\x1b[0m tab 2 \x1b[33mold:name\x1b[0m → \x1b[33mneo-done\x1b[0m\n");
    }

    #[test]
    fn rename_missing_args_match_maw_js_usage() {
        assert_eq!(rename_with_runner(&[], &mut MockTmuxRunner::default()).expect_err("usage"), rename_usage());
        assert_eq!(rename_with_runner(&strings(&["1"]), &mut MockTmuxRunner::default()).expect_err("usage"), rename_usage());
    }

    #[test]
    fn rename_not_found_prints_tabs_then_error() {
        let mut runner = MockTmuxRunner {
            session: "03-neo\n".to_owned(),
            windows: "0:zsh\n1:old\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let error = rename_with_runner(&strings(&["missing", "work"]), &mut runner).expect_err("missing");

        assert_eq!(error, "tabs: 0:zsh, 1:old\ntab missing not found in 03-neo");
        assert_eq!(runner.calls.len(), 2, "must not rename when target is absent");
    }

    #[test]
    fn rename_rejects_leading_dash_session_before_target_use() {
        let mut runner = MockTmuxRunner {
            session: "-Sbad\n".to_owned(),
            windows: "0:zsh\n".to_owned(),
            ..MockTmuxRunner::default()
        };

        let error = rename_with_runner(&strings(&["0", "work"]), &mut runner).expect_err("guard");

        assert!(error.contains("target/session"), "{error}");
        assert_eq!(runner.calls.len(), 1, "guard before list-windows -t target");
    }

    #[test]
    fn rename_dispatcher_returns_stderr_for_usage() {
        let output = run_rename_command(&[]);

        assert_eq!(output.code, 1);
        assert_eq!(output.stdout, "");
        assert_eq!(output.stderr, format!("{}\n", rename_usage()));
    }
}
