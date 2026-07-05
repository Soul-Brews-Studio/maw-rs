use self::more_discover::LiveTeamState;

const DISPATCH_324: &[DispatcherEntry] = &[DispatcherEntry {
    command: "more",
    handler: Handler::Sync(run_more_command),
}];

const MORE_USAGE: &str = "usage: maw more codex [N] [--session <s>] [--dry-run] [-e|--engine <e>]\n       maw more status\n\nPlan additional Codex coder lanes.";
const MORE_DEFAULT_BASE: &str = "origin/alpha";
const MORE_CODEX_ENGINE: &str = "codex";

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoreCodexOptions {
    count: u32,
    dry_run: bool,
    engine: String,
    session: Option<String>,
}

trait MoreRuntime {
    fn more_current_session(&mut self) -> Result<String, String>;
    fn more_discover(&mut self, session: &str) -> Result<LiveTeamState, String>;
    fn more_spawn(&mut self, prefix: &str, index: u32, base: &str) -> Result<SpawnResult, String>;
    fn more_status(&mut self) -> String;
}

struct MoreSystemRuntime {
    tmux: maw_tmux::CommandTmuxRunner,
}

impl Default for MoreSystemRuntime {
    fn default() -> Self {
        Self {
            tmux: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl MoreRuntime for MoreSystemRuntime {
    fn more_current_session(&mut self) -> Result<String, String> {
        let raw = maw_tmux::TmuxRunner::run(
            &mut self.tmux,
            "display-message",
            &["-p".to_owned(), "#{session_name}".to_owned()],
        )
        .map_err(|error| {
            format!(
                "more codex: --session is required outside tmux ({})",
                error.message
            )
        })?;
        let session = raw.trim().to_owned();
        if session.is_empty() {
            Err("more codex: --session is required outside tmux".to_owned())
        } else {
            Ok(session)
        }
    }

    fn more_discover(&mut self, session: &str) -> Result<LiveTeamState, String> {
        more_discover::more_discover_live_team_state(&mut self.tmux, session)
    }

    fn more_spawn(&mut self, prefix: &str, index: u32, base: &str) -> Result<SpawnResult, String> {
        more_spawn_codex(prefix, index, base)
    }

    fn more_status(&mut self) -> String {
        more_status_live()
    }
}

fn run_more_command(argv: &[String]) -> CliOutput {
    let mut runtime = MoreSystemRuntime::default();
    match more_run_with_runtime(argv, &mut runtime) {
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

fn more_run_with_runtime(
    argv: &[String],
    runtime: &mut impl MoreRuntime,
) -> Result<String, String> {
    let Some((subcommand, rest)) = argv.split_first() else {
        return Ok(format!("{MORE_USAGE}\n"));
    };
    match subcommand.as_str() {
        "--help" | "-h" => Ok(format!("{MORE_USAGE}\n")),
        "codex" => {
            let options = more_parse_codex(rest)?;
            more_run_codex(&options, runtime)
        }
        "status" => more_run_status(rest, runtime),
        value => Err(format!("more: unknown subcommand '{value}'\n{MORE_USAGE}")),
    }
}

fn more_run_codex(
    options: &MoreCodexOptions,
    runtime: &mut impl MoreRuntime,
) -> Result<String, String> {
    let session = match options.session.as_deref() {
        Some(value) => value.to_owned(),
        None => runtime.more_current_session()?,
    };
    let live = runtime.more_discover(&session)?;
    let base = live
        .base_branch
        .as_deref()
        .unwrap_or(MORE_DEFAULT_BASE)
        .to_owned();
    let indices = more_spawn_indices(live.next_index, options.count)?;

    if options.dry_run {
        return Ok(more_render_codex_plan(options, &live));
    }
    if options.engine != MORE_CODEX_ENGINE {
        return Err(format!(
            "more codex: only engine '{MORE_CODEX_ENGINE}' can spawn today, got '{}'",
            options.engine
        ));
    }

    let mut spawned = Vec::with_capacity(indices.len());
    for index in indices {
        spawned.push(runtime.more_spawn(&live.prefix, index, &base)?);
    }
    Ok(more_render_codex_spawned(options, &live, &spawned))
}

fn more_run_status(argv: &[String], runtime: &mut impl MoreRuntime) -> Result<String, String> {
    if argv
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Ok(format!("{MORE_USAGE}\n"));
    }
    if let Some(extra) = argv.first() {
        return Err(format!(
            "more status: unknown argument {extra}\n{MORE_USAGE}"
        ));
    }
    Ok(runtime.more_status())
}

fn more_parse_codex(argv: &[String]) -> Result<MoreCodexOptions, String> {
    let mut count = None::<u32>;
    let mut dry_run = false;
    let mut engine = MORE_CODEX_ENGINE.to_owned();
    let mut session = None::<String>;
    let mut index = 0usize;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--help" | "-h" => return Err(MORE_USAGE.to_owned()),
            "--dry-run" => dry_run = true,
            "-e" | "--engine" => {
                index += 1;
                engine = more_take_safe_value(argv, index, arg, "engine")?;
            }
            "--session" => {
                index += 1;
                session = Some(more_take_safe_value(argv, index, arg, "session")?);
            }
            value if value.starts_with('-') && more_count_like(value) => {
                more_set_count(&mut count, value)?;
            }
            value if value.starts_with('-') => {
                return Err(format!(
                    "more codex: unknown argument {value}\n{MORE_USAGE}"
                ));
            }
            value => more_set_count(&mut count, value)?,
        }
        index += 1;
    }
    Ok(MoreCodexOptions {
        count: count.unwrap_or(1),
        dry_run,
        engine,
        session,
    })
}

fn more_take_safe_value(
    argv: &[String],
    index: usize,
    flag: &str,
    label: &str,
) -> Result<String, String> {
    let Some(value) = argv.get(index) else {
        return Err(format!(
            "more codex: {flag} requires a {label}\n{MORE_USAGE}"
        ));
    };
    if value.trim().is_empty() || value.starts_with('-') || value.contains(['\n', '\r']) {
        return Err(format!("more codex: invalid {label} '{value}'"));
    }
    Ok(value.to_owned())
}

fn more_set_count(count: &mut Option<u32>, value: &str) -> Result<(), String> {
    if count.is_some() {
        return Err(format!(
            "more codex: count specified more than once\n{MORE_USAGE}"
        ));
    }
    *count = Some(more_parse_count(value)?);
    Ok(())
}

fn more_count_like(value: &str) -> bool {
    value
        .strip_prefix('-')
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|ch| ch.is_ascii_digit()))
}

