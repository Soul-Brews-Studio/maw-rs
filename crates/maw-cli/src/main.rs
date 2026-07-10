use std::{
    collections::BTreeSet,
    ffi::OsStr,
    io::{self, ErrorKind, IsTerminal, Write},
    process::{Command, Stdio},
};

use maw_tmux::{resolve_tmux_attach_session, TmuxAttachSessionResolution, TmuxClient};

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    std::process::exit(main_code_async(&argv).await);
}

async fn main_code_async(argv: &[String]) -> i32 {
    main_code_async_with(argv, maybe_exec_attach).await
}

#[cfg(test)]
fn main_code(argv: &[String]) -> i32 {
    main_code_with(argv, maybe_exec_attach)
}

#[cfg(test)]
fn main_code_with(argv: &[String], attach: impl FnOnce(&[String]) -> Option<i32>) -> i32 {
    if let Some(code) = attach(argv) {
        return code;
    }
    let output = maw_cli::run_cli(argv);
    finish_cli_output(output.code, &output.stdout, &output.stderr)
}

async fn main_code_async_with(
    argv: &[String],
    attach: impl FnOnce(&[String]) -> Option<i32>,
) -> i32 {
    if let Some(code) = attach(argv) {
        return code;
    }
    let output = maw_cli::run_cli_async(argv).await;
    finish_cli_output(output.code, &output.stdout, &output.stderr)
}

fn finish_cli_output(code: i32, stdout: &str, stderr: &str) -> i32 {
    match write_cli_output(stdout, stderr) {
        Ok(()) => code,
        Err(error) if error.kind() == ErrorKind::BrokenPipe => 0,
        Err(error) => {
            let _ = writeln!(
                io::stderr().lock(),
                "maw-rs: failed to write output: {error}"
            );
            1
        }
    }
}

fn write_cli_output(stdout: &str, stderr: &str) -> io::Result<()> {
    let mut out = io::stdout().lock();
    if !stdout.is_empty() {
        out.write_all(stdout.as_bytes())?;
    }
    let mut err = io::stderr().lock();
    if !stderr.is_empty() {
        err.write_all(stderr.as_bytes())?;
    }
    Ok(())
}

fn maybe_exec_attach(argv: &[String]) -> Option<i32> {
    let mut client = TmuxClient::local();
    let alive_sessions = client.list_session_names();
    maybe_exec_attach_with(
        argv,
        std::io::stdout().is_terminal(),
        std::env::var_os("TMUX").is_some(),
        &alive_sessions,
        run_tmux_attach,
    )
}

fn maybe_exec_attach_with(
    argv: &[String],
    stdout_is_terminal: bool,
    inside_tmux: bool,
    alive_sessions: &[String],
    run: impl FnOnce(Vec<String>) -> i32,
) -> Option<i32> {
    attach_exec_tmux_args(argv, stdout_is_terminal, inside_tmux, alive_sessions).map(run)
}

fn run_tmux_attach(tmux_args: Vec<String>) -> i32 {
    run_tmux_attach_with(OsStr::new("tmux"), tmux_args)
}

fn run_tmux_attach_with(program: &OsStr, tmux_args: Vec<String>) -> i32 {
    let status = Command::new(program)
        .args(tmux_args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status();
    match status {
        Ok(status) => status.code().unwrap_or(1),
        Err(error) => {
            eprintln!("attach: failed to execute tmux: {error}");
            1
        }
    }
}

fn attach_exec_tmux_args(
    argv: &[String],
    stdout_is_terminal: bool,
    inside_tmux: bool,
    alive_sessions: &[String],
) -> Option<Vec<String>> {
    let verb = argv.first()?.as_str();
    if !matches!(verb, "a" | "attach") {
        return None;
    }
    if argv.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "--help" | "-h" | "--print" | "--plan-json" | "--dry-run"
        )
    }) {
        return None;
    }
    if !stdout_is_terminal {
        return None;
    }

    let mut readonly = false;
    let mut target: Option<&str> = None;
    for arg in argv.iter().skip(1).map(String::as_str) {
        match arg {
            "--readonly" | "--read-only" | "-r" => readonly = true,
            arg if arg.starts_with('-') => return None,
            value => {
                if target.is_some() {
                    return None;
                }
                target = Some(value);
            }
        }
    }
    let session_query = target?.split(':').next().unwrap_or_default();
    let alive = alive_sessions.iter().cloned().collect::<BTreeSet<_>>();
    let session = match resolve_tmux_attach_session(session_query, &alive) {
        TmuxAttachSessionResolution::Match { session } => session,
        TmuxAttachSessionResolution::Ambiguous { .. }
        | TmuxAttachSessionResolution::Missing { .. } => return None,
    };
    let tmux_args = if readonly {
        vec![
            "attach".to_owned(),
            "-r".to_owned(),
            "-t".to_owned(),
            session,
        ]
    } else if inside_tmux {
        vec!["switch-client".to_owned(), "-t".to_owned(), session]
    } else {
        vec!["attach".to_owned(), "-t".to_owned(), session]
    };
    Some(tmux_args)
}

