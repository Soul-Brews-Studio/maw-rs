const DISPATCH_47: &[DispatcherEntry] = &[
    DispatcherEntry { command: "demo", handler: Handler::Sync(run_demo_command) },
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct DemoOptions {
    fast: bool,
    help: bool,
}

fn run_demo_command(argv: &[String]) -> CliOutput {
    match demo_run_with_runner(argv, &mut maw_tmux::CommandTmuxRunner::new()) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(stderr) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{stderr}\n"),
        },
    }
}

fn demo_run_with_runner<R: maw_tmux::TmuxRunner>(
    argv: &[String],
    runner: &mut R,
) -> Result<String, String> {
    let options = demo_parse_options(argv);
    if options.help {
        return Ok(demo_help_text());
    }

    if std::env::var_os("TMUX").is_none() {
        return Ok(demo_no_tmux_text());
    }

    demo_run_tmux_showcase(runner, options.fast)
}

fn demo_parse_options(argv: &[String]) -> DemoOptions {
    DemoOptions {
        fast: argv.iter().any(|arg| arg == "--fast"),
        help: argv.iter().any(|arg| arg == "--help" || arg == "-h"),
    }
}

fn demo_no_tmux_text() -> String {
    concat!(
        "\n",
        "  \x1b[36mmaw demo\x1b[0m — simulated multi-agent session\n",
        "\n",
        "  \x1b[90mThis demo requires an active tmux session.\x1b[0m\n",
        "  Run: \x1b[36mtmux new-session -s demo\x1b[0m\n",
        "  Then re-run: \x1b[36mmaw demo\x1b[0m\n",
        "\n",
    )
    .to_owned()
}

fn demo_help_text() -> String {
    concat!(
        "maw demo — simulated multi-agent session\n",
        "\n",
        "Usage: maw demo [--fast]\n",
        "\n",
        "Spawns two mock agents in tmux panes, streams scripted output with\n",
        "realistic pauses, then shows $0.00 cost. No API key required.\n",
        "\n",
        "Flags:\n",
        "  --fast   Skip sleep delays (CI / screenshot mode)\n",
        "  --help   Show this message\n",
        "\n",
        "Requires an active tmux session.\n",
        "  Run: tmux new-session -s demo\n",
        "  Then: maw demo\n",
    )
    .to_owned()
}

fn demo_run_tmux_showcase<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    fast: bool,
) -> Result<String, String> {
    let mut stdout = String::new();
    demo_header(&mut stdout, "🎬  maw demo — simulated multi-agent session");
    demo_line(&mut stdout, &format!("  {}No API key required. Zero real Claude calls.{}", demo_dim(), demo_reset()));
    demo_line(&mut stdout, &format!("  {}Two mock agents will work on a canned task.{}", demo_dim(), demo_reset()));
    demo_sleep(fast, 1_200);

    let script1 = demo_build_agent1_script(fast);
    let script2 = demo_build_agent2_script(fast);
    let path1 = demo_write_temp_script(&script1)?;
    let path2 = demo_write_temp_script(&script2)?;
    let mut pane1_id = None;
    let mut pane2_id = None;

    let result = (|| {
        demo_step(&mut stdout, "writing agent scripts...");
        demo_sleep(fast, 300);

        demo_step(&mut stdout, "spawning agent-1 in left pane...");
        let caller_pane = demo_caller_target()?;
        let before1 = demo_list_pane_ids(runner);
        demo_split_agent_pane(runner, &caller_pane, true, &path1, "agent-1")?;
        demo_sleep(fast, 800);
        let after1 = demo_list_pane_ids(runner);
        pane1_id = demo_new_pane_id(&before1, &after1);
        demo_ok(
            &mut stdout,
            &format!(
                "agent-1 spawned{}",
                pane1_id
                    .as_deref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            ),
        );
        demo_sleep(fast, 600);

        demo_step(&mut stdout, "spawning agent-2 in right pane...");
        let before2 = demo_list_pane_ids(runner);
        let split_target = pane1_id.as_deref().unwrap_or(&caller_pane);
        demo_validate_tmux_target(split_target)?;
        demo_split_agent_pane(runner, split_target, false, &path2, "agent-2")?;
        demo_sleep(fast, 800);
        let after2 = demo_list_pane_ids(runner);
        pane2_id = demo_new_pane_id(&before2, &after2);
        demo_ok(
            &mut stdout,
            &format!(
                "agent-2 spawned{}",
                pane2_id
                    .as_deref()
                    .map(|id| format!(" ({id})"))
                    .unwrap_or_default()
            ),
        );
        demo_sleep(fast, 600);

        demo_header(&mut stdout, "📡  broadcasting task to both agents");
        demo_step(&mut stdout, "task: \"summarize this repo and suggest improvements\"");
        demo_sleep(fast, 1_500);

        demo_header(&mut stdout, "⏳  agents working...");
        demo_line(&mut stdout, &format!("  {}Watch the side panes for their output.{}", demo_dim(), demo_reset()));
        demo_sleep(fast, if fast { 500 } else { 18_000 });

        demo_header(&mut stdout, "💰  gathering cost data...");
        demo_sleep(fast, 1_000);
        stdout.push_str(&demo_cost_report());
        demo_sleep(fast, 1_500);
        stdout.push_str(&demo_closing_text());
        Ok::<(), String>(())
    })();

    demo_cleanup_pane(runner, pane2_id.as_deref());
    demo_cleanup_pane(runner, pane1_id.as_deref());
    demo_cleanup_script(&path1);
    demo_cleanup_script(&path2);

    result.map(|()| stdout)
}

