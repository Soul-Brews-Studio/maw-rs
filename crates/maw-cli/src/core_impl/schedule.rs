const DISPATCH_334: &[DispatcherEntry] = &[DispatcherEntry { command: "schedule", handler: Handler::Sync(schedule_run_command334) }];
const SCHEDULE_USAGE334: &str = "usage: maw schedule run <id> [--force]\nprivate: maw schedule fire <oracle> <id> <repo> [--force] | exec <run-id>";

fn schedule_run_command334(argv: &[String]) -> CliOutput {
    match schedule_dispatch334(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("schedule: {message}\n") },
    }
}

fn schedule_dispatch334(argv: &[String]) -> Result<String, (i32, String)> {
    match argv.first().map(String::as_str) {
        Some("run") => schedule_run334(&argv[1..]).map_err(|message| (1, message)),
        Some("fire") => schedule_fire334(&argv[1..]).map_err(|message| (1, message)),
        Some("exec") => schedule_exec334(&argv[1..]).map_err(|message| (1, message)),
        Some("--help" | "-h") | None => Ok(format!("{SCHEDULE_USAGE334}\n")),
        _ => Err((2, SCHEDULE_USAGE334.to_owned())),
    }
}

fn schedule_run334(argv: &[String]) -> Result<String, String> {
    let (id, forced) = schedule_id_force334(argv)?;
    let repo = std::env::current_dir().map_err(|error| format!("current directory: {error}"))?
        .canonicalize().map_err(|error| format!("resolve repository: {error}"))?;
    let oracle = repo.file_name().and_then(|value| value.to_str()).ok_or_else(|| "repository has no oracle name".to_owned())?;
    let mut fire = vec![oracle.to_owned(), id, repo.to_string_lossy().into_owned()];
    if forced { fire.push("--force".to_owned()); } schedule_fire334(&fire)
}

fn schedule_fire334(argv: &[String]) -> Result<String, String> {
    #[cfg(not(target_os = "macos"))]
    return Err("launchd scheduling is supported only on macOS".to_owned());
    #[cfg(target_os = "macos")]
    {
        let (oracle, id, repo, forced) = schedule_fire_args334(argv)?;
        let state = maw_state_dir(&current_xdg_env());
        let log = state.join("logs").join(format!("{oracle}.{id}.log"));
        schedule_log334(&log, "FIRE_START")?;
        let result = (|| {
        let repo = Path::new(&repo).canonicalize().map_err(|error| format!("resolve repository: {error}"))?;
        let config = std::fs::read_to_string(repo.join(".maw/schedule.toml")).map_err(|error| format!("read schedule config: {error}"))?;
        let file = maw_schedule::parse_schedule(&config).map_err(|error| format!("parse schedule config: {error}"))?;
        let mut jobs = file.schedule.into_iter().filter(|job| job.id == id);
        let job = jobs.next().ok_or_else(|| format!("job {id} not found"))?;
        if jobs.next().is_some() { return Err(format!("duplicate job id {id}")); }
        let cadence_seconds = schedule_cadence_seconds334(&job)?;
        let (local_date, hour) = schedule_local_time334()?;
        let now = current_epoch_seconds();
        let run_id = format!("{oracle}-{id}-{now}-{}", std::process::id());
        schedule_safe334(&run_id, "run id")?;
        let output = state.join("logs").join(format!("{run_id}.out"));
        let claude = (job.exec == maw_schedule::ExecMode::ClaudeHeadless).then(|| maw_schedule_runner::exec::resolve_binary("claude")).transpose()?;
        let pass = (job.exec == maw_schedule::ExecMode::ClaudeHeadless).then(|| maw_schedule_runner::exec::resolve_binary("pass")).transpose()?;
        let tmux = (job.exec == maw_schedule::ExecMode::ClaudeHeadless).then(|| maw_schedule_runner::exec::resolve_binary("tmux")).transpose()?;
        let store = maw_schedule_runner::FireStore::new(state);
        let run = store.reserve(maw_schedule_runner::StartRequest { run_id: run_id.clone(), oracle, job_id: id,
            local_date, reserved_at: now, cadence_seconds, boot_identity: schedule_boot334()?, cap: job.max_fires_per_day,
            forced, exec: job.exec, expected_output: job.expected_output, command: job.command,
            cwd: repo.to_string_lossy().into_owned(), log_path: log.to_string_lossy().into_owned(),
            output_path: (job.exec == maw_schedule::ExecMode::ClaudeHeadless).then(|| output.to_string_lossy().into_owned()),
            token_name: job.token_name, bash_path: "/bin/bash".to_owned(),
            claude_path: claude.map(|path| path.to_string_lossy().into_owned()), pass_path: pass.map(|path| path.to_string_lossy().into_owned()) })?;
        if run.outcome.status == maw_schedule::RunStatus::CapHit { schedule_log334(&log, "CAP_HIT")?; return Ok(format!("cap-hit {run_id}\n")); }
        if run.outcome.exec == maw_schedule::ExecMode::Shell {
            let finished = maw_schedule_runner::exec::execute(&store, &run_id, &run.local_date, &hour, std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok().as_deref())?;
            return serde_json::to_string(&finished).map(|value| format!("{value}\n")).map_err(|error| format!("encode outcome: {error}"));
        }
        let maw = std::env::current_exe().map_err(|error| format!("resolve maw executable: {error}"))?;
        let mut runner = maw_tmux::CommandTmuxRunner::with_program(tmux.expect("headless run resolves tmux"));
        if let Err(error) = schedule_handoff334(&mut runner, &run_id, &repo, &maw) {
            store.finalize(&run_id, maw_schedule_runner::FinishRequest { exited_at: current_epoch_seconds(), exit_code: 1,
                output_file_written: false, output_bytes: 0, deliverable_written: None, expected_output: None, error: Some(error.clone()) })?;
            return Err(error);
        }
        schedule_log334(&log, &format!("HANDOFF {run_id}"))?; Ok(format!("spawned {run_id}\n"))
        })();
        if let Err(error) = &result { schedule_log334(&log, &format!("ERROR {error}"))?; }
        result
    }
}

