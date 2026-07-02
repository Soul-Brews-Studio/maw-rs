const DISPATCH_114: &[DispatcherEntry] = &[DispatcherEntry { command: "stream", handler: Handler::Sync(stream_run_command) }];

const STREAM_USAGE: &str = "usage: maw-rs stream <session>:<win> [--into <session>] [--name <alias>] [--dry-run|--plan-json] | maw-rs stream --unlink <session>:<alias> [--dry-run|--plan-json]";
const STREAM_PLACEHOLDER_WINDOW: &str = "maw-stream-placeholder";

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamOptions { into: Option<String>, name: Option<String>, unlink: bool, dry_run: bool, plan_json: bool }

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamSource { session: String, index: u32, name: String, target: String }

#[derive(Debug, Clone, PartialEq, Eq)]
struct StreamPlan { source: Option<String>, into: String, name: String, target: String, created_destination: bool, renamed_shared_window: bool, unlinked: bool, tmux_commands: Vec<Vec<String>> }

#[derive(Debug, Clone, PartialEq, Eq)]
enum StreamBuildError { Usage(String), Command(String) }

fn stream_run_command(argv: &[String]) -> CliOutput {
    match stream_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(output) | Err(output) => output,
    }
}

fn stream_run_with_runner<R: maw_tmux::TmuxRunner>(argv: &[String], runner: &mut R) -> Result<CliOutput, CliOutput> {
    let (target, opts) = stream_parse_args(argv).map_err(|message| stream_usage_error(&message))?;
    if opts.unlink {
        let plan = stream_build_unlink_plan(&target).map_err(|message| stream_usage_error(&message))?;
        return Ok(stream_execute_or_render_plan(&plan, opts.dry_run, opts.plan_json, runner));
    }
    match stream_build_link_plan(&target, &opts, runner) {
        Ok(plan) => Ok(stream_execute_or_render_plan(&plan, opts.dry_run, opts.plan_json, runner)),
        Err(StreamBuildError::Usage(message)) => Err(stream_usage_error(&message)),
        Err(StreamBuildError::Command(message)) => Err(command_target_error("stream", &message)),
    }
}

fn stream_build_unlink_plan(target: &str) -> Result<StreamPlan, String> {
    let parsed = stream_parse_window_target(target)?;
    Ok(StreamPlan { source: None, into: parsed.0.clone(), name: parsed.1.clone(), target: format!("{}:{}", parsed.0, parsed.1), created_destination: false, renamed_shared_window: false, unlinked: true, tmux_commands: vec![vec!["unlink-window".to_owned(), "-t".to_owned(), format!("{}:{}", parsed.0, parsed.1)]] })
}

fn stream_build_link_plan<R: maw_tmux::TmuxRunner>(target: &str, opts: &StreamOptions, runner: &mut R) -> Result<StreamPlan, StreamBuildError> {
    let source = stream_resolve_source(runner, target).map_err(StreamBuildError::Command)?;
    let destination = stream_destination(opts, runner).map_err(StreamBuildError::Command)?;
    if destination == source.session { return Err(StreamBuildError::Command("destination session must differ from source session".to_owned())); }
    let alias = stream_assert_name("window alias", opts.name.as_deref().unwrap_or(&source.name)).map_err(StreamBuildError::Usage)?;
    let destination_exists = stream_has_session(runner, &destination);
    if !destination_exists && opts.into.is_some() { return Err(StreamBuildError::Command(format!("destination session '{destination}' not found"))); }
    let before = if destination_exists { stream_list_windows(runner, &destination).map_err(StreamBuildError::Command)? } else { Vec::new() };
    if before.iter().any(|window| window.name == alias) {
        let hint = if opts.name.is_some() { "choose a different --name" } else { "use --name <alias>" };
        return Err(StreamBuildError::Command(format!("destination window '{destination}:{alias}' already exists; {hint}")));
    }
    Ok(stream_link_plan_from_parts(source, &destination, &alias, destination_exists, &before, runner))
}