fn demo_build_agent1_script(fast: bool) -> String {
    let fast_val = if fast { "1" } else { "" };
    [
        "#!/usr/bin/env bash".to_owned(),
        "set -euo pipefail".to_owned(),
        format!("FAST=\"{fast_val}\""),
        "pause() { [ -n \"$FAST\" ] && return 0; sleep \"$1\"; }".to_owned(),
        "echo \"\"".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m ● session started\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m → reading task: 'summarize this repo and suggest improvements'\"".to_owned(),
        "pause 3".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m   scanning source tree...\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m   found 57 command plugins across src/commands/plugins/\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m   found 94 test files (test/ + test/isolated/)\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m   found 19 API endpoints in src/api/\"".to_owned(),
        "pause 3".to_owned(),
        "echo \"\"".to_owned(),
        "echo \"  \\033[36m[agent-1]\\033[0m ✓ summary ready — handing off to agent-2 for improvements pass\"".to_owned(),
        "echo \"\"".to_owned(),
    ]
    .join("\n")
}

fn demo_build_agent2_script(fast: bool) -> String {
    let fast_val = if fast { "1" } else { "" };
    let initial_delay = if fast { "0" } else { "4" };
    [
        "#!/usr/bin/env bash".to_owned(),
        "set -euo pipefail".to_owned(),
        format!("FAST=\"{fast_val}\""),
        format!("sleep {initial_delay}"),
        "pause() { [ -n \"$FAST\" ] && return 0; sleep \"$1\"; }".to_owned(),
        "echo \"\"".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m ● session started\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m → received handoff from agent-1\"".to_owned(),
        "pause 3".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m   analysing improvement opportunities...\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m   [1] ship maw init wizard — reduce setup from 6 steps to 30 seconds\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m   [2] add asciinema to README — first-5-minute retention lever\"".to_owned(),
        "pause 2".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m   [3] maw costs --daily sparkline — 80% already built\"".to_owned(),
        "pause 3".to_owned(),
        "echo \"\"".to_owned(),
        "echo \"  \\033[33m[agent-2]\\033[0m ✓ improvements filed — 3 issues created\"".to_owned(),
        "echo \"\"".to_owned(),
    ]
    .join("\n")
}

fn demo_write_temp_script(content: &str) -> Result<String, String> {
    let suffix = demo_temp_suffix()?;
    let path = std::env::temp_dir().join(format!("maw-demo-{suffix}.sh"));
    std::fs::write(&path, format!("{content}\n"))
        .map_err(|error| format!("demo: write {}: {error}", path.display()))?;
    let path = path.display().to_string();
    demo_validate_script_path(&path)?;
    Ok(path)
}

fn demo_temp_suffix() -> Result<String, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("demo: clock before epoch: {error}"))?
        .as_nanos();
    Ok(format!("{nanos}-{}", std::process::id()))
}

fn demo_split_agent_pane<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: &str,
    horizontal: bool,
    script_path: &str,
    label: &str,
) -> Result<(), String> {
    demo_validate_tmux_target(target)?;
    demo_validate_script_path(script_path)?;
    let orientation = if horizontal { "-h" } else { "-v" };
    let command = format!(
        "bash {}; echo \"  [{label}] session ended\"; read -p \"\" 2>/dev/null || true",
        demo_shell_quote(script_path)
    );
    runner
        .run(
            "split-window",
            &[
                "-t".to_owned(),
                target.to_owned(),
                orientation.to_owned(),
                "-l".to_owned(),
                "50%".to_owned(),
                command,
            ],
        )
        .map(|_| ())
        .map_err(|error| format!("demo: split pane: {}", error.message))
}

