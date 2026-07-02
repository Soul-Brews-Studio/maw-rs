const DISPATCH_113: &[DispatcherEntry] = &[DispatcherEntry { command: "split", handler: Handler::Sync(split_run_command) }];

const SPLIT_USAGE: &str = "usage: maw split <target> [-v|--vertical] [--pct N] [--cmd <cmd>] [--dry-run]";

#[derive(Debug, Clone, PartialEq)]
struct SplitOptions { target: String, vertical: bool, pct: f64, command: Option<String>, dry_run: bool }

fn split_run_command(argv: &[String]) -> CliOutput {
    match split_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn split_run_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = split_parse_args(argv)?;
    split_validate_tmux_target(&opts.target).map_err(|message| (1, message))?;
    if let Some(command) = opts.command.as_deref() { split_validate_command_text(command).map_err(|message| (1, message))?; }
    if opts.dry_run { return Ok(split_render_dry_run(&opts)); }
    let tmux_args = split_tmux_args(&opts).map_err(|message| (1, message))?;
    runner.run("split-window", &tmux_args).map_err(|error| (1, error.message))?;
    Ok(format!("split → {}\n", opts.target))
}

fn split_parse_args(argv: &[String]) -> Result<SplitOptions, (i32, String)> {
    let mut target = None;
    let mut vertical = false;
    let mut pct = 50.0f64;
    let mut command = None;
    let mut dry_run = false;
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err((2, SPLIT_USAGE.to_owned())),
            "-v" | "--vertical" => vertical = true,
            "--dry-run" => dry_run = true,
            "--pct" => {
                let Some(value) = argv.get(index + 1) else { return Err((2, "split: missing --pct value".to_owned())); };
                pct = split_parse_pct(value)?;
                index += 1;
            }
            "--cmd" => {
                let Some(value) = argv.get(index + 1) else { return Err((2, "split: missing --cmd value".to_owned())); };
                command = Some(value.clone());
                index += 1;
            }
            arg if arg.starts_with("--pct=") => pct = split_parse_pct(&arg["--pct=".len()..])?,
            arg if arg.starts_with("--cmd=") => command = Some(arg["--cmd=".len()..].to_owned()),
            arg if arg.starts_with('-') => return Err((2, format!("split: unknown argument {arg}"))),
            value => {
                if target.is_some() { return Err((2, "split: target already provided".to_owned())); }
                target = Some(value.to_owned());
            }
        }
        index += 1;
    }
    Ok(SplitOptions { target: target.ok_or_else(|| (2, SPLIT_USAGE.to_owned()))?, vertical, pct, command, dry_run })
}

fn split_parse_pct(value: &str) -> Result<f64, (i32, String)> {
    value.parse::<f64>().map_err(|_| (2, format!("split: invalid --pct value {value}")))
}

fn split_tmux_args(opts: &SplitOptions) -> Result<Vec<String>, String> {
    let options = maw_tmux::TmuxSplitActionOptions { vertical: opts.vertical, pct: opts.pct, command: opts.command.clone() };
    maw_tmux::tmux_split_action_args(&opts.target, &options).map_err(|error| error.message)
}

fn split_render_dry_run(opts: &SplitOptions) -> String {
    let flag = if opts.vertical { "-v" } else { "-h" };
    let command = opts.command.as_ref().map(|cmd| format!(" -- {cmd}")).unwrap_or_default();
    format!("tmux split-window {flag} -l {}% -t {}{command}\n", opts.pct, opts.target)
}

fn split_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("split target must be non-empty, unpadded, not start with '-', and contain no control characters".to_owned());
    }
    Ok(())
}

fn split_validate_command_text(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("split command must be non-empty and contain no control characters".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod split_tests {
    use super::*;

    #[derive(Default)]
    struct SplitFakeRunner { calls: Vec<(String, Vec<String>)> }

    impl maw_tmux::TmuxRunner for SplitFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            Ok(String::new())
        }
    }

    fn split_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn split_dispatch_fragment_owns_split() {
        assert_eq!(DISPATCH_113[0].command, "split");
    }

    #[test]
    fn split_uses_tmux_runner_and_argv_vec() {
        let mut runner = SplitFakeRunner::default();
        let out = split_run_with_runner(&split_strings(&["%1", "--vertical", "--pct", "25", "--cmd", "echo hi"]), &mut runner).unwrap();
        assert_eq!(out, "split → %1\n");
        assert_eq!(runner.calls[0].0, "split-window");
        assert_eq!(runner.calls[0].1, vec!["-v", "-l", "25%", "-t", "%1", "echo hi"]);
    }

    #[test]
    fn split_rejects_injection_targets_before_runner() {
        let mut runner = SplitFakeRunner::default();
        let err = split_run_with_runner(&split_strings(&["bad\nname"]), &mut runner).unwrap_err();
        assert!(err.1.contains("contain no control"));
        assert!(runner.calls.is_empty());
    }
}