fn stream_destination<R: maw_tmux::TmuxRunner>(opts: &StreamOptions, runner: &mut R) -> Result<String, String> {
    if let Some(into) = opts.into.as_deref() { return stream_assert_name("destination session", into); }
    stream_current_session_name(runner).map(|current| if current.ends_with("-view") { current } else { format!("{current}-view") })
}

fn stream_link_plan_from_parts<R: maw_tmux::TmuxRunner>(source: StreamSource, destination: &str, alias: &str, destination_exists: bool, before: &[maw_tmux::TmuxWindow], runner: &mut R) -> StreamPlan {
    let index = stream_next_window_index(before, stream_destination_base_index(runner, destination));
    let destination_target = format!("{destination}:{index}");
    let mut commands = Vec::new();
    if !destination_exists { commands.push(vec!["new-session".to_owned(), "-d".to_owned(), "-s".to_owned(), destination.to_owned(), "-n".to_owned(), STREAM_PLACEHOLDER_WINDOW.to_owned()]); }
    commands.push(vec!["link-window".to_owned(), "-d".to_owned(), "-s".to_owned(), source.target.clone(), "-t".to_owned(), destination_target.clone()]);
    if alias != source.name { commands.push(vec!["rename-window".to_owned(), "-t".to_owned(), destination_target.clone(), alias.to_owned()]); }
    commands.push(vec!["set-window-option".to_owned(), "-t".to_owned(), destination_target, "@maw-linked-from".to_owned(), source.target.clone()]);
    if !destination_exists { commands.push(vec!["kill-window".to_owned(), "-t".to_owned(), format!("{destination}:{STREAM_PLACEHOLDER_WINDOW}")]); }
    StreamPlan { source: Some(source.target), into: destination.to_owned(), name: alias.to_owned(), target: format!("{destination}:{alias}"), created_destination: !destination_exists, renamed_shared_window: alias != source.name, unlinked: false, tmux_commands: commands }
}

fn stream_parse_args(argv: &[String]) -> Result<(String, StreamOptions), String> {
    let mut opts = StreamOptions { into: None, name: None, unlink: false, dry_run: false, plan_json: false };
    let mut target = None;
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(STREAM_USAGE.to_owned()),
            "--unlink" => opts.unlink = true,
            "--dry-run" => opts.dry_run = true,
            "--plan-json" => opts.plan_json = true,
            "--into" => { let Some(value) = argv.get(index + 1) else { return Err("stream: missing --into value".to_owned()); }; opts.into = Some(value.clone()); index += 1; }
            "--name" => { let Some(value) = argv.get(index + 1) else { return Err("stream: missing --name value".to_owned()); }; opts.name = Some(value.clone()); index += 1; }
            arg if arg.starts_with("--into=") => opts.into = Some(arg["--into=".len()..].to_owned()),
            arg if arg.starts_with("--name=") => opts.name = Some(arg["--name=".len()..].to_owned()),
            arg if arg.starts_with('-') => return Err(format!("stream: unknown argument {arg}")),
            value => { if target.is_some() { return Err("stream: target already provided".to_owned()); } target = Some(value.to_owned()); }
        }
        index += 1;
    }
    target.map(|target| (target, opts)).ok_or_else(|| STREAM_USAGE.to_owned())
}

fn stream_usage_error(message: &str) -> CliOutput {
    let stderr = if message == STREAM_USAGE { format!("{STREAM_USAGE}\n") } else { format!("{message}\n{STREAM_USAGE}\n") };
    CliOutput { code: 2, stdout: String::new(), stderr }
}

fn stream_parse_window_target(target: &str) -> Result<(String, String), String> {
    let raw = target.trim();
    if raw.is_empty() || raw != target || raw.starts_with('-') || raw.chars().any(char::is_control) { return Err(STREAM_USAGE.to_owned()); }
    let Some((session, window)) = raw.split_once(':') else { return Err("stream: target must be <session>:<window>".to_owned()); };
    stream_validate_part(session, "session")?;
    stream_validate_part(window, "window")?;
    if window.rsplit_once('.').is_some_and(|(_, pane)| pane.chars().all(|c| c.is_ascii_digit())) { return Err("stream: target must be a tmux window, not a pane".to_owned()); }
    Ok((session.to_owned(), window.to_owned()))
}