fn demo_list_pane_ids<R: maw_tmux::TmuxRunner>(runner: &mut R) -> BTreeSet<String> {
    runner
        .run("list-panes", &["-a".to_owned(), "-F".to_owned(), "#{pane_id}".to_owned()])
        .map(|raw| raw.lines().filter(|line| !line.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

fn demo_new_pane_id(before: &BTreeSet<String>, after: &BTreeSet<String>) -> Option<String> {
    after.iter()
        .find(|id| !before.contains(*id) && demo_validate_tmux_target(id).is_ok())
        .cloned()
}

fn demo_caller_target() -> Result<String, String> {
    let target = std::env::var("TMUX_PANE").unwrap_or_else(|_| ":.".to_owned());
    demo_validate_tmux_target(&target)?;
    Ok(target)
}

fn demo_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        Err("demo: tmux target must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn demo_validate_script_path(path: &str) -> Result<(), String> {
    if path.is_empty() || path.trim() != path || path.starts_with('-') || path.contains('\0') {
        Err("demo: script path must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn demo_shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn demo_cleanup_pane<R: maw_tmux::TmuxRunner>(runner: &mut R, pane_id: Option<&str>) {
    if let Some(pane_id) = pane_id.filter(|id| demo_validate_tmux_target(id).is_ok()) {
        let _ = runner.run("kill-pane", &["-t".to_owned(), pane_id.to_owned()]);
    }
}

fn demo_cleanup_script(path: &str) {
    if demo_validate_script_path(path).is_ok() {
        let _ = std::fs::remove_file(path);
    }
}

fn demo_sleep(fast: bool, millis: u64) {
    if !fast {
        std::thread::sleep(std::time::Duration::from_millis(millis));
    }
}

fn demo_cost_report() -> String {
    let sep = "─".repeat(52);
    format!(
        "\n  {sep}\n  {}COST REPORT — demo session{}\n  {sep}\n  {:>20}  {:>12}  {}$0.00{}\n  {:>20}  {:>12}  {}$0.00{}\n  {sep}\n  {:>20}  {:>12}  {}$0.00{}  {}(demo mode — no real Claude calls){}\n  {sep}\n\n",
        demo_cyan(),
        demo_reset(),
        "agent-1",
        "0 tokens",
        demo_green(),
        demo_reset(),
        "agent-2",
        "0 tokens",
        demo_green(),
        demo_reset(),
        "TOTAL",
        "0 tokens",
        demo_green(),
        demo_reset(),
        demo_dim(),
        demo_reset(),
    )
}

fn demo_closing_text() -> String {
    format!(
        "  {}✓ demo complete.{}\n\n  {}For the real thing:{}\n    {}maw wake <your-repo>{}   — spawn a real agent from any GitHub repo\n    {}maw hey <agent> \"...\"{}   — send it a task\n    {}maw peek <agent>{}         — watch its screen\n    {}maw costs{}                — see what it spent\n\n  {}Install: curl -fsSL https://github.com/Soul-Brews-Studio/maw-js/install.sh | bash{}\n\n",
        demo_green(),
        demo_reset(),
        demo_dim(),
        demo_reset(),
        demo_cyan(),
        demo_reset(),
        demo_cyan(),
        demo_reset(),
        demo_cyan(),
        demo_reset(),
        demo_cyan(),
        demo_reset(),
        demo_dim(),
        demo_reset(),
    )
}

fn demo_header(stdout: &mut String, msg: &str) {
    demo_line(stdout, &format!("\n{}{msg}{}", demo_cyan(), demo_reset()));
}

fn demo_step(stdout: &mut String, msg: &str) {
    demo_line(stdout, &format!("  {}→{} {msg}", demo_dim(), demo_reset()));
}

fn demo_ok(stdout: &mut String, msg: &str) {
    demo_line(stdout, &format!("  {}✓{} {msg}", demo_green(), demo_reset()));
}

fn demo_line(stdout: &mut String, msg: &str) {
    stdout.push_str(msg);
    stdout.push('\n');
}

fn demo_cyan() -> &'static str { "\x1b[36m" }
fn demo_green() -> &'static str { "\x1b[32m" }
fn demo_dim() -> &'static str { "\x1b[90m" }
fn demo_reset() -> &'static str { "\x1b[0m" }

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod demo_tests {
    use super::*;
    use std::collections::VecDeque;
    use std::ffi::OsString;

    #[derive(Debug, Default)]
    struct DemoMockTmuxRunner {
        calls: Vec<(String, Vec<String>)>,
        pane_lists: VecDeque<String>,
        fail_split: bool,
    }

    impl maw_tmux::TmuxRunner for DemoMockTmuxRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            self.calls.push((subcommand.to_owned(), args.to_vec()));
            match subcommand {
                "split-window" | "kill-pane" => {
                    if subcommand == "split-window" && self.fail_split {
                        Err(maw_tmux::TmuxError::new("split failed"))
                    } else {
                        Ok(String::new())
                    }
                }
                "list-panes" => Ok(self.pane_lists.pop_front().unwrap_or_default()),
                other => Err(maw_tmux::TmuxError::new(format!("unexpected {other}"))),
            }
        }
    }


    struct DemoEnvRestore {
        tmux: Option<OsString>,
        tmux_pane: Option<OsString>,
    }

    impl DemoEnvRestore {
        fn capture() -> Self {
            Self {
                tmux: std::env::var_os("TMUX"),
                tmux_pane: std::env::var_os("TMUX_PANE"),
            }
        }
    }

    impl Drop for DemoEnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.tmux.take() {
                std::env::set_var("TMUX", value);
            } else {
                std::env::remove_var("TMUX");
            }
            if let Some(value) = self.tmux_pane.take() {
                std::env::set_var("TMUX_PANE", value);
            } else {
                std::env::remove_var("TMUX_PANE");
            }
        }
    }

    fn demo_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn demo_help_matches_maw_js_handler_text() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = DemoEnvRestore::capture();
        std::env::remove_var("TMUX");
        std::env::remove_var("TMUX_PANE");
        assert_eq!(demo_run_with_runner(&demo_strings(&["-h"]), &mut DemoMockTmuxRunner::default()).expect("help"), demo_help_text());
        assert_eq!(demo_parse_options(&demo_strings(&["--fast", "--help"])), DemoOptions { fast: true, help: true });
    }

    #[test]
    fn demo_no_tmux_matches_maw_js_showcase_golden() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = DemoEnvRestore::capture();
        std::env::remove_var("TMUX");
        std::env::remove_var("TMUX_PANE");
        assert_eq!(demo_run_with_runner(&demo_strings(&["--fast"]), &mut DemoMockTmuxRunner::default()).expect("no tmux"), demo_no_tmux_text());
    }

    #[test]
    fn demo_tmux_fast_orchestrates_panes_and_cleans_up() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = DemoEnvRestore::capture();
        std::env::set_var("TMUX", "/tmp/tmux-1000/default,1,0");
        std::env::set_var("TMUX_PANE", "%0");
        let mut runner = DemoMockTmuxRunner {
            pane_lists: VecDeque::from([
                "%0\n".to_owned(),
                "%0\n%1\n".to_owned(),
                "%0\n%1\n".to_owned(),
                "%0\n%1\n%2\n".to_owned(),
            ]),
            ..DemoMockTmuxRunner::default()
        };

        let stdout = demo_run_with_runner(&demo_strings(&["--fast"]), &mut runner).expect("demo");

        assert!(stdout.contains("🎬  maw demo — simulated multi-agent session"), "{stdout}");
        assert!(stdout.contains("agent-1 spawned (%1)"), "{stdout}");
        assert!(stdout.contains("agent-2 spawned (%2)"), "{stdout}");
        assert!(stdout.contains("COST REPORT — demo session"), "{stdout}");
        assert_eq!(runner.calls[0].0, "list-panes");
        assert!(runner.calls.iter().any(|call| call.0 == "split-window" && call.1[1] == "%0"));
        assert!(runner.calls.iter().any(|call| call.0 == "split-window" && call.1[1] == "%1"));
        assert!(runner.calls.iter().any(|call| call.0 == "kill-pane" && call.1 == demo_strings(&["-t", "%2"])));
        assert!(runner.calls.iter().any(|call| call.0 == "kill-pane" && call.1 == demo_strings(&["-t", "%1"])));
        std::env::remove_var("TMUX");
        std::env::remove_var("TMUX_PANE");
    }

    #[test]
    fn demo_rejects_option_injection_caller_target() {
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = DemoEnvRestore::capture();
        std::env::set_var("TMUX", "/tmp/tmux-1000/default,1,0");
        std::env::set_var("TMUX_PANE", "-tbad");
        let error = demo_run_with_runner(&demo_strings(&["--fast"]), &mut DemoMockTmuxRunner::default()).expect_err("guard");
        assert!(error.contains("not start with '-'"), "{error}");
    }
}