fn more_parse_count(value: &str) -> Result<u32, String> {
    let Ok(parsed) = value.parse::<u32>() else {
        return Err(format!(
            "more codex: N must be a positive integer, got '{value}'"
        ));
    };
    if parsed == 0 {
        return Err(format!(
            "more codex: N must be a positive integer, got '{value}'"
        ));
    }
    Ok(parsed)
}

fn more_spawn_indices(start: u32, count: u32) -> Result<Vec<u32>, String> {
    (0..count)
        .map(|offset| {
            start
                .checked_add(offset)
                .ok_or_else(|| "more codex: coder index overflow".to_owned())
        })
        .collect()
}

fn more_render_codex_plan(options: &MoreCodexOptions, live: &LiveTeamState) -> String {
    format!(
        "would spawn {} coders in session {} with engine {}\n",
        options.count, live.session, options.engine
    )
}

fn more_render_codex_spawned(
    options: &MoreCodexOptions,
    live: &LiveTeamState,
    spawned: &[SpawnResult],
) -> String {
    use std::fmt::Write as _;

    let mut out = format!(
        "spawned {} coders in session {} with engine {}\n",
        spawned.len(),
        live.session,
        options.engine
    );
    if spawned.is_empty() {
        return out;
    }
    out.push_str("window | worktree | branch\n");
    for result in spawned {
        let _ = writeln!(
            out,
            "{} | {} | {}",
            result.window_name,
            result.worktree_path.display(),
            result.branch
        );
    }
    out
}

#[cfg(test)]
mod more_wiring_tests {
    use super::*;

    #[derive(Debug)]
    struct FakeMoreRuntime {
        current_session: Result<String, String>,
        discover: Result<LiveTeamState, String>,
        status: String,
        discover_sessions: Vec<String>,
        spawns: Vec<(String, u32, String)>,
    }