fn stream_assert_name(kind: &str, value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value || trimmed.starts_with('-') || trimmed.contains(':') || trimmed.chars().any(char::is_control) {
        return Err(format!("stream: invalid {kind}: {}", if value.is_empty() { "(empty)" } else { value }));
    }
    Ok(trimmed.to_owned())
}

fn stream_validate_part(value: &str, kind: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) { return Err(format!("stream: invalid {kind}")); }
    Ok(())
}

fn stream_resolve_source<R: maw_tmux::TmuxRunner>(runner: &mut R, target: &str) -> Result<StreamSource, String> {
    let (session, window) = stream_parse_window_target(target)?;
    let windows = stream_list_windows(runner, &session).map_err(|_| format!("source session '{session}' not found"))?;
    if window.chars().all(|c| c.is_ascii_digit()) {
        let index = window.parse::<u32>().map_err(|_| format!("source window '{session}:{window}' not found"))?;
        let Some(found) = windows.iter().find(|candidate| candidate.index == index) else { return Err(format!("source window '{session}:{window}' not found")); };
        return Ok(StreamSource { session, index: found.index, name: found.name.clone(), target: format!("{}:{}", stream_target_session(target), found.index) });
    }
    let exact = windows.iter().filter(|candidate| candidate.name == window).collect::<Vec<_>>();
    match exact.as_slice() {
        [found] => Ok(StreamSource { session: session.clone(), index: found.index, name: found.name.clone(), target: format!("{session}:{}", found.index) }),
        [] => { let available = if windows.is_empty() { "(none)".to_owned() } else { windows.iter().map(|w| format!("{}:{}", w.index, w.name)).collect::<Vec<_>>().join(", ") }; Err(format!("source window '{session}:{window}' not found; windows: {available}")) }
        _ => Err(format!("source window '{session}:{window}' is ambiguous; use one of: {}", exact.iter().map(|w| format!("{session}:{}", w.index)).collect::<Vec<_>>().join(", "))),
    }
}

fn stream_target_session(target: &str) -> String { target.split_once(':').map_or(target, |(session, _)| session).to_owned() }

fn stream_current_session_name<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Result<String, String> {
    let raw = runner.run("display-message", &["-p".to_owned(), "#{session_name}".to_owned()]).map_err(|error| format!("--into is required outside tmux ({})", error.message))?;
    let session = raw.trim().to_owned();
    if session.is_empty() { Err("--into is required outside tmux".to_owned()) } else { Ok(session) }
}

fn stream_destination_base_index<R: maw_tmux::TmuxRunner>(runner: &mut R, session: &str) -> u32 {
    runner.run("show-options", &["-t".to_owned(), session.to_owned(), "-gv".to_owned(), "base-index".to_owned()]).ok().and_then(|raw| raw.trim().parse::<u32>().ok()).unwrap_or(0)
}

fn stream_next_window_index(windows: &[maw_tmux::TmuxWindow], base_index: u32) -> u32 {
    let used = windows.iter().map(|window| window.index).collect::<BTreeSet<_>>();
    (base_index..10_000).find(|index| !used.contains(index)).unwrap_or(base_index)
}

fn stream_execute_or_render_plan<R: maw_tmux::TmuxRunner>(plan: &StreamPlan, dry_run: bool, plan_json: bool, runner: &mut R) -> CliOutput {
    if plan_json { return CliOutput { code: 0, stdout: stream_render_plan_json(plan), stderr: String::new() }; }
    if dry_run { return CliOutput { code: 0, stdout: stream_render_dry_run(plan), stderr: String::new() }; }
    for command in &plan.tmux_commands {
        let Some((subcommand, args)) = command.split_first() else { continue; };
        if let Err(error) = runner.run(subcommand, args) { return command_target_error("stream", &format!("tmux {subcommand} failed: {}", error.message)); }
    }
    CliOutput { code: 0, stdout: stream_format_result(plan), stderr: String::new() }
}

