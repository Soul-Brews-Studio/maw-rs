const DISPATCH_264: &[DispatcherEntry] = &[DispatcherEntry { command: "join", handler: Handler::Sync(join_run_command) }];
const JOIN_USAGE: &str = "usage: maw join <source> --to <session:window> [-v]";

struct JoinOptions { source: String, target: String, vertical: bool }

fn join_run_command(argv: &[String]) -> CliOutput {
    match join_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn join_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<String, (i32, String)> {
    let opts = join_parse(argv)?;
    join_validate_target(&opts.source, "source").map_err(|message| (1, message))?;
    join_validate_target(&opts.target, "target").map_err(|message| (1, message))?;
    let source = join_resolve_source(runner, &opts.source).map_err(|message| (1, message))?;
    join_validate_target(&source, "resolved source").map_err(|message| (1, message))?;
    let direction = if opts.vertical { "-v" } else { "-h" };
    let tmux_args = vec!["-s".to_owned(), source.clone(), "-t".to_owned(), opts.target.clone(), direction.to_owned()];
    runner.run("join-pane", &tmux_args).map_err(|error| (1, format!("join: join-pane failed: {}", error.message)))?;
    Ok(format!("joined {source} → {}\n", opts.target))
}

fn join_parse(argv: &[String]) -> Result<JoinOptions, (i32, String)> {
    let mut source = None;
    let mut target = None;
    let mut vertical = false;
    let mut iter = argv.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--help" => return Err((0, JOIN_USAGE.to_owned())),
            "-v" | "--vertical" => vertical = true,
            "-h" | "--horizontal" => vertical = false,
            "--to" => target = Some(iter.next().ok_or_else(|| (2, "join: --to requires a target".to_owned()))?.clone()),
            value if value.starts_with("--to=") => target = Some(value[5..].to_owned()),
            value if value.starts_with('-') => return Err((2, format!("join: unknown argument {value}"))),
            value => {
                if source.is_some() { return Err((2, "join: source already provided".to_owned())); }
                source = Some(value.to_owned());
            }
        }
    }
    Ok(JoinOptions { source: source.ok_or_else(|| (2, JOIN_USAGE.to_owned()))?, target: target.ok_or_else(|| (2, "join: --to <session:window> is required".to_owned()))?, vertical })
}

fn join_resolve_source<R: maw_tmux::TmuxRunner>(runner: &mut R, source: &str) -> Result<String, String> {
    if source.starts_with('%') || source.contains(':') { Ok(source.to_owned()) } else { resolve_local_tmux_runner_target(runner, source, "join") }
}

fn join_validate_target(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err(format!("join: {label} must be non-empty, unpadded, not '--', and not start with '-'"));
    }
    if value.chars().any(|ch| ch == '\0' || ch.is_control() || ch.is_whitespace()) {
        return Err(format!("join: {label} must not contain whitespace, NUL, or control characters"));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '/' | '@' | '%')) {
        return Err(format!("join: {label} contains unsupported characters"));
    }
    Ok(())
}

#[cfg(test)]
mod tmux_join_tests {
    use super::*;
    #[derive(Default)] struct JoinFakeRunner { calls: Vec<(String, Vec<String>)>, windows: String }
    impl maw_tmux::TmuxRunner for JoinFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand { "list-windows" => Ok(self.windows.clone()), "join-pane" => Ok(String::new()), other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))) }
        }
    }
    fn strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
    #[test] fn join_dispatches_and_resolves_oracle_window_to_join_pane() {
        assert_eq!(DISPATCH_264[0].command, "join");
        let mut runner = JoinFakeRunner { windows: "team|||3|||coder|||1|||/tmp\n".to_owned(), ..JoinFakeRunner::default() };
        assert_eq!(join_with_runner(&strings(&["coder", "--to", "lead:main"]), &mut runner).expect("join"), "joined team:3 → lead:main\n");
        assert_eq!(runner.calls[1], ("join-pane".to_owned(), strings(&["-s", "team:3", "-t", "lead:main", "-h"])));
    }
    #[test] fn join_direct_pane_skips_resolution_vertical_and_guards_injection() {
        let mut runner = JoinFakeRunner::default();
        join_with_runner(&strings(&["%42", "--to=lead:main", "-v"]), &mut runner).expect("join");
        assert_eq!(runner.calls, vec![("join-pane".to_owned(), strings(&["-s", "%42", "-t", "lead:main", "-v"]))]);
        let mut runner = JoinFakeRunner::default();
        assert_eq!(join_with_runner(&strings(&["bad;pane", "--to", "lead:main"]), &mut runner).expect_err("guard").0, 1);
        assert!(runner.calls.is_empty());
    }
}
