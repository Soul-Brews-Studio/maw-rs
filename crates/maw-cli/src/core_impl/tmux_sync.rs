const DISPATCH_286: &[DispatcherEntry] = &[];

const TMUX_SUB_286: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["sync"],
    handler: run_tmux_sync_command,
}];

const TMUX_SYNC_USAGE: &str = "usage: maw tmux sync <target> <on|off>";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxSyncOptions {
    target: String,
    state: TmuxSyncState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TmuxSyncState {
    On,
    Off,
}

impl TmuxSyncState {
    fn as_tmux_value(self) -> &'static str {
        match self {
            Self::On => "on",
            Self::Off => "off",
        }
    }
}

fn run_tmux_sync_command(argv: &[String]) -> CliOutput {
    match tmux_sync_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn tmux_sync_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = tmux_sync_parse(argv)?;
    tmux_sync_validate_target(&opts.target).map_err(|message| (1, message))?;
    let window_target = tmux_sync_window_target(&opts.target);
    let tmux_args = vec![
        "-t".to_owned(),
        window_target.clone(),
        "synchronize-panes".to_owned(),
        opts.state.as_tmux_value().to_owned(),
    ];
    runner
        .run("set-window-option", &tmux_args)
        .map_err(|error| (1, format!("tmux sync: set-window-option failed: {}", error.message)))?;
    Ok(format!(
        "✓ synchronize-panes {} for {}\n",
        opts.state.as_tmux_value(),
        window_target
    ))
}

fn tmux_sync_parse(argv: &[String]) -> Result<TmuxSyncOptions, (i32, String)> {
    let mut positionals = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, TMUX_SYNC_USAGE.to_owned())),
            "--" => return Err((2, "tmux sync: -- separator is not allowed".to_owned())),
            value if value.starts_with('-') => {
                return Err((2, format!("tmux sync: unknown argument {value}")));
            }
            value => positionals.push(value.to_owned()),
        }
    }
    if positionals.len() != 2 {
        return Err((2, TMUX_SYNC_USAGE.to_owned()));
    }
    Ok(TmuxSyncOptions {
        target: positionals[0].clone(),
        state: tmux_sync_parse_state(&positionals[1])?,
    })
}

fn tmux_sync_parse_state(value: &str) -> Result<TmuxSyncState, (i32, String)> {
    match value {
        "on" | "true" | "1" => Ok(TmuxSyncState::On),
        "off" | "false" | "0" => Ok(TmuxSyncState::Off),
        _ => Err((2, "tmux sync: state must be one of on/off/true/false/1/0".to_owned())),
    }
}

fn tmux_sync_validate_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err("tmux sync: target must be non-empty, unpadded, not '--', and not start with '-'".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err("tmux sync: target must not contain control characters".to_owned());
    }
    if !value.chars().all(tmux_sync_valid_target_char) {
        return Err("tmux sync: target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_sync_valid_target_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')
}

fn tmux_sync_window_target(target: &str) -> String {
    let Some((prefix, suffix)) = target.rsplit_once('.') else {
        return target.to_owned();
    };
    if suffix.is_empty() || !suffix.bytes().all(|byte| byte.is_ascii_digit()) {
        return target.to_owned();
    }
    if prefix.is_empty() {
        target.to_owned()
    } else {
        prefix.to_owned()
    }
}

#[cfg(test)]
mod tmux_sync_tests {
    use super::*;

    #[derive(Default)]
    struct SyncFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        fail: Option<String>,
    }

    impl maw_tmux::TmuxRunner for SyncFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            if let Some(message) = &self.fail {
                return Err(maw_tmux::TmuxError::new(message.clone()));
            }
            Ok(String::new())
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn tmux_sync_fragment_is_part286_only() {
        assert!(DISPATCH_286.is_empty());
        assert_eq!(TMUX_SUB_286.len(), 1);
        assert_eq!(TMUX_SUB_286[0].names, &["sync"]);
    }

    #[test]
    fn tmux_sync_sets_window_option_with_window_target_and_golden_output() {
        let mut runner = SyncFakeRunner::default();
        let out = tmux_sync_with_runner(&strings(&["nova:2.1", "on"]), &mut runner).expect("sync");
        assert_eq!(out, include_str!("../../tests/fixtures/native-tmux-sync/sync-on.stdout"));
        assert_eq!(
            runner.calls,
            vec![(
                "set-window-option".to_owned(),
                strings(&["-t", "nova:2", "synchronize-panes", "on"]),
            )]
        );
    }

    #[test]
    fn tmux_sync_accepts_boolean_aliases_and_percent_targets() {
        for (raw, expected) in [("true", "on"), ("1", "on"), ("off", "off"), ("false", "off"), ("0", "off")] {
            let mut runner = SyncFakeRunner::default();
            let out = tmux_sync_with_runner(&strings(&["%42", raw]), &mut runner).expect("sync");
            assert_eq!(out, format!("✓ synchronize-panes {expected} for %42\n"));
            assert_eq!(
                runner.calls,
                vec![(
                    "set-window-option".to_owned(),
                    strings(&["-t", "%42", "synchronize-panes", expected]),
                )]
            );
        }
    }

    #[test]
    fn tmux_sync_rejects_bad_state_and_targets_before_runner() {
        let mut runner = SyncFakeRunner::default();
        let bad_state = tmux_sync_with_runner(&strings(&["nova:2", "maybe"]), &mut runner).expect_err("state");
        assert_eq!(bad_state.0, 2);
        assert!(bad_state.1.contains("state must be"));
        let bad_target = tmux_sync_with_runner(&strings(&["-oProxyCommand=bad", "on"]), &mut runner).expect_err("target");
        assert_eq!(bad_target.0, 2);
        assert!(bad_target.1.contains("unknown argument"));
        let bad_control = tmux_sync_with_runner(&strings(&["bad\ntarget", "on"]), &mut runner).expect_err("control");
        assert_eq!(bad_control.0, 1);
        assert!(bad_control.1.contains("control"));
        assert!(runner.calls.is_empty(), "guarded input reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_sync_surfaces_runner_failure() {
        let mut runner = SyncFakeRunner { fail: Some("no server".to_owned()), ..SyncFakeRunner::default() };
        let err = tmux_sync_with_runner(&strings(&["nova:2.1", "off"]), &mut runner).expect_err("failure");
        assert_eq!(err.0, 1);
        assert_eq!(err.1, "tmux sync: set-window-option failed: no server");
        assert_eq!(
            runner.calls,
            vec![(
                "set-window-option".to_owned(),
                strings(&["-t", "nova:2", "synchronize-panes", "off"]),
            )]
        );
    }

    #[test]
    fn tmux_sync_routes_through_tmux_dispatch_fragment() {
        let out = run_tmux_command(&strings(&["sync", "--help"]));
        assert_eq!(out.code, 0);
        assert!(out.stdout.is_empty());
        assert_eq!(out.stderr, format!("{TMUX_SYNC_USAGE}\n"));
    }
}
