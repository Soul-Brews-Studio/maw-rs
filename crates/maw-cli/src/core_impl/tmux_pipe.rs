const DISPATCH_285: &[DispatcherEntry] = &[];

const TMUX_SUB_285: &[TmuxSubcommandEntry] = &[TmuxSubcommandEntry {
    names: &["pipe", "pipe-pane"],
    handler: run_tmux_pipe_command,
}];

const TMUX_PIPE_USAGE: &str =
    "usage: maw tmux pipe <target> [command] [--input] [--output|--no-output] [--only-if-closed|-o]";

#[derive(Debug, Clone, PartialEq, Eq)]
struct TmuxPipeOptions {
    target: String,
    command: Option<String>,
    input: bool,
    output: Option<bool>,
    only_if_closed: bool,
}

fn run_tmux_pipe_command(argv: &[String]) -> CliOutput {
    match tmux_pipe_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn tmux_pipe_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, (i32, String)> {
    let opts = tmux_pipe_parse(argv)?;
    tmux_pipe_validate_target(&opts.target).map_err(|message| (1, message))?;
    if let Some(command) = opts.command.as_deref() {
        tmux_pipe_validate_explicit_command(command).map_err(|message| (1, message))?;
    }
    let pipe_args = tmux_pipe_args(&opts);
    runner
        .run("pipe-pane", &pipe_args)
        .map_err(|error| (1, format!("tmux pipe: pipe-pane failed for '{}': {}", opts.target, error.message)))?;
    let mode = tmux_pipe_mode_label(&opts);
    let action = opts
        .command
        .as_ref()
        .map_or_else(|| "closed pipe".to_owned(), |_| format!("piped ({mode})"));
    let only = if opts.only_if_closed { " (only-if-closed)" } else { "" };
    Ok(format!("✓ {action} {} → {} [direct]{only}\n", opts.target, opts.target))
}

fn tmux_pipe_parse(argv: &[String]) -> Result<TmuxPipeOptions, (i32, String)> {
    let mut input = false;
    let mut output = None;
    let mut only_if_closed = false;
    let mut positionals = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" => return Err((0, TMUX_PIPE_USAGE.to_owned())),
            "--input" | "-I" => input = true,
            "--output" | "-O" => output = Some(true),
            "--no-output" => output = Some(false),
            "--only-if-closed" | "--open" | "-o" => only_if_closed = true,
            value => positionals.push(value.to_owned()),
        }
    }
    let target = positionals.first().ok_or_else(|| (2, TMUX_PIPE_USAGE.to_owned()))?.to_owned();
    if output == Some(false) && !input {
        return Err((2, "tmux pipe: --no-output requires --input".to_owned()));
    }
    let command = if positionals.len() > 1 {
        let command = positionals[1..].join(" ");
        if command.is_empty() { None } else { Some(command) }
    } else {
        None
    };
    Ok(TmuxPipeOptions { target, command, input, output, only_if_closed })
}

fn tmux_pipe_args(opts: &TmuxPipeOptions) -> Vec<String> {
    let mut args = Vec::new();
    let output = opts.output.unwrap_or(true) || !opts.input;
    if opts.input {
        args.push("-I".to_owned());
    }
    if output {
        args.push("-O".to_owned());
    }
    if opts.only_if_closed {
        args.push("-o".to_owned());
    }
    args.push("-t".to_owned());
    args.push(opts.target.clone());
    if let Some(command) = &opts.command {
        args.push(command.clone());
    }
    args
}

fn tmux_pipe_mode_label(opts: &TmuxPipeOptions) -> String {
    let output = opts.output.unwrap_or(true) || !opts.input;
    match (opts.input, output) {
        (true, true) => "input+output".to_owned(),
        (true, false) => "input".to_owned(),
        (false, _) => "output".to_owned(),
    }
}