    impl FakeMoreRuntime {
        fn new(discover: LiveTeamState) -> Self {
            Self {
                current_session: Ok(discover.session.clone()),
                discover: Ok(discover),
                status: "status output\n".to_owned(),
                discover_sessions: Vec::new(),
                spawns: Vec::new(),
            }
        }
    }

    impl MoreRuntime for FakeMoreRuntime {
        fn more_current_session(&mut self) -> Result<String, String> {
            self.current_session.clone()
        }

        fn more_discover(&mut self, session: &str) -> Result<LiveTeamState, String> {
            self.discover_sessions.push(session.to_owned());
            self.discover.clone()
        }

        fn more_spawn(
            &mut self,
            prefix: &str,
            index: u32,
            base: &str,
        ) -> Result<SpawnResult, String> {
            self.spawns
                .push((prefix.to_owned(), index, base.to_owned()));
            Ok(SpawnResult {
                window_name: format!("{prefix}-codex-{index}"),
                worktree_path: std::path::PathBuf::from(format!(
                    "/repo/agents/{prefix}-codex-{index}"
                )),
                branch: format!("agents/{prefix}-codex-{index}"),
                engine: MORE_CODEX_ENGINE.to_owned(),
                success: true,
            })
        }

        fn more_status(&mut self) -> String {
            self.status.clone()
        }
    }

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn live_state(session: &str) -> LiveTeamState {
        LiveTeamState {
            session: session.to_owned(),
            prefix: "maw-rs".to_owned(),
            next_index: 4,
            base_branch: Some("agents/maw-rs-codex-3".to_owned()),
            existing_coders: vec![more_discover::LiveCodexCoder {
                session: session.to_owned(),
                window_index: 3,
                window_name: "maw-rs-codex-3".to_owned(),
                coder_index: 3,
                worktree: Some(std::path::PathBuf::from("/repo/agents/maw-rs-codex-3")),
                branch: Some("agents/maw-rs-codex-3".to_owned()),
            }],
        }
    }

    #[test]
    fn status_delegates_to_more_status() {
        let mut runtime = FakeMoreRuntime::new(live_state("188-maw-rs"));
        assert_eq!(
            more_run_with_runtime(&strings(&["status"]), &mut runtime).expect("status"),
            "status output\n"
        );
        assert!(runtime.discover_sessions.is_empty());
        assert!(runtime.spawns.is_empty());
    }

    #[test]
    fn dry_run_discovers_but_does_not_spawn() {
        let mut runtime = FakeMoreRuntime::new(live_state("188-maw-rs"));
        let output = more_run_with_runtime(
            &strings(&[
                "codex",
                "2",
                "--session",
                "188-maw-rs",
                "--dry-run",
                "-e",
                "omx",
            ]),
            &mut runtime,
        )
        .expect("dry-run");

        assert_eq!(
            output,
            "would spawn 2 coders in session 188-maw-rs with engine omx\n"
        );
        assert_eq!(runtime.discover_sessions, vec!["188-maw-rs"]);
        assert!(runtime.spawns.is_empty());
    }

    #[test]
    fn spawn_uses_discovered_prefix_indices_and_base() {
        let mut runtime = FakeMoreRuntime::new(live_state("188-maw-rs"));
        let output = more_run_with_runtime(
            &strings(&["codex", "2", "--session", "188-maw-rs"]),
            &mut runtime,
        )
        .expect("spawn");

        assert!(
            output.starts_with("spawned 2 coders in session 188-maw-rs with engine codex\n"),
            "{output}"
        );
        assert_eq!(
            runtime.spawns,
            vec![
                ("maw-rs".to_owned(), 4, "agents/maw-rs-codex-3".to_owned()),
                ("maw-rs".to_owned(), 5, "agents/maw-rs-codex-3".to_owned()),
            ]
        );
    }

    #[test]
    fn missing_session_uses_current_tmux_session() {
        let mut runtime = FakeMoreRuntime::new(live_state("188-maw-rs"));
        let output = more_run_with_runtime(&strings(&["codex", "1", "--dry-run"]), &mut runtime)
            .expect("dry-run");

        assert_eq!(
            output,
            "would spawn 1 coders in session 188-maw-rs with engine codex\n"
        );
        assert_eq!(runtime.discover_sessions, vec!["188-maw-rs"]);
    }
}
