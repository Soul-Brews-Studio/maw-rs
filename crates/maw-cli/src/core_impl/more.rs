use self::more_discover::LiveTeamState;

const DISPATCH_324: &[DispatcherEntry] = &[DispatcherEntry {
    command: "more",
    handler: Handler::Sync(run_more_command),
}];

const MORE_USAGE: &str = "usage: maw more codex [N] [--session <s>] [--dry-run] [-e|--engine <e>] [--pool <n|engine>]\n       maw more status\n\nPlan additional Codex coder lanes.";
const MORE_DEFAULT_BASE: &str = "origin/alpha";
const MORE_CODEX_ENGINE: &str = "codex";

#[derive(Debug, Clone, PartialEq, Eq)]
struct MoreCodexOptions {
    count: u32,
    dry_run: bool,
    engine: String,
    pool: Option<String>,
    session: Option<String>,
}

trait MoreRuntime {
    fn more_current_session(&mut self) -> Result<String, String>;
    fn more_discover(&mut self, session: &str) -> Result<LiveTeamState, String>;
    fn more_codex_update(&mut self) -> Result<(), String>;
    fn more_spawn(
        &mut self,
        session: &str,
        prefix: &str,
        index: u32,
        base: &str,
        engine: &str,
    ) -> Result<SpawnResult, String>;
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

    fn more_codex_update(&mut self) -> Result<(), String> {
        let output = std::process::Command::new("codex")
            .arg("update")
            .output()
            .map_err(|error| format!("more codex: failed to run codex update: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(if stderr.is_empty() {
                "more codex: codex update failed".to_owned()
            } else {
                format!("more codex: codex update failed: {stderr}")
            })
        }
    }

    fn more_spawn(
        &mut self,
        session: &str,
        prefix: &str,
        index: u32,
        base: &str,
        engine: &str,
    ) -> Result<SpawnResult, String> {
        let result = more_spawn_codex_engine(prefix, index, base, engine)?;
        more_boot_spawned_codex(session, &result, engine)?;
        Ok(result)
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
    let engine = more_effective_engine(options, &live);

    if options.dry_run {
        return Ok(more_render_codex_plan(options, &live, &engine));
    }
    runtime.more_codex_update()?;

    let mut spawned = Vec::with_capacity(indices.len());
    for index in indices {
        spawned.push(runtime.more_spawn(&session, &live.prefix, index, &base, &engine)?);
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
    let mut pool = None::<String>;
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
                more_validate_engine_token(&engine)?;
            }
            "--pool" => {
                index += 1;
                let pool_value = more_take_safe_value(argv, index, arg, "pool")?;
                engine = more_engine_from_pool(&pool_value)?;
                pool = Some(pool_value);
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
        pool,
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

fn more_engine_from_pool(pool: &str) -> Result<String, String> {
    if pool.chars().all(|ch| ch.is_ascii_digit()) {
        return Ok(format!("omx-{pool}"));
    }
    more_validate_engine_token(pool)?;
    Ok(pool.to_owned())
}

fn more_validate_engine_token(engine: &str) -> Result<(), String> {
    if engine.is_empty() || engine.starts_with('-') {
        return Err(format!("more codex: invalid engine '{engine}'"));
    }
    if !engine
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(format!("more codex: invalid engine '{engine}'"));
    }
    Ok(())
}

fn more_effective_engine(options: &MoreCodexOptions, live: &LiveTeamState) -> String {
    if options.engine != MORE_CODEX_ENGINE || options.pool.is_some() {
        return options.engine.clone();
    }
    live.existing_coders
        .iter()
        .filter_map(|coder| coder.worktree.as_ref())
        .find_map(|worktree| {
            std::fs::read_to_string(worktree.join(".maw-engine"))
                .ok()
                .map(|raw| raw.trim().to_owned())
                .filter(|engine| more_validate_engine_token(engine).is_ok())
        })
        .unwrap_or_else(|| MORE_CODEX_ENGINE.to_owned())
}

fn more_boot_spawned_codex(
    session: &str,
    result: &SpawnResult,
    engine: &str,
) -> Result<(), String> {
    let args = vec![
        result.window_name.clone(),
        "--session".to_owned(),
        session.to_owned(),
        "--no-attach".to_owned(),
        "--new".to_owned(),
        "-e".to_owned(),
        engine.to_owned(),
        "--repo-path".to_owned(),
        result.worktree_path.display().to_string(),
    ];
    let (code, stdout) = wake_run(&args, &mut WakeNativeTmux)?;
    if code == 0 {
        Ok(())
    } else {
        Err(format!(
            "more codex: wake failed for {} with code {code}: {}",
            result.window_name,
            stdout.trim()
        ))
    }
}

fn more_engine_pool(engine: &str) -> &str {
    engine
        .strip_prefix("omx-")
        .filter(|pool| !pool.is_empty() && pool.chars().all(|ch| ch.is_ascii_digit()))
        .unwrap_or("-")
}

fn more_render_codex_plan(options: &MoreCodexOptions, live: &LiveTeamState, engine: &str) -> String {
    format!(
        "would spawn {} coders in session {} with engine {}\n",
        options.count, live.session, engine
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
        spawned
            .first()
            .map_or(options.engine.as_str(), |result| result.engine.as_str())
    );
    for result in spawned {
        let _ = writeln!(
            out,
            "window={} worktree={} branch={} engine={} pool={} boot={}",
            result.window_name,
            result.worktree_path.display(),
            result.branch,
            result.engine,
            more_engine_pool(&result.engine),
            if result.success { "OK" } else { "FAIL" }
        );
        let _ = writeln!(
            out,
            "dispatch=maw hey {}:{} \"<task + done-criteria>\"",
            live.session, result.window_name
        );
    }
    let failed = spawned.iter().filter(|result| !result.success).count();
    let before = live.existing_coders.len();
    let after = before + spawned.len() - failed;
    let _ = writeln!(
        out,
        "before={before} after={after} requested={} spawned={} failed={failed}",
        options.count,
        spawned.len()
    );
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
        updates: usize,
        spawns: Vec<(String, String, u32, String, String)>,
    }

    impl FakeMoreRuntime {
        fn new(discover: LiveTeamState) -> Self {
            Self {
                current_session: Ok(discover.session.clone()),
                discover: Ok(discover),
                status: "status output\n".to_owned(),
                discover_sessions: Vec::new(),
                updates: 0,
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

        fn more_codex_update(&mut self) -> Result<(), String> {
            self.updates += 1;
            Ok(())
        }

        fn more_spawn(
            &mut self,
            session: &str,
            prefix: &str,
            index: u32,
            base: &str,
            engine: &str,
        ) -> Result<SpawnResult, String> {
            self.spawns.push((
                session.to_owned(),
                prefix.to_owned(),
                index,
                base.to_owned(),
                engine.to_owned(),
            ));
            Ok(SpawnResult {
                window_name: format!("{prefix}-codex-{index}"),
                worktree_path: std::path::PathBuf::from(format!(
                    "/repo/agents/{prefix}-codex-{index}"
                )),
                branch: format!("agents/{prefix}-codex-{index}"),
                engine: engine.to_owned(),
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
                (
                    "188-maw-rs".to_owned(),
                    "maw-rs".to_owned(),
                    4,
                    "agents/maw-rs-codex-3".to_owned(),
                    "codex".to_owned()
                ),
                (
                    "188-maw-rs".to_owned(),
                    "maw-rs".to_owned(),
                    5,
                    "agents/maw-rs-codex-3".to_owned(),
                    "codex".to_owned()
                ),
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
