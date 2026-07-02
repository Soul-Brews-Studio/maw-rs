const DISPATCH_75: &[DispatcherEntry] = &[DispatcherEntry {
    command: "session",
    handler: Handler::Sync(session_run_command),
}];

const SESSION_ADDRESS_FORMAT: &str = "#S\t#W\t#{window_id}\t#{pane_title}\t#{pane_id}";
const SESSION_SHORT_FORMAT: &str = "#S";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct SessionOptions {
    short: bool,
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SessionAddress {
    session: String,
    window: String,
    window_id: String,
    pane_title: String,
    pane_id: String,
}

trait SessionTmux {
    fn session_display_message(&mut self, format: &str) -> Result<String, String>;
}

struct SessionSystemTmux;

impl SessionTmux for SessionSystemTmux {
    fn session_display_message(&mut self, format: &str) -> Result<String, String> {
        session_validate_format(format)?;
        let args = vec!["-p".to_owned(), format.to_owned()];
        let mut runner = maw_tmux::CommandTmuxRunner::new();
        maw_tmux::TmuxRunner::run(&mut runner, "display-message", &args)
            .map_err(|error| format!("session: tmux display-message failed: {error}"))
    }
}

fn session_run_command(argv: &[String]) -> CliOutput {
    session_run_command_with(
        argv,
        std::env::var_os("TMUX").is_some(),
        &mut SessionSystemTmux,
    )
}

fn session_run_command_with(
    argv: &[String],
    in_tmux: bool,
    runner: &mut impl SessionTmux,
) -> CliOutput {
    match session_run(argv, in_tmux, runner) {
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

fn session_run(
    argv: &[String],
    in_tmux: bool,
    runner: &mut impl SessionTmux,
) -> Result<String, String> {
    let options = session_parse_args(argv)?;
    if !in_tmux {
        return Err("maw session requires an active tmux session — run 'maw wake <oracle>' or attach to tmux first".to_owned());
    }
    if options.short {
        return runner
            .session_display_message(SESSION_SHORT_FORMAT)
            .map(|raw| format!("{}\n", raw.trim()));
    }
    let raw = runner.session_display_message(SESSION_ADDRESS_FORMAT)?;
    let address = session_parse_address(raw.trim());
    Ok(if options.json {
        session_render_json(&address)
    } else {
        session_render_human(&address)
    })
}

fn session_parse_args(argv: &[String]) -> Result<SessionOptions, String> {
    let mut options = SessionOptions::default();
    for arg in argv {
        session_parse_arg(arg, &mut options)?;
    }
    Ok(options)
}

fn session_parse_arg(arg: &str, options: &mut SessionOptions) -> Result<(), String> {
    match arg {
        "--short" | "-s" => {
            options.short = true;
            Ok(())
        }
        "--json" => {
            options.json = true;
            Ok(())
        }
        value if value.starts_with('-') => Err(format!("session: unknown argument {value}")),
        value => Err(format!("session: unexpected argument {value}")),
    }
}

fn session_validate_format(format: &str) -> Result<(), String> {
    if format.is_empty() || format.starts_with('-') {
        return Err("session: invalid tmux format".to_owned());
    }
    if format
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control() && byte != b'\t')
    {
        return Err("session: invalid tmux format".to_owned());
    }
    Ok(())
}

fn session_parse_address(raw: &str) -> SessionAddress {
    let parts = raw.split('\t').collect::<Vec<_>>();
    SessionAddress {
        session: parts.first().copied().unwrap_or_default().to_owned(),
        window: parts.get(1).copied().unwrap_or_default().to_owned(),
        window_id: parts.get(2).copied().unwrap_or_default().to_owned(),
        pane_title: parts.get(3).copied().unwrap_or_default().to_owned(),
        pane_id: parts.get(4).copied().unwrap_or_default().to_owned(),
    }
}

fn session_render_json(address: &SessionAddress) -> String {
    format!(
        "{{\"session\":{},\"window\":{},\"window_id\":{},\"pane_title\":{},\"pane_id\":{},\"target\":{}}}\n",
        json_string(&address.session),
        json_string(&address.window),
        json_string(&address.window_id),
        json_string(&address.pane_title),
        json_string(&address.pane_id),
        json_string(&session_target(address))
    )
}

fn session_render_human(address: &SessionAddress) -> String {
    format!(
        "session  {}\nwindow   {}  \u{1b}[90m({})\u{1b}[0m\npane     {}  \u{1b}[90m({})\u{1b}[0m\ntarget   \u{1b}[36m{}:{}\u{1b}[0m  (or {} for the exact pane)\n",
        address.session,
        address.window,
        address.window_id,
        address.pane_title,
        address.pane_id,
        address.session,
        address.window,
        address.pane_id
    )
}

fn session_target(address: &SessionAddress) -> String {
    format!(
        "{}:{}.{}",
        address.session,
        address.window,
        address.pane_id.trim_start_matches('%')
    )
}

#[cfg(test)]
mod session_tests {
    use super::*;

    #[derive(Default)]
    struct SessionFakeTmux {
        responses: Vec<String>,
        calls: Vec<(String, Vec<String>)>,
        error: Option<String>,
    }

    impl SessionTmux for SessionFakeTmux {
        fn session_display_message(&mut self, format: &str) -> Result<String, String> {
            session_validate_format(format)?;
            self.calls
                .push(("display-message".to_owned(), session_args(&["-p", format])));
            if let Some(error) = &self.error {
                return Err(format!("session: tmux display-message failed: {error}"));
            }
            Ok(self.responses.pop().unwrap_or_default())
        }
    }

    fn session_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn session_fake(raw: &str) -> SessionFakeTmux {
        SessionFakeTmux {
            responses: vec![raw.to_owned()],
            calls: Vec::new(),
            error: None,
        }
    }

    #[test]
    fn session_dispatch_registers_native_session() {
        assert_eq!(DISPATCH_75.len(), 1);
        assert_eq!(DISPATCH_75[0].command, "session");
    }

    #[test]
    fn session_short_uses_constant_tmux_format_only() {
        let mut tmux = session_fake("13-nova\n");
        let output = session_run_command_with(&session_args(&["--short"]), true, &mut tmux);
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, "13-nova\n");
        assert_eq!(tmux.calls.len(), 1);
        assert_eq!(tmux.calls[0].0, "display-message");
        assert_eq!(tmux.calls[0].1, session_args(&["-p", SESSION_SHORT_FORMAT]));
    }

    #[test]
    fn session_human_output_matches_whoami_alias_shape() {
        let mut tmux = session_fake("13-nova\tnova-codex-3.0\t@9\tgm-bo\t%42\n");
        let output = session_run_command_with(&Vec::new(), true, &mut tmux);
        assert_eq!(output.code, 0);
        assert!(
            output.stdout.contains("session  13-nova"),
            "{}",
            output.stdout
        );
        assert!(
            output.stdout.contains("window   nova-codex-3.0"),
            "{}",
            output.stdout
        );
        assert!(
            output.stdout.contains("pane     gm-bo"),
            "{}",
            output.stdout
        );
        assert!(output.stdout.contains("target"), "{}", output.stdout);
        assert_eq!(
            tmux.calls[0].1,
            session_args(&["-p", SESSION_ADDRESS_FORMAT])
        );
    }

    #[test]
    fn session_json_output_is_machine_readable() {
        let mut tmux = session_fake("13-nova\tnova-codex-3.0\t@9\tgm-bo\t%42\n");
        let output = session_run_command_with(&session_args(&["--json"]), true, &mut tmux);
        assert_eq!(output.code, 0);
        let value: serde_json::Value = serde_json::from_str(&output.stdout).expect("json");
        assert_eq!(value["session"], "13-nova");
        assert_eq!(value["window"], "nova-codex-3.0");
        assert_eq!(value["window_id"], "@9");
        assert_eq!(value["pane_title"], "gm-bo");
        assert_eq!(value["pane_id"], "%42");
        assert_eq!(value["target"], "13-nova:nova-codex-3.0.42");
    }

    #[test]
    fn session_requires_tmux_before_touching_runner() {
        let mut tmux = session_fake("should-not-read");
        let output = session_run_command_with(&Vec::new(), false, &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output
            .stderr
            .contains("maw session requires an active tmux session"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn session_rejects_leading_dash_and_positionals_before_tmux() {
        let mut tmux = session_fake("should-not-read");
        let bad_flag =
            session_run_command_with(&session_args(&["--target", "-x"]), true, &mut tmux);
        assert_eq!(bad_flag.code, 1);
        assert!(bad_flag.stderr.contains("unknown argument --target"));
        let positional = session_run_command_with(&session_args(&["alpha"]), true, &mut tmux);
        assert_eq!(positional.code, 1);
        assert!(positional.stderr.contains("unexpected argument alpha"));
        assert!(tmux.calls.is_empty());
    }

    #[test]
    fn session_tmux_errors_are_labeled() {
        let mut tmux = SessionFakeTmux {
            responses: Vec::new(),
            calls: Vec::new(),
            error: Some("no server running".to_owned()),
        };
        let output = session_run_command_with(&Vec::new(), true, &mut tmux);
        assert_eq!(output.code, 1);
        assert!(output
            .stderr
            .contains("session: tmux display-message failed"));
        assert!(output.stderr.contains("no server running"));
    }
}