fn tmux_pipe_validate_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err("tmux pipe: target must be non-empty, unpadded, not '--', and not start with '-'".to_owned());
    }
    if value.chars().any(char::is_control) {
        return Err("tmux pipe: target must not contain control characters".to_owned());
    }
    if !value.chars().all(tmux_pipe_valid_target_char) {
        return Err("tmux pipe: target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn tmux_pipe_valid_target_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')
}

fn tmux_pipe_validate_explicit_command(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(|ch| ch == '\0' || (ch.is_control() && ch != '\t')) {
        return Err("tmux pipe: command must be explicit operator input without NUL/newline/control characters".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tmux_pipe_tests285 {
    use super::*;

    #[derive(Default)]
    struct PipeFakeRunner {
        calls: Vec<(String, Vec<String>)>,
        fail_pipe: bool,
    }

    impl maw_tmux::TmuxRunner for PipeFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "pipe-pane" if self.fail_pipe => Err(maw_tmux::TmuxError::new("pipe failed")),
                "pipe-pane" => Ok(String::new()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn tmux_pipe_fragment_is_part285_only() {
        assert!(DISPATCH_285.is_empty());
        assert_eq!(TMUX_SUB_285.len(), 1);
        assert_eq!(TMUX_SUB_285[0].names, &["pipe", "pipe-pane"]);
    }

    #[test]
    fn tmux_pipe_default_output_uses_arg_vector_and_explicit_command() {
        let mut runner = PipeFakeRunner::default();
        let out = tmux_pipe_with_runner(&strings(&["%42", "cat", "-n"]), &mut runner).expect("pipe");
        assert_eq!(out, "✓ piped (output) %42 → %42 [direct]\n");
        assert_eq!(runner.calls, vec![("pipe-pane".to_owned(), strings(&["-O", "-t", "%42", "cat -n"]))]);
    }

    #[test]
    fn tmux_pipe_input_no_output_only_if_closed_maps_flags() {
        let mut runner = PipeFakeRunner::default();
        let out = tmux_pipe_with_runner(
            &strings(&["%42", "printf hi", "--input", "--no-output", "-o"]),
            &mut runner,
        )
        .expect("pipe");
        assert_eq!(out, "✓ piped (input) %42 → %42 [direct] (only-if-closed)\n");
        assert_eq!(runner.calls, vec![("pipe-pane".to_owned(), strings(&["-I", "-o", "-t", "%42", "printf hi"]))]);
    }

    #[test]
    fn tmux_pipe_no_command_closes_pipe() {
        let mut runner = PipeFakeRunner::default();
        let out = tmux_pipe_with_runner(&strings(&["session:1.2"]), &mut runner).expect("close pipe");
        assert_eq!(out, "✓ closed pipe session:1.2 → session:1.2 [direct]\n");
        assert_eq!(runner.calls, vec![("pipe-pane".to_owned(), strings(&["-O", "-t", "session:1.2"]))]);
    }

    #[test]
    fn tmux_pipe_rejects_no_output_without_input_before_runner() {
        let mut runner = PipeFakeRunner::default();
        let err = tmux_pipe_with_runner(&strings(&["%42", "cat", "--no-output"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("--no-output requires --input"));
        assert!(runner.calls.is_empty(), "guarded args reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_pipe_rejects_leading_dash_target_before_runner() {
        let mut runner = PipeFakeRunner::default();
        let err = tmux_pipe_with_runner(&strings(&["-bad", "cat"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("target"));
        assert!(runner.calls.is_empty(), "guarded target reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_pipe_rejects_control_target_before_runner() {
        let mut runner = PipeFakeRunner::default();
        let err = tmux_pipe_with_runner(&strings(&["bad\npane", "cat"]), &mut runner).expect_err("guard");
        assert_eq!(err.0, 1);
        assert!(err.1.contains("control"));
        assert!(runner.calls.is_empty(), "guarded target reached tmux: {:?}", runner.calls);
    }

    #[test]
    fn tmux_pipe_fake_maw_no_delegate_and_no_bun_runtime() {
        let _lock = env_test_lock().lock().expect("env lock");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut runner = PipeFakeRunner::default();
        let out = tmux_pipe_with_runner(&strings(&["%42", "cat"]), &mut runner).expect("pipe");
        assert_eq!(out, "✓ piped (output) %42 → %42 [direct]\n");
        assert!(runner.calls.iter().all(|(subcommand, _)| subcommand != "bun"));
        std::env::remove_var("MAW_JS_REF_DIR");
    }
}
