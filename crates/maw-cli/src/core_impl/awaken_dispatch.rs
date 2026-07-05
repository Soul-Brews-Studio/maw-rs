const DISPATCH_54: &[DispatcherEntry] = &[DispatcherEntry {
    command: "awaken",
    handler: Handler::Sync(run_awaken_command),
}];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct AwakenOptions {
    name: String,
    from: Option<String>,
    from_repo: Option<String>,
    stem: Option<String>,
    org: Option<String>,
    repo: Option<String>,
    issue: Option<u64>,
    note: Option<String>,
    nickname: Option<String>,
    trigger: Option<String>,
    no_trigger: bool,
    fast: bool,
    root: bool,
    blank: bool,
    pr: bool,
    split: bool,
    seed: bool,
    dry_run: bool,
    signal_on_birth: bool,
    force: bool,
    track_vault: bool,
    sync_peers: bool,
    parent: Option<String>,
    parent_session_id: Option<String>,
    session_id: Option<String>,
    yes: bool,
}

trait AwakenRunner {
    fn awaken_stdin_is_tty(&mut self) -> bool;
    fn awaken_ask_yes_no(&mut self, question: &str) -> bool;
    fn awaken_run(&mut self, program: &str, args: &[String])
        -> Result<AwakenProcessOutput, String>;
    fn awaken_sleep(&mut self, duration: std::time::Duration);
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AwakenProcessOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

struct AwakenSystemRunner;

impl AwakenRunner for AwakenSystemRunner {
    fn awaken_stdin_is_tty(&mut self) -> bool {
        use std::io::IsTerminal as _;
        std::io::stdin().is_terminal()
    }

    fn awaken_ask_yes_no(&mut self, question: &str) -> bool {
        use std::io::{Read as _, Write as _};
        let Ok(mut tty) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("/dev/tty")
        else {
            return false;
        };
        if tty.write_all(question.as_bytes()).is_err() || tty.flush().is_err() {
            return false;
        }
        let mut buf = [0_u8; 8];
        let Ok(bytes_read) = tty.read(&mut buf) else {
            return false;
        };
        let answer = String::from_utf8_lossy(&buf[..bytes_read])
            .trim()
            .to_ascii_lowercase();
        answer == "y" || answer == "yes"
    }

    fn awaken_run(
        &mut self,
        program: &str,
        args: &[String],
    ) -> Result<AwakenProcessOutput, String> {
        awaken_validate_exec_name(program)?;
        let output = std::process::Command::new(program)
            .args(args)
            .output()
            .map_err(|error| {
                let mut message = String::from("awaken: failed to execute ");
                message.push_str(program);
                message.push_str(": ");
                message.push_str(&error.to_string());
                message
            })?;
        Ok(AwakenProcessOutput {
            code: output.status.code().unwrap_or(1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }

    fn awaken_sleep(&mut self, duration: std::time::Duration) {
        std::thread::sleep(duration);
    }
}

fn run_awaken_command(argv: &[String]) -> CliOutput {
    match awaken_run_with_runner(argv, &mut AwakenSystemRunner) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: awaken_error_line(&error),
        },
    }
}

fn awaken_run_with_runner(
    argv: &[String],
    runner: &mut impl AwakenRunner,
) -> Result<String, String> {
    let options = awaken_parse_args(argv)?;
    let mut stdout = String::new();

    if awaken_prompting_needed(&options, runner) {
        stdout.push_str(&awaken_summarize_plan(&options));
        stdout.push('\n');
        if !runner.awaken_ask_yes_no("  Proceed? [y/N] ") {
            stdout.push_str("  aborted — no changes made.\n");
            return Ok(stdout);
        }
    }

    let trigger = awaken_trigger(&options);
    let bud_args = awaken_bud_args(&options)?;
    let bud = runner.awaken_run("maw", &bud_args)?;
    stdout.push_str(&bud.stdout);
    if bud.code != 0 {
        return Err(awaken_child_error("bud", &bud));
    }

    if options.dry_run {
        if let Some(trigger) = trigger {
            stdout.push_str("  \u{001b}[36m⬡\u{001b}[0m [dry-run] would send \u{001b}[33m");
            stdout.push_str(trigger);
            stdout.push_str("\u{001b}[0m to ");
            stdout.push_str(&options.name);
            stdout.push('\n');
        } else {
            stdout.push_str(
                "  \u{001b}[36m⬡\u{001b}[0m [dry-run] --no-trigger: would NOT fire /awaken\n",
            );
        }
        return Ok(stdout);
    }

    let Some(trigger) = trigger else {
        stdout.push_str(
            "  \u{001b}[90m○\u{001b}[0m --no-trigger: bud + wake done, skipping /awaken\n",
        );
        return Ok(stdout);
    };

    let Some(target) = awaken_resolve_target(&options.name, runner, &mut stdout)? else {
        return Ok(stdout);
    };
    if !awaken_wait_for_agent(&target, runner)? {
        stdout.push_str("  \u{001b}[33m⚠\u{001b}[0m timeout waiting for agent in ");
        stdout.push_str(&target);
        stdout.push_str(" after 10000ms\n");
        stdout.push_str("  \u{001b}[90m  pane may still be in zsh — try manually: maw send-text ");
        stdout.push_str(&options.name);
        stdout.push(' ');
        stdout.push_str(trigger);
        stdout.push_str("\u{001b}[0m\n");
        return Ok(stdout);
    }

    stdout.push_str("  \u{001b}[36m🔔\u{001b}[0m firing \u{001b}[33m");
    stdout.push_str(trigger);
    stdout.push_str("\u{001b}[0m → ");
    stdout.push_str(&options.name);
    stdout.push('\n');

    let send_args = vec![
        "send-text".to_owned(),
        options.name.clone(),
        trigger.to_owned(),
    ];
    let sent = runner.awaken_run("maw", &send_args)?;
    stdout.push_str(&sent.stdout);
    if sent.code == 0 {
        stdout.push_str("  \u{001b}[32m✓\u{001b}[0m awakened\n");
    } else {
        stdout.push_str("  \u{001b}[33m⚠\u{001b}[0m send-text failed: ");
        stdout.push_str(&awaken_child_stderr(&sent));
        stdout.push('\n');
        stdout.push_str("  \u{001b}[90m  try manually: maw send-text ");
        stdout.push_str(&options.name);
        stdout.push(' ');
        stdout.push_str(trigger);
        stdout.push_str("\u{001b}[0m\n");
    }
    Ok(stdout)
}