fn stream_render_dry_run(plan: &StreamPlan) -> String {
    let mut stdout = String::new();
    for command in &plan.tmux_commands { let _ = writeln!(stdout, "tmux {}", command.join(" ")); }
    stdout
}

fn stream_render_plan_json(plan: &StreamPlan) -> String {
    format!("{{\"command\":\"stream\",\"source\":{},\"into\":{},\"name\":{},\"target\":{},\"createdDestination\":{},\"renamedSharedWindow\":{},\"unlinked\":{},\"tmuxCommands\":{}}}\n", plan.source.as_ref().map_or("null".to_owned(), |source| json_string(source)), json_string(&plan.into), json_string(&plan.name), json_string(&plan.target), plan.created_destination, plan.renamed_shared_window, plan.unlinked, stream_json_string_matrix(&plan.tmux_commands))
}

fn stream_json_string_matrix(rows: &[Vec<String>]) -> String { format!("[{}]", rows.iter().map(|row| json_string_array(row)).collect::<Vec<_>>().join(",")) }

fn stream_format_result(plan: &StreamPlan) -> String {
    if plan.unlinked { return format!("stream: unlinked {}\n", plan.target); }
    let created = if plan.created_destination { " (created destination)" } else { "" };
    let renamed = if plan.renamed_shared_window { " (renamed shared window)" } else { "" };
    format!("stream: linked {} -> {}{created}{renamed}\n", plan.source.as_deref().unwrap_or(""), plan.target)
}

fn stream_list_windows<R: maw_tmux::TmuxRunner>(runner: &mut R, session: &str) -> Result<Vec<maw_tmux::TmuxWindow>, String> {
    stream_validate_part(session, "session")?;
    let raw = runner.run("list-windows", &["-t".to_owned(), session.to_owned(), "-F".to_owned(), "#{window_index}:#{window_name}:#{window_active}".to_owned()]).map_err(|error| error.message)?;
    Ok(maw_tmux::parse_list_windows(&raw))
}

fn stream_has_session<R: maw_tmux::TmuxRunner>(runner: &mut R, session: &str) -> bool { stream_list_windows(runner, session).is_ok() }

#[cfg(test)]
mod stream_tests {
    use super::*;

    #[derive(Default)]
    struct StreamFakeRunner { calls: Vec<(String, Vec<String>)> }

    impl maw_tmux::TmuxRunner for StreamFakeRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "list-windows" if args.get(1).is_some_and(|s| s == "src") => Ok("1:work:1\n2:ops:0\n".to_owned()),
                "list-windows" if args.get(1).is_some_and(|s| s == "dest") => Ok("0:home:1\n".to_owned()),
                "show-options" => Ok("0\n".to_owned()),
                "display-message" => Ok("dest\n".to_owned()),
                _ => Ok(String::new()),
            }
        }
    }

    fn stream_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn stream_dispatch_fragment_owns_stream() { assert_eq!(DISPATCH_114[0].command, "stream"); }

    #[test]
    fn stream_link_uses_runner_and_executes_argv_vector() {
        let mut runner = StreamFakeRunner::default();
        let output = stream_run_with_runner(&stream_strings(&["src:work", "--into", "dest", "--name", "alias"]), &mut runner).unwrap();
        assert_eq!(output.stdout, "stream: linked src:1 -> dest:alias (renamed shared window)\n");
        assert!(runner.calls.iter().any(|(cmd, _)| cmd == "link-window"));
        assert!(runner.calls.iter().any(|(cmd, _)| cmd == "rename-window"));
    }

    #[test]
    fn stream_rejects_bad_targets_before_runner() {
        let mut runner = StreamFakeRunner::default();
        let err = stream_run_with_runner(&stream_strings(&["-t", "--dry-run"]), &mut runner).unwrap_err();
        assert!(err.stderr.contains("unknown argument -t"));
        assert!(runner.calls.is_empty());
        let err = stream_run_with_runner(&stream_strings(&["bad\nname:win", "--dry-run"]), &mut runner).unwrap_err();
        assert!(err.stderr.contains(STREAM_USAGE));
    }
}