fn schedule_exec334(argv: &[String]) -> Result<String, String> {
    let [run_id] = argv else { return Err(SCHEDULE_USAGE334.to_owned()); };
    schedule_safe334(run_id, "run id")?;
    let store = maw_schedule_runner::FireStore::new(maw_state_dir(&current_xdg_env()));
    let run = store.load(run_id)?;
    let (_, hour) = schedule_local_time334()?;
    let finished = maw_schedule_runner::exec::execute(&store, run_id, &run.local_date, &hour,
        std::env::var("CLAUDE_CODE_OAUTH_TOKEN").ok().as_deref())?;
    serde_json::to_string(&finished).map(|value| format!("{value}\n")).map_err(|error| format!("encode outcome: {error}"))
}

fn schedule_handoff334<R: maw_tmux::TmuxRunner>(runner: &mut R, run_id: &str, repo: &Path, maw: &Path) -> Result<(), String> {
    schedule_safe334(run_id, "run id")?;
    let maw = maw.to_str().filter(|value| value.starts_with('/') && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"/_+.-".contains(&byte)))
        .ok_or_else(|| "maw executable path is not shell-safe".to_owned())?;
    let session = format!("maw-schedule-{run_id}");
    let args = vec!["-d".into(), "-s".into(), session, "-n".into(), "fire".into(), "-c".into(), repo.to_string_lossy().into_owned(),
        format!("exec {maw} schedule exec {run_id}")];
    runner.run("new-session", &args).map(|_| ()).map_err(|error| format!("tmux handoff failed: {}", error.message))
}

