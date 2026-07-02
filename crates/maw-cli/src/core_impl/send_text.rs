const DISPATCH_84: &[DispatcherEntry] = &[DispatcherEntry {
    command: "send-text",
    handler: Handler::Sync(sendtext_run_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
struct SendtextOptions {
    target: String,
    text: String,
}

fn sendtext_run_command(argv: &[String]) -> CliOutput {
    match sendtext_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) => output,
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn sendtext_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<CliOutput, String> {
    let options = sendtext_parse_args(argv)?;
    sendtext_validate_tmux_target(&options.target)?;
    sendtext_validate_text(&options.text)?;
    let report = sendtext_send_text(runner, &options.target, &options.text)
        .map_err(|error| format!("send-text failed: {}", error.message))?;
    Ok(sendtext_success_output(&options.target, &report))
}

fn sendtext_send_text<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
    text: &str,
) -> Result<maw_tmux::SendTextReport, maw_tmux::TmuxError> {
    sendtext_exit_mode_if_needed(runner, target)?;
    let used_buffer = text.contains('\n') || text.len() > 500;
    if used_buffer {
        runner.run_with_stdin("load-buffer", &["-".to_owned()], text.as_bytes())?;
        runner.run("paste-buffer", &["-t".to_owned(), target.to_owned()])?;
    } else {
        runner.run("send-keys", &maw_tmux::tmux_send_keys_literal_args(target, text))?;
    }
    let (enter_attempts, warned_pending) = sendtext_submit_with_confirm(runner, target)?;
    Ok(maw_tmux::SendTextReport {
        used_buffer,
        enter_attempts,
        warned_pending,
    })
}

fn sendtext_exit_mode_if_needed<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
) -> Result<(), maw_tmux::TmuxError> {
    let probe = runner.run(
        "display-message",
        &[
            "-t".to_owned(),
            target.to_owned(),
            "-p".to_owned(),
            "#{pane_in_mode}".to_owned(),
        ],
    );
    if probe.is_ok_and(|raw| raw.trim() == "1") {
        match runner.run(
            "send-keys",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-X".to_owned(),
                "cancel".to_owned(),
            ],
        ) {
            Ok(_) => Ok(()),
            Err(error) if error.message.contains("not in a mode") => Ok(()),
            Err(error) => Err(error),
        }
    } else {
        Ok(())
    }
}

fn sendtext_submit_with_confirm<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
) -> Result<(u32, bool), maw_tmux::TmuxError> {
    for attempt in 1..=4 {
        runner.run("send-keys", &maw_tmux::tmux_send_enter_args(target))?;
        if !sendtext_pane_input_pending(runner, target) {
            return Ok((attempt, false));
        }
    }
    Ok((4, true))
}

fn sendtext_pane_input_pending<R: maw_tmux::TmuxRunner>(runner: &mut R, target: &str) -> bool {
    runner
        .run(
            "capture-pane",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-e".to_owned(),
                "-p".to_owned(),
                "-S".to_owned(),
                "-5".to_owned(),
            ],
        )
        .is_ok_and(|content| maw_tmux::pane_input_pending_from_capture(&content))
}

fn sendtext_parse_args(argv: &[String]) -> Result<SendtextOptions, String> {
    if argv.iter().any(|arg| arg == "--") {
        return Err("send-text does not accept -- separator".to_owned());
    }
    let Some(target) = argv.first() else {
        return Err(sendtext_usage());
    };
    if argv.len() < 2 {
        return Err(sendtext_usage());
    }
    let target = sendtext_validate_cli_target(target)?;
    let text = argv[1..].join(" ");
    sendtext_validate_text(&text)?;
    Ok(SendtextOptions { target, text })
}

fn sendtext_validate_cli_target(value: &str) -> Result<String, String> {
    if value.starts_with('-') || value == "--" {
        return Err(sendtext_flag_like_target(value));
    }
    sendtext_validate_tmux_target(value)?;
    Ok(value.to_owned())
}

fn sendtext_validate_text(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') {
        return Err("send-text text must be non-empty and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch == '\0') {
        return Err("send-text text must not contain NUL characters".to_owned());
    }
    Ok(())
}

fn sendtext_usage() -> String {
    "usage: maw send-text <target> <text...>".to_owned()
}

fn sendtext_flag_like_target(target: &str) -> String {
    format!("\"{target}\" looks like a flag, not a target.\n  usage: maw send-text <target> <text...>")
}

fn sendtext_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err("send-text target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("send-text target must not contain whitespace or control characters".to_owned());
    }
    Ok(())
}

fn sendtext_success_output(target: &str, report: &maw_tmux::SendTextReport) -> CliOutput {
    let method = if report.used_buffer { "buffer" } else { "literal" };
    let mut stdout = format!("  \x1b[32m✓\x1b[0m sent text to {target} ({method})\n");
    if report.warned_pending {
        stdout.push_str("  \x1b[33m⚠\x1b[0m pane still had pending input after Enter retries\n");
    }
    CliOutput {
        code: 0,
        stdout,
        stderr: String::new(),
    }
}