#[cfg(test)]
mod tests {
    use super::{
        attach_exec_tmux_args, main_code, main_code_async_with, main_code_with,
        maybe_exec_attach_with, run_tmux_attach_with,
    };
    use std::ffi::OsStr;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn run_tmux_attach_reports_status_and_spawn_errors() {
        assert_eq!(
            run_tmux_attach_with(OsStr::new("/bin/sh"), args(&["-c", "exit 7"])),
            7
        );

        let missing = std::env::temp_dir()
            .join(format!("maw-rs-missing-tmux-{}", std::process::id()))
            .join("tmux");
        assert_eq!(
            run_tmux_attach_with(missing.as_os_str(), args(&["attach", "-t", "50-mawjs"])),
            1
        );
    }

    #[test]
    fn attach_exec_fast_path_rejects_non_live_cli_inputs() {
        assert_eq!(
            attach_exec_tmux_args(&args(&["ls"]), true, false, &args(&["50-mawjs"])),
            None
        );
        assert_eq!(
            attach_exec_tmux_args(&args(&["a", "target"]), false, false, &args(&["target"])),
            None
        );
        assert_eq!(
            attach_exec_tmux_args(
                &args(&["a", "target", "--print"]),
                true,
                false,
                &args(&["target"])
            ),
            None
        );
        assert_eq!(
            attach_exec_tmux_args(&args(&["a", "--unknown"]), true, false, &args(&["target"])),
            None
        );
        assert_eq!(
            attach_exec_tmux_args(
                &args(&["attach", "one", "two"]),
                true,
                false,
                &args(&["one"])
            ),
            None
        );
        assert_eq!(
            attach_exec_tmux_args(&args(&["a"]), true, false, &args(&["50-mawjs"])),
            None
        );
        assert_eq!(
            attach_exec_tmux_args(&args(&["a", "ghost"]), true, false, &args(&["50-mawjs"])),
            None
        );
    }

    #[test]
    fn attach_exec_fast_path_builds_tmux_commands() {
        assert_eq!(
            attach_exec_tmux_args(
                &args(&["a", "mawjs:1.0"]),
                true,
                false,
                &args(&["50-mawjs"])
            ),
            Some(args(&["attach", "-t", "50-mawjs"]))
        );
        assert_eq!(
            attach_exec_tmux_args(
                &args(&["attach", "mawjs"]),
                true,
                true,
                &args(&["50-mawjs"])
            ),
            Some(args(&["switch-client", "-t", "50-mawjs"]))
        );
        assert_eq!(
            attach_exec_tmux_args(
                &args(&["a", "mawjs", "--readonly"]),
                true,
                true,
                &args(&["50-mawjs"])
            ),
            Some(args(&["attach", "-r", "-t", "50-mawjs"]))
        );
        assert_eq!(
            attach_exec_tmux_args(&args(&["a", "volt"]), true, false, &args(&["05-volt"])),
            Some(args(&["attach", "-t", "05-volt"]))
        );
    }

    #[test]
    fn main_code_with_returns_fast_attach_status_without_running_cli() {
        assert_eq!(main_code_with(&args(&["a", "50-mawjs"]), |_| Some(23)), 23);
    }

    #[tokio::test]
    async fn main_code_async_with_returns_fast_attach_status_without_running_cli() {
        assert_eq!(
            main_code_async_with(&args(&["a", "50-mawjs"]), |_| Some(29)).await,
            29
        );
    }

    #[test]
    fn maybe_exec_attach_with_runs_only_valid_fast_path_commands() {
        let code = maybe_exec_attach_with(
            &args(&["a", "mawjs"]),
            true,
            false,
            &args(&["50-mawjs"]),
            |tmux_args| {
                assert_eq!(tmux_args, args(&["attach", "-t", "50-mawjs"]));
                17
            },
        );
        assert_eq!(code, Some(17));

        assert_eq!(main_code(&args(&["--help"])), 0);
    }
}