fn schedule_id_force334(argv: &[String]) -> Result<(String, bool), String> {
    let (id, forced) = match argv { [id] => (id.clone(), false), [id, flag] if flag == "--force" => (id.clone(), true), _ => return Err(SCHEDULE_USAGE334.to_owned()) };
    schedule_safe334(&id, "job id")?; Ok((id, forced))
}
fn schedule_fire_args334(argv: &[String]) -> Result<(String, String, String, bool), String> {
    let (oracle, id, repo, forced) = match argv { [oracle, id, repo] => (oracle, id, repo, false), [oracle, id, repo, flag] if flag == "--force" => (oracle, id, repo, true), _ => return Err(SCHEDULE_USAGE334.to_owned()) };
    schedule_safe334(oracle, "oracle")?; schedule_safe334(id, "job id")?; Ok((oracle.clone(), id.clone(), repo.clone(), forced))
}
fn schedule_safe334(value: &str, name: &str) -> Result<(), String> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)) { Ok(()) } else { Err(format!("invalid {name}")) }
}
fn schedule_cadence_seconds334(job: &maw_schedule::Schedule) -> Result<u64, String> {
    match maw_schedule::plist::parse_cadence(job).map_err(|error| error.to_string())? {
        maw_schedule::plist::CadencePlan::Interval { seconds } => Ok(u64::from(seconds)),
        maw_schedule::plist::CadencePlan::Calendar(times) => u64::try_from(times.len()).ok().filter(|count| *count > 0).map(|count| 86_400 / count).ok_or_else(|| "empty cadence".to_owned()),
    }
}
fn schedule_local_time334() -> Result<(String, String), String> {
    let output = std::process::Command::new("/bin/date").arg("+%Y-%m-%d %H").output().map_err(|error| format!("date: {error}"))?;
    let text = String::from_utf8_lossy(&output.stdout); let mut parts = text.split_whitespace();
    match (output.status.success(), parts.next(), parts.next(), parts.next()) { (true, Some(day), Some(hour), None) => Ok((day.to_owned(), hour.to_owned())), _ => Err("date returned invalid local time".to_owned()) }
}
fn schedule_boot334() -> Result<String, String> {
    let output = std::process::Command::new("/usr/sbin/sysctl").args(["-n", "kern.boottime"]).output().map_err(|error| format!("boot identity: {error}"))?;
    let identity = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if output.status.success() && !identity.is_empty() { Ok(identity) } else { Err("boot identity unavailable".to_owned()) }
}
fn schedule_log334(path: &Path, message: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("create log dir: {error}"))?; }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).map_err(|error| format!("open log: {error}"))?;
    writeln!(file, "[{}] {message}", current_epoch_seconds()).map_err(|error| format!("write log: {error}"))
}

#[cfg(test)] mod schedule_tests334 {
    use super::*;
    #[derive(Default)] struct Fake { calls: Vec<(String, Vec<String>)> }
    impl maw_tmux::TmuxRunner for Fake { fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> { self.calls.push((subcommand.into(), args.to_vec())); Ok(String::new()) } }
    #[test] fn schedule_registers_and_rejects_bad_private_ids() {
        assert_eq!(DISPATCH_334[0].command, "schedule");
        let output = schedule_run_command334(&["exec".into(), "../bad".into()]); assert_eq!(output.code, 1); assert!(output.stderr.contains("invalid run id"));
    }
    #[test] fn headless_handoff_contains_only_safe_run_identity_not_prompt() {
        let mut fake = Fake::default(); schedule_handoff334(&mut fake, "odin-daily-1", Path::new("/tmp/repo"), Path::new("/opt/bin/maw-rs")).unwrap();
        assert_eq!(fake.calls[0].0, "new-session"); let joined = fake.calls[0].1.join(" ");
        assert!(joined.contains("exec /opt/bin/maw-rs schedule exec odin-daily-1")); assert!(!joined.contains("WHO Matrix"));
    }
    #[test] fn shell_fire_loads_config_reserves_executes_and_publishes_outcome() {
        let _lock = env_test_lock().lock().unwrap(); let _home = EnvVarRestore::capture("MAW_HOME");
        let root = std::env::temp_dir().join(format!("maw-schedule-cli-{}", std::process::id())); let repo = root.join("odin-oracle");
        let _ = std::fs::remove_dir_all(&root); std::fs::create_dir_all(repo.join(".maw")).unwrap(); std::env::set_var("MAW_HOME", root.join("home"));
        std::fs::write(repo.join(".maw/schedule.toml"), "[[schedule]]\nid='canary'\ncommand='printf ok > artifact'\ncadence='every 1h'\nexec='shell'\nexpected_output='artifact'\n").unwrap();
        let output = schedule_fire334(&["odin-oracle".into(), "canary".into(), repo.to_string_lossy().into_owned(), "--force".into()]).unwrap();
        assert!(output.contains("\"status\":\"succeeded\"")); assert_eq!(std::fs::read_to_string(repo.join("artifact")).unwrap(), "ok");
        assert!(root.join("home/schedule/runs/latest.json").is_file()); let _ = std::fs::remove_dir_all(root);
    }
}