#[cfg(test)]
mod sendtext_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct SendtextMockTmux {
        calls: Vec<(String, Vec<String>)>,
        stdin_calls: Vec<(String, Vec<String>, String)>,
        responses: std::collections::VecDeque<Result<String, maw_tmux::TmuxError>>,
    }

    impl SendtextMockTmux {
        fn sendtext_with_responses(responses: Vec<Result<&str, &str>>) -> Self {
            let responses = responses
                .into_iter()
                .map(|result| result.map(str::to_owned).map_err(maw_tmux::TmuxError::new))
                .collect();
            Self {
                responses,
                ..Default::default()
            }
        }
    }

    impl maw_tmux::TmuxRunner for SendtextMockTmux {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            self.responses
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }

        fn run_with_stdin(
            &mut self,
            subcommand: &str,
            args: &[String],
            stdin: &[u8],
        ) -> Result<String, maw_tmux::TmuxError> {
            self.stdin_calls.push((
                subcommand.to_owned(),
                args.to_vec(),
                String::from_utf8_lossy(stdin).into_owned(),
            ));
            self.responses
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))
        }
    }

    struct SendtextEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl SendtextEnvGuard {
        fn sendtext_new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "MAW_CONFIG_DIR", "TMUX", "PATH"];
            let saved = keys
                .into_iter()
                .map(|key| (key, std::env::var_os(key)))
                .collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-sendtext-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("config")).expect("config");
            std::env::set_var("HOME", root.join("home"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
            std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
            std::env::set_var("TMUX", "fake-tmux-socket");
            std::env::set_var("PATH", root.join("bin"));
            Self { saved }
        }
    }

    impl Drop for SendtextEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }

    fn sendtext_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn sendtext_dispatch_registers_send_text() {
        assert_eq!(DISPATCH_84.len(), 1);
        assert_eq!(DISPATCH_84[0].command, "send-text");
    }

    #[test]
    fn sendtext_literal_path_joins_text_and_enters() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux =
            SendtextMockTmux::sendtext_with_responses(vec![Ok("0"), Ok(""), Ok(""), Ok("$ \r")]);

        let output = sendtext_with_runner(&sendtext_strings(&["sess:1.0", "hello", "world"]), &mut tmux)
            .expect("send");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m sent text to sess:1.0 (literal)\n");
        assert_eq!(tmux.calls[0], ("display-message".to_owned(), sendtext_strings(&["-t", "sess:1.0", "-p", "#{pane_in_mode}"])));
        assert_eq!(tmux.calls[1], ("send-keys".to_owned(), sendtext_strings(&["-t", "sess:1.0", "-l", "hello world"])));
        assert_eq!(tmux.calls[2], ("send-keys".to_owned(), sendtext_strings(&["-t", "sess:1.0", "Enter"])));
        assert!(tmux.stdin_calls.is_empty());
    }

    #[test]
    fn sendtext_buffer_path_is_hermetic_for_long_text() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = SendtextEnvGuard::sendtext_new();
        let long_text = "x".repeat(501);
        let mut tmux =
            SendtextMockTmux::sendtext_with_responses(vec![Ok("0"), Ok(""), Ok(""), Ok("$ \r")]);

        let output = sendtext_with_runner(&[String::from("sess:1.0"), long_text.clone()], &mut tmux)
            .expect("send");

        assert!(output.stdout.contains("(buffer)"));
        assert_eq!(tmux.calls[1], ("paste-buffer".to_owned(), sendtext_strings(&["-t", "sess:1.0"])));
        assert_eq!(
            tmux.stdin_calls,
            vec![("load-buffer".to_owned(), vec!["-".to_owned()], long_text)]
        );
    }

    #[test]
    fn sendtext_rejects_separator_and_leading_dash_before_tmux() {
        let mut tmux = SendtextMockTmux::default();
        let err = sendtext_with_runner(&sendtext_strings(&["--", "hi"]), &mut tmux).expect_err("target");
        assert!(err.contains("-- separator"));
        let err = sendtext_with_runner(&sendtext_strings(&["sess:1", "-oops"]), &mut tmux).expect_err("text");
        assert!(err.contains("not start with '-'"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn sendtext_rejects_bad_targets_before_tmux() {
        let mut tmux = SendtextMockTmux::default();
        let err = sendtext_with_runner(&sendtext_strings(&["bad target", "hi"]), &mut tmux).expect_err("target");
        assert!(err.contains("must not contain whitespace"));
        let err = sendtext_with_runner(&sendtext_strings(&["-Sbad", "hi"]), &mut tmux).expect_err("target");
        assert!(err.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn sendtext_warns_when_pending_input_remains() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
        ]);

        let output =
            sendtext_with_runner(&sendtext_strings(&["sess:1", "deploy"]), &mut tmux).expect("send");

        assert!(output.stdout.contains("pending input after Enter retries"));
        assert_eq!(
            tmux.calls
                .iter()
                .filter(|(command, args)| command == "send-keys" && args.last().is_some_and(|arg| arg == "Enter"))
                .count(),
            4
        );
    }

    #[test]
    fn sendtext_reports_tmux_failure() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![Ok("0"), Err("no pane")]);

        let err = sendtext_with_runner(&sendtext_strings(&["sess:1", "hi"]), &mut tmux).expect_err("tmux");

        assert!(err.contains("send-text failed: no pane"));
    }
}
