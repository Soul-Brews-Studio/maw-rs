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
    if wants_help_before_positionals(argv, &[]) {
        return help_output(sendtext_usage());
    }
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
    sendtext_with_runner_and_sleeper(argv, runner, std::thread::sleep)
}

fn sendtext_with_runner_and_sleeper<R, F>(
    argv: &[String],
    runner: &mut R,
    sleep: F,
) -> Result<CliOutput, String>
where
    R: maw_tmux::TmuxRunner,
    F: FnMut(std::time::Duration),
{
    let options = sendtext_parse_args(argv)?;
    let target = resolve_local_tmux_runner_target(runner, &options.target, "send-text")?;
    sendtext_validate_tmux_target(&target)?;
    sendtext_validate_text(&options.text)?;
    let report = sendtext_send_text(runner, &target, &options.text, sleep)
        .map_err(|error| format!("send-text failed: {}", error.message))?;
    Ok(sendtext_success_output(&target, &report))
}

fn sendtext_send_text<R, F>(
    runner: &mut R,
    target: &str,
    text: &str,
    sleep: F,
) -> Result<maw_tmux::SendTextReport, maw_tmux::TmuxError>
where
    R: maw_tmux::TmuxRunner,
    F: FnMut(std::time::Duration),
{
    maw_tmux::TmuxClient::new(runner).send_text_with_sleeper(target, text, sleep)
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
    CliOutput {
        code: 0,
        stdout: format!("  \x1b[32m✓\x1b[0m sent text to {target} ({method})\n"),
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
        preflight: Option<Result<String, maw_tmux::TmuxError>>,
        preflight_args: Option<Vec<String>>,
        joined_capture_seen: bool,
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
            if subcommand == "capture-pane"
                && args.iter().any(|arg| arg == "-J")
                && !std::mem::replace(&mut self.joined_capture_seen, true)
            {
                self.preflight_args = Some(args.to_vec());
                return self.preflight.take().unwrap_or_else(|| Ok(String::new()));
            }
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

    fn sendtext_with_no_sleep(
        argv: &[String],
        runner: &mut impl maw_tmux::TmuxRunner,
    ) -> Result<CliOutput, String> {
        sendtext_with_runner_and_sleeper(argv, runner, |_| {})
    }

    const SENDTEXT_BUFFERED_TEXT: &str = "deploy\nnow";
    const SENDTEXT_BUFFERED_PLACEHOLDER: &str = "❯ [Pasted Content 10 chars]";

    fn sendtext_buffered_case(
        mut after_paste: Vec<Result<&str, &str>>,
    ) -> (Result<CliOutput, String>, SendtextMockTmux, usize) {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut responses = vec![Ok("0"), Ok(""), Ok("")];
        responses.append(&mut after_paste);
        let mut tmux = SendtextMockTmux::sendtext_with_responses(responses);
        let output = sendtext_with_no_sleep(
            &[String::from("%9"), SENDTEXT_BUFFERED_TEXT.to_owned()],
            &mut tmux,
        );
        let enter_count = tmux
            .calls
            .iter()
            .filter(|(command, args)| {
                command == "send-keys" && args.last().is_some_and(|arg| arg == "Enter")
            })
            .count();
        (output, tmux, enter_count)
    }

    #[test]
    fn sendtext_dispatch_registers_send_text() {
        assert_eq!(DISPATCH_84.len(), 1);
        assert_eq!(DISPATCH_84[0].command, "send-text");
    }

    #[test]
    fn sendtext_literal_path_joins_text_and_enters() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux =
            SendtextMockTmux::sendtext_with_responses(vec![Ok("0"), Ok(""), Ok(""), Ok("$ \r"), Ok("$ \r")]);

        let output = sendtext_with_no_sleep(&sendtext_strings(&["%9", "hello", "world"]), &mut tmux)
            .expect("send");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m sent text to %9 (literal)\n");
        assert_eq!(tmux.calls[0], ("display-message".to_owned(), sendtext_strings(&["-t", "%9", "-p", "#{pane_in_mode}"])));
        assert_eq!(tmux.calls[1], ("send-keys".to_owned(), sendtext_strings(&["-t", "%9", "-l", "hello world"])));
        assert_eq!(tmux.calls[2], ("send-keys".to_owned(), sendtext_strings(&["-t", "%9", "Enter"])));
        assert!(tmux.stdin_calls.is_empty());
    }

    #[test]
    fn sendtext_buffer_path_is_hermetic_for_long_text() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let long_text = "x".repeat(501);
        let mut tmux =
            SendtextMockTmux::sendtext_with_responses(vec![Ok("0"), Ok(""), Ok(""), Ok("$ \r"), Ok("$ \r")]);

        let output = sendtext_with_no_sleep(&[String::from("%9"), long_text.clone()], &mut tmux)
            .expect("send");

        assert!(output.stdout.contains("(buffer)"));
        assert_eq!(tmux.calls[1], ("paste-buffer".to_owned(), sendtext_strings(&["-t", "%9"])));
        assert_eq!(
            tmux.stdin_calls,
            vec![("load-buffer".to_owned(), vec!["-".to_owned()], long_text)]
        );
    }

    #[test]
    fn sendtext_retries_buffered_placeholder_until_capture_clears() {
        let (output, tmux, enter_count) = sendtext_buffered_case(vec![
            Ok(SENDTEXT_BUFFERED_PLACEHOLDER),
            Ok(""),
            Ok("❯ "),
            Ok(SENDTEXT_BUFFERED_PLACEHOLDER),
            Ok(""),
            Ok("❯ "),
            Ok("❯ "),
        ]);
        let output = output.expect("send text ok");

        assert!(!output.stdout.contains("pending input after Enter retries"));
        assert_eq!(enter_count, 2);
        assert_eq!(tmux.calls[1].0, "paste-buffer");
        assert_eq!(tmux.calls[2].0, "capture-pane");
        assert_eq!(tmux.calls[3].0, "send-keys");
    }

    #[test]
    fn sendtext_retries_buffered_literal_echo_until_capture_clears() {
        let (output, _, enter_count) = sendtext_buffered_case(vec![
            Ok("❯ deploy"),
            Ok(""),
            Ok("❯ deploy"),
            Ok("❯ deploy"),
            Ok(""),
            Ok("❯ "),
            Ok("❯ "),
        ]);
        let output = output.expect("send text ok");

        assert!(!output.stdout.contains("pending input after Enter retries"));
        assert_eq!(enter_count, 2);
    }

    #[test]
    fn sendtext_buffered_baseline_does_not_retry_different_input() {
        let (output, _, enter_count) = sendtext_buffered_case(vec![
            Ok(SENDTEXT_BUFFERED_PLACEHOLDER),
            Ok(""),
            Ok("❯ different queued input"),
            Ok("❯ different queued input"),
        ]);

        assert!(output.expect_err("different input must fail").contains("not be confirmed"));
        assert_eq!(enter_count, 1);
    }

    #[test]
    fn sendtext_buffered_baseline_capture_failure_fails_closed() {
        let (output, _, enter_count) = sendtext_buffered_case(vec![
            Err("capture failed"),
            Ok(""),
            Ok(SENDTEXT_BUFFERED_PLACEHOLDER),
            Ok(SENDTEXT_BUFFERED_PLACEHOLDER),
        ]);

        assert!(output.expect_err("unknown baseline must fail").contains("not be confirmed"));
        assert_eq!(enter_count, 1);
    }

    #[test]
    fn sendtext_rejects_separator_and_leading_dash_before_tmux() {
        let mut tmux = SendtextMockTmux::default();
        let err = sendtext_with_no_sleep(&sendtext_strings(&["--", "hi"]), &mut tmux).expect_err("target");
        assert!(err.contains("-- separator"));
        let err = sendtext_with_no_sleep(&sendtext_strings(&["sess:1", "-oops"]), &mut tmux).expect_err("text");
        assert!(err.contains("not start with '-'"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn sendtext_rejects_bad_targets_before_tmux() {
        let mut tmux = SendtextMockTmux::default();
        let err = sendtext_with_no_sleep(&sendtext_strings(&["bad target", "hi"]), &mut tmux).expect_err("target");
        assert!(err.contains("must not contain whitespace"));
        let err = sendtext_with_no_sleep(&sendtext_strings(&["-Sbad", "hi"]), &mut tmux).expect_err("target");
        assert!(err.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn sendtext_fails_when_pending_input_remains() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ deploy"),
        ]);

        let error = sendtext_with_no_sleep(&sendtext_strings(&["%9", "deploy"]), &mut tmux)
            .expect_err("pending input must fail");

        assert!(error.contains("delivery could not be confirmed"));
        assert_eq!(
            tmux.calls
                .iter()
                .filter(|(command, args)| command == "send-keys" && args.last().is_some_and(|arg| arg == "Enter"))
                .count(),
            4
        );
    }

    #[test]
    fn sendtext_refuses_to_append_to_existing_pending_input() {
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![Ok("0")]);
        tmux.preflight = Some(Ok("$ existing input".to_owned()));
        let error = sendtext_with_no_sleep(&sendtext_strings(&["%9", "deploy"]), &mut tmux)
            .expect_err("existing input must be refused before mutation");
        assert!(error.contains("already has pending input"));
        assert_eq!(tmux.preflight_args, Some(sendtext_strings(&["-t", "%9", "-e", "-p", "-J", "-S", "-80"])));
        assert!(tmux.calls.iter().all(|(command, _)| command != "send-keys"));
        assert!(tmux.stdin_calls.is_empty());
    }

    #[test]
    fn sendtext_does_not_retry_non_matching_pending_input() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("$ deploy"),
            Ok("$ different queued input"),
        ]);

        let error =
            sendtext_with_no_sleep(&sendtext_strings(&["%9", "deploy"]), &mut tmux)
                .expect_err("different input must fail");

        assert!(error.contains("inspect the pane before retrying"));
        assert_eq!(
            tmux.calls
                .iter()
                .filter(|(command, args)| command == "send-keys"
                    && args.last().is_some_and(|arg| arg == "Enter"))
                .count(),
            1
        );
    }

    #[test]
    fn sendtext_grace_recheck_catches_false_negative_before_success() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("$ "),
            Ok("$ deploy"),
            Ok(""),
            Ok("$ "),
            Ok("$ "),
        ]);
        let mut sleeps = Vec::new();

        let output = sendtext_with_runner_and_sleeper(
            &sendtext_strings(&["%9", "deploy"]),
            &mut tmux,
            |duration| sleeps.push(duration),
        )
        .expect("send");

        assert!(!output.stdout.contains("pending input after Enter retries"));
        assert_eq!(
            tmux.calls
                .iter()
                .filter(|(command, args)| command == "send-keys"
                    && args.last().is_some_and(|arg| arg == "Enter"))
                .count(),
            2
        );
        assert_eq!(
            sleeps,
            vec![
                std::time::Duration::from_millis(maw_tmux::SEND_SETTLE_MS),
                std::time::Duration::from_millis(maw_tmux::SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(maw_tmux::SUBMIT_GRACE_MS),
                std::time::Duration::from_millis(maw_tmux::SUBMIT_CONFIRM_MS),
                std::time::Duration::from_millis(maw_tmux::SUBMIT_GRACE_MS),
            ]
        );
    }

    #[test]
    fn sendtext_reports_tmux_failure() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![Ok("0"), Err("no pane")]);

        let err = sendtext_with_no_sleep(&sendtext_strings(&["%9", "hi"]), &mut tmux).expect_err("tmux");

        assert!(err.contains("send-text failed: no pane"));
    }

    #[test]
    fn sendtext_explicit_session_window_pins_duplicate_window_names() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![
            Ok(concat!(
                "webhook-relay-v3|||2|||codex-1|||1|||\n",
                "arra-oracle-v3|||4|||codex-1|||0|||\n"
            )),
            Ok("0"),
            Ok(""),
            Ok(""),
            Ok("$ \r"),
            Ok("$ \r"),
        ]);

        let output = sendtext_with_no_sleep(
            &sendtext_strings(&["webhook-relay-v3:codex-1", "hello"]),
            &mut tmux,
        )
        .expect("send");

        assert_eq!(output.stdout, "  \x1b[32m✓\x1b[0m sent text to webhook-relay-v3:2 (literal)\n");
        assert_eq!(
            tmux.calls[2],
            ("send-keys".to_owned(), sendtext_strings(&["-t", "webhook-relay-v3:2", "-l", "hello"]))
        );
    }

    #[test]
    fn sendtext_explicit_session_window_miss_is_loud_without_cross_session_fallback() {
        let _lock = super::env_test_lock();
        let _env = SendtextEnvGuard::sendtext_new();
        let mut tmux = SendtextMockTmux::sendtext_with_responses(vec![Ok(concat!(
            "webhook-relay-v3|||0|||oracle|||1|||\n",
            "arra-oracle-v3|||4|||codex-1|||0|||\n"
        ))]);

        let error = sendtext_with_no_sleep(
            &sendtext_strings(&["webhook-relay-v3:codex-1", "hello"]),
            &mut tmux,
        )
        .expect_err("missing window");

        assert!(error.contains("no window 'codex-1' in session 'webhook-relay-v3'"), "{error}");
        assert_eq!(tmux.calls.len(), 1, "{:?}", tmux.calls);
    }
}
