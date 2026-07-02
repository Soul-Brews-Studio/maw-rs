const DISPATCH_59: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "restart",
        handler: Handler::Sync(run_restart_command),
    },
    DispatcherEntry {
        command: "reboot",
        handler: Handler::Sync(run_restart_command),
    },
];

const RESTART_DEFAULT_REF: &str = "main";
const RESTART_REPOSITORY: &str = "Soul-Brews-Studio/maw-js";

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestartOptions {
    mode: RestartMode,
    no_update: bool,
    git_ref: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RestartMode {
    Run,
    Help,
    Version,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestartSession {
    name: String,
    windows: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RestartProcessOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

trait RestartRunner {
    fn restart_list_sessions(&mut self) -> Result<Vec<RestartSession>, String>;
    fn restart_kill_session(&mut self, name: &str) -> Result<(), String>;
    fn restart_run(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<RestartProcessOutput, String>;
}

struct RestartSystemRunner;

impl RestartRunner for RestartSystemRunner {
    fn restart_list_sessions(&mut self) -> Result<Vec<RestartSession>, String> {
        let mut client = TmuxClient::local();
        Ok(client
            .list_all()
            .into_iter()
            .map(|session| RestartSession {
                name: session.name,
                windows: session
                    .windows
                    .into_iter()
                    .map(|window| window.name)
                    .collect(),
            })
            .collect())
    }

    fn restart_kill_session(&mut self, name: &str) -> Result<(), String> {
        restart_validate_tmux_target(name, "session")?;
        let mut client = TmuxClient::local();
        client.kill_session(name);
        Ok(())
    }

    fn restart_run(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<RestartProcessOutput, String> {
        restart_validate_exec_name(program)?;
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|error| {
                let mut message = String::from("restart: failed to execute ");
                message.push_str(program);
                message.push_str(": ");
                message.push_str(&error.to_string());
                message
            })?;
        Ok(RestartProcessOutput {
            code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

fn run_restart_command(argv: &[String]) -> CliOutput {
    match restart_run_with_runner(argv, &mut RestartSystemRunner) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: restart_error_line(&error),
        },
    }
}

fn restart_run_with_runner(
    argv: &[String],
    runner: &mut impl RestartRunner,
) -> Result<String, String> {
    let options = restart_parse_args(argv)?;
    match options.mode {
        RestartMode::Help => return Ok(restart_help_text().to_owned()),
        RestartMode::Version => return Ok(restart_version_text()),
        RestartMode::Run => {}
    }

    let mut stdout = String::new();
    restart_write_title(&mut stdout);
    restart_clean_stale_sessions(runner, &mut stdout)?;
    restart_update_if_needed(&options, runner, &mut stdout)?;
    restart_stop_fleet(runner, &mut stdout)?;
    restart_wake_fleet(runner, &mut stdout)?;
    stdout.push_str("\n  \u{001b}[32m✓ restart complete\u{001b}[0m\n");
    Ok(stdout)
}

fn restart_help_text() -> &'static str {
    "usage: maw restart [--no-update] [--ref <git-ref>]\n\n  Restart the whole maw fleet:\n    1. kill stale *-view sessions\n    2. update maw-js (unless --no-update)\n    3. stop fleet (maw stop)\n    4. wake fleet (maw wake all)\n\n  Flags:\n    --no-update   skip the git pull + rebuild step\n    --ref <ref>   update to a specific ref (branch/tag/sha) instead of default\n    --version     show native restart bridge version and exit\n    --help, -h    show this message and exit (no side effects)\n"
}

fn restart_version_text() -> String {
    let mut text = String::from("restart ");
    text.push_str(env!("CARGO_PKG_VERSION"));
    text.push('\n');
    text
}

fn restart_parse_args(argv: &[String]) -> Result<RestartOptions, String> {
    let mut options = RestartOptions {
        mode: RestartMode::Run,
        no_update: false,
        git_ref: RESTART_DEFAULT_REF.to_owned(),
    };
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        match arg.as_str() {
            "--help" | "-h" => options.mode = RestartMode::Help,
            "--version" => options.mode = RestartMode::Version,
            "--no-update" => options.no_update = true,
            "--ref" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("restart: missing --ref value".to_owned());
                };
                options.git_ref = restart_validate_ref(value)?;
                index += 1;
            }
            value if value.starts_with("--ref=") => {
                let value = value.trim_start_matches("--ref=");
                options.git_ref = restart_validate_ref(value)?;
            }
            value if value.starts_with('-') => {
                return Err(format!("restart: unknown argument {value}"));
            }
            value => return Err(format!("restart: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(options)
}

fn restart_write_title(stdout: &mut String) {
    stdout.push_str("\n  \u{001b}[36m🔄 maw restart\u{001b}[0m\n");
}

fn restart_clean_stale_sessions(
    runner: &mut impl RestartRunner,
    stdout: &mut String,
) -> Result<(), String> {
    let stale = restart_stale_sessions(runner.restart_list_sessions()?);
    if stale.is_empty() {
        stdout.push_str("\n  \u{001b}[90m1. No stale sessions\u{001b}[0m\n");
        return Ok(());
    }
    let _ = writeln!(
        stdout,
        "\n  \u{001b}[33m1. Cleaning {} stale sessions...\u{001b}[0m",
        stale.len()
    );
    for session in stale {
        restart_validate_tmux_target(&session.name, "session")?;
        runner.restart_kill_session(&session.name)?;
        let _ = writeln!(stdout, "    \u{001b}[90m✗ {}\u{001b}[0m", session.name);
    }
    Ok(())
}

fn restart_stale_sessions(sessions: Vec<RestartSession>) -> Vec<RestartSession> {
    sessions
        .into_iter()
        .filter(restart_is_stale_session)
        .collect()
}

fn restart_is_stale_session(session: &RestartSession) -> bool {
    session.name.ends_with("-view")
        || session.name.starts_with("maw-pty-")
        || session.windows.iter().all(|window| window == "bash")
}

fn restart_update_if_needed(
    options: &RestartOptions,
    runner: &mut impl RestartRunner,
    stdout: &mut String,
) -> Result<(), String> {
    if options.no_update {
        stdout.push_str("\n  \u{001b}[90m2. Update skipped (--no-update)\u{001b}[0m\n");
        return Ok(());
    }
    let _ = writeln!(
        stdout,
        "\n  \u{001b}[33m2. Updating maw-js ({})...\u{001b}[0m",
        options.git_ref
    );
    restart_install_update(runner, &options.git_ref)?;
    let version = restart_maw_version(runner)?;
    stdout.push_str("    updated → ");
    stdout.push_str(&version);
    stdout.push('\n');
    Ok(())
}

fn restart_install_update(runner: &mut impl RestartRunner, git_ref: &str) -> Result<(), String> {
    let spec = restart_global_spec(git_ref);
    let add_args = vec!["add".to_owned(), "-g".to_owned(), spec];
    let first = runner.restart_run("bun", &add_args)?;
    if first.code == 0 {
        return Ok(());
    }
    let remove_args = vec!["remove".to_owned(), "-g".to_owned(), "maw".to_owned()];
    let _ = runner.restart_run("bun", &remove_args)?;
    let retry = runner.restart_run("bun", &add_args)?;
    if retry.code == 0 {
        Ok(())
    } else {
        Err(restart_child_error("bun add -g", &retry))
    }
}

fn restart_global_spec(git_ref: &str) -> String {
    let mut spec = String::from("github:");
    spec.push_str(RESTART_REPOSITORY);
    spec.push('#');
    spec.push_str(git_ref);
    spec
}

fn restart_maw_version(runner: &mut impl RestartRunner) -> Result<String, String> {
    let output = runner.restart_run("maw", &["--version".to_owned()])?;
    if output.code != 0 {
        return Err(restart_child_error("maw --version", &output));
    }
    let version = output.stdout.trim();
    if version.is_empty() {
        Ok("maw --version complete".to_owned())
    } else {
        Ok(version.to_owned())
    }
}

fn restart_stop_fleet(runner: &mut impl RestartRunner, stdout: &mut String) -> Result<(), String> {
    stdout.push_str("\n  \u{001b}[33m3. Stopping fleet...\u{001b}[0m\n");
    let output = runner.restart_run("maw", &["sleep".to_owned()])?;
    if output.code == 0 {
        Ok(())
    } else {
        Err(restart_child_error("maw sleep", &output))
    }
}

fn restart_wake_fleet(runner: &mut impl RestartRunner, stdout: &mut String) -> Result<(), String> {
    stdout.push_str("  \u{001b}[33m4. Waking fleet...\u{001b}[0m\n");
    let output = runner.restart_run("maw", &["wake".to_owned(), "all".to_owned()])?;
    if output.code == 0 {
        Ok(())
    } else {
        Err(restart_child_error("maw wake all", &output))
    }
}

fn restart_validate_ref(value: &str) -> Result<String, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.starts_with('-') {
        return Err("restart: --ref must be a non-option git ref".to_owned());
    }
    if !trimmed.bytes().all(restart_is_safe_ref_byte) {
        return Err("restart: --ref contains unsafe characters".to_owned());
    }
    Ok(trimmed.to_owned())
}

fn restart_is_safe_ref_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/')
}

fn restart_validate_tmux_target(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.starts_with('-') {
        return Err(format!("restart: invalid {label} target"));
    }
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("restart: invalid {label} target"));
    }
    Ok(())
}

fn restart_validate_exec_name(program: &str) -> Result<(), String> {
    if program.trim().is_empty() || program.starts_with('-') || program.contains('/') {
        return Err("restart: invalid executable name".to_owned());
    }
    if program
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err("restart: invalid executable name".to_owned());
    }
    Ok(())
}

fn restart_child_error(label: &str, output: &RestartProcessOutput) -> String {
    let detail = restart_process_detail(output);
    if detail.is_empty() {
        let mut message = String::from("restart: ");
        message.push_str(label);
        message.push_str(" failed");
        message
    } else {
        let mut message = String::from("restart: ");
        message.push_str(label);
        message.push_str(" failed: ");
        message.push_str(&detail);
        message
    }
}

fn restart_process_detail(output: &RestartProcessOutput) -> String {
    let stderr = output.stderr.trim();
    if !stderr.is_empty() {
        return stderr.to_owned();
    }
    output.stdout.trim().to_owned()
}

fn restart_error_line(error: &str) -> String {
    let mut line = String::new();
    line.push_str(error);
    line.push('\n');
    line
}

#[cfg(test)]
mod restart_tests {
    use super::*;

    #[derive(Default)]
    struct RestartFakeRunner {
        sessions: Vec<RestartSession>,
        kills: Vec<String>,
        runs: Vec<(String, Vec<String>)>,
        outputs: Vec<RestartProcessOutput>,
    }

    impl RestartFakeRunner {
        fn restart_with_sessions(sessions: Vec<RestartSession>) -> Self {
            Self {
                sessions,
                ..Self::default()
            }
        }

        fn restart_with_outputs(outputs: Vec<RestartProcessOutput>) -> Self {
            Self {
                outputs,
                ..Self::default()
            }
        }
    }

    impl RestartRunner for RestartFakeRunner {
        fn restart_list_sessions(&mut self) -> Result<Vec<RestartSession>, String> {
            Ok(self.sessions.clone())
        }

        fn restart_kill_session(&mut self, name: &str) -> Result<(), String> {
            restart_validate_tmux_target(name, "session")?;
            self.kills.push(name.to_owned());
            Ok(())
        }

        fn restart_run(
            &mut self,
            program: &str,
            args: &[String],
        ) -> Result<RestartProcessOutput, String> {
            restart_validate_exec_name(program)?;
            self.runs.push((program.to_owned(), args.to_vec()));
            Ok(self.outputs.remove(0))
        }
    }

    fn restart_ok_output(stdout: &str) -> RestartProcessOutput {
        RestartProcessOutput {
            code: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    fn restart_fail_output(stderr: &str) -> RestartProcessOutput {
        RestartProcessOutput {
            code: 1,
            stdout: String::new(),
            stderr: stderr.to_owned(),
        }
    }

    fn restart_session(name: &str, windows: &[&str]) -> RestartSession {
        RestartSession {
            name: name.to_owned(),
            windows: windows.iter().map(|window| (*window).to_owned()).collect(),
        }
    }

    #[test]
    fn restart_help_has_no_side_effects() {
        let mut runner = RestartFakeRunner::default();
        let output = restart_run_with_runner(&["--help".to_owned()], &mut runner).unwrap();
        assert_eq!(output, restart_help_text());
        assert!(runner.runs.is_empty());
        assert!(runner.kills.is_empty());
    }

    #[test]
    fn restart_parse_rejects_unsafe_refs_and_args() {
        assert_eq!(
            restart_parse_args(&["--no-update".to_owned(), "--ref=alpha".to_owned()])
                .unwrap()
                .git_ref,
            "alpha"
        );
        assert!(restart_parse_args(&["--ref".to_owned(), "-bad".to_owned()]).is_err());
        assert!(restart_parse_args(&["--ref=main;rm".to_owned()]).is_err());
        assert!(restart_parse_args(&["--wat".to_owned()]).is_err());
    }

    #[test]
    fn restart_no_update_matches_golden_and_runs_sleep_wake() {
        let mut runner = RestartFakeRunner::restart_with_outputs(vec![
            restart_ok_output(""),
            restart_ok_output(""),
        ]);
        let output = restart_run_with_runner(&["--no-update".to_owned()], &mut runner).unwrap();
        assert_eq!(
            output,
            include_str!("../../tests/fixtures/native-restart/restart-no-update.stdout")
        );
        assert_eq!(
            runner.runs,
            vec![
                ("maw".to_owned(), vec!["sleep".to_owned()]),
                ("maw".to_owned(), vec!["wake".to_owned(), "all".to_owned()]),
            ]
        );
    }

    #[test]
    fn restart_cleans_only_stale_sessions_and_guards_targets() {
        let sessions = vec![
            restart_session("left-view", &["zsh"]),
            restart_session("maw-pty-42", &["node"]),
            restart_session("bash-only", &["bash", "bash"]),
            restart_session("busy", &["zsh"]),
        ];
        let mut runner = RestartFakeRunner::restart_with_sessions(sessions);
        runner.outputs = vec![restart_ok_output(""), restart_ok_output("")];
        restart_run_with_runner(&["--no-update".to_owned()], &mut runner).unwrap();
        assert_eq!(runner.kills, vec!["left-view", "maw-pty-42", "bash-only"]);

        let sessions = vec![restart_session("-bad-view", &["zsh"])];
        let mut runner = RestartFakeRunner::restart_with_sessions(sessions);
        let error = restart_run_with_runner(&["--no-update".to_owned()], &mut runner).unwrap_err();
        assert!(error.contains("invalid session target"));
        assert!(runner.kills.is_empty());
    }

    #[test]
    fn restart_update_uses_safe_argv_and_fallback() {
        let mut runner = RestartFakeRunner::restart_with_outputs(vec![
            restart_fail_output("busy"),
            restart_ok_output("removed"),
            restart_ok_output("installed"),
            restart_ok_output("maw 1.2.3\n"),
            restart_ok_output(""),
            restart_ok_output(""),
        ]);
        restart_run_with_runner(&["--ref".to_owned(), "alpha".to_owned()], &mut runner).unwrap();
        assert_eq!(runner.runs[0].0, "bun");
        assert_eq!(runner.runs[0].1[0], "add");
        assert_eq!(runner.runs[0].1[2], "github:Soul-Brews-Studio/maw-js#alpha");
        assert_eq!(runner.runs[1].1, vec!["remove", "-g", "maw"]);
        assert_eq!(
            runner.runs[3],
            ("maw".to_owned(), vec!["--version".to_owned()])
        );
    }

    #[test]
    fn restart_dispatch_exposes_restart_and_reboot() {
        assert_eq!(DISPATCH_59.len(), 2);
        assert_eq!(DISPATCH_59[0].command, "restart");
        assert_eq!(DISPATCH_59[1].command, "reboot");
    }
}
