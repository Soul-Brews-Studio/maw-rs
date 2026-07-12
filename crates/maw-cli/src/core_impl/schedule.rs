const DISPATCH_334: &[DispatcherEntry] = &[DispatcherEntry { command: "schedule", handler: Handler::Sync(schedule_run_command334) }];
const SCHEDULE_USAGE334: &str = "usage: maw schedule add <id> <command> --every <cadence> [--at <time>] [--max-fires <N>] [--exec <mode>]\n       maw schedule ls | rm <id> | sync [--check|--dry-run] | run <id> [--force]\n       maw schedule peek|pause|resume <id> | logs <id> [-n N] | cost\nprivate: maw schedule fire <oracle> <id> <repo> [--force] | exec <run-id>";

fn schedule_run_command334(argv: &[String]) -> CliOutput {
    match schedule_dispatch334(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("schedule: {message}\n") },
    }
}

fn schedule_dispatch334(argv: &[String]) -> Result<String, (i32, String)> {
    match argv.first().map(String::as_str) {
        Some("add") => schedule_add334(&argv[1..]).map_err(|message| (1, message)),
        Some("ls") => schedule_ls334(&argv[1..]).map_err(|message| (1, message)),
        Some("rm") => schedule_rm334(&argv[1..]).map_err(|message| (1, message)),
        Some("sync") => schedule_sync334(&argv[1..]).map_err(|message| (1, message)),
        Some("peek") => schedule_peek334(&argv[1..]).map_err(|message| (1, message)),
        Some("pause") => schedule_toggle334(&argv[1..], false).map_err(|message| (1, message)),
        Some("resume") => schedule_toggle334(&argv[1..], true).map_err(|message| (1, message)),
        Some("logs") => schedule_logs334(&argv[1..]).map_err(|message| (1, message)),
        Some("cost") => schedule_cost334(&argv[1..]).map_err(|message| (1, message)),
        Some("run") => schedule_run334(&argv[1..]).map_err(|message| (1, message)),
        Some("fire") => schedule_fire334(&argv[1..]).map_err(|message| (1, message)),
        Some("exec") => schedule_exec334(&argv[1..]).map_err(|message| (1, message)),
        Some("--help" | "-h") | None => Ok(format!("{SCHEDULE_USAGE334}\n")),
        _ => Err((2, SCHEDULE_USAGE334.to_owned())),
    }
}

fn schedule_add334(argv: &[String]) -> Result<String, String> {
    let (id, command, cadence, minute, hour, cap, exec) = schedule_add_args334(argv)?;
    let repo = schedule_repo334()?; let path = repo.join(".maw/schedule.toml");
    let body = match std::fs::read_to_string(&path) { Ok(body) => body, Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(), Err(e) => return Err(format!("read {}: {e}", path.display())) };
    if !body.trim().is_empty() && maw_schedule::parse_schedule(&body).map_err(|e| format!("parse {}: {e}", path.display()))?.schedule.iter().any(|job| job.id == id) {
        return Err(format!("id '{id}' already exists in {}", path.display()));
    }
    let job = maw_schedule::Schedule { id: id.clone(), command, cadence, max_fires_per_day: cap, exec,
        expected_output: None, token_name: "t2".into(), created: Some(schedule_created334()?), at_minute: minute, at_hour: hour };
    maw_schedule::plist::parse_cadence(&job).map_err(|e| e.to_string())?;
    schedule_write_config334(&path, &schedule_append334(&body, &job)?)?;
    let result = schedule_sync_jobs334(&[repo], maw_schedule_launchd::SyncMode::Apply, false)?;
    Ok(format!("✓ Added {id} to {}\n{result}", path.display()))
}

fn schedule_rm334(argv: &[String]) -> Result<String, String> {
    let [id] = argv else { return Err(SCHEDULE_USAGE334.to_owned()); }; schedule_safe334(id, "job id")?;
    let repo = schedule_repo334()?; let path = repo.join(".maw/schedule.toml");
    let body = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let updated = schedule_remove334(&body, id)?; schedule_write_config334(&path, &updated)?;
    let label = schedule_label334(&repo, id)?; let plist = schedule_plist_root334()?.join(format!("{label}.plist"));
    let removed = maw_schedule_launchd::remove_job(&label, &plist, &schedule_domain334()?, maw_schedule_launchd::SyncMode::Apply, &mut maw_schedule_launchd::SystemLaunchctl)?;
    Ok(format!("✓ Removed {id} from TOML{}\n", if removed { " + launchd" } else { " (no launchd job)" }))
}

fn schedule_ls334(argv: &[String]) -> Result<String, String> {
    if !argv.is_empty() { return Err(SCHEDULE_USAGE334.to_owned()); }
    let repos = schedule_repos334(); let domain = schedule_domain334()?; let mut runner = maw_schedule_launchd::SystemLaunchctl;
    let mut rows = Vec::new();
    for repo in repos { for job in maw_schedule_launchd::load_config(&repo.join(".maw/schedule.toml"))?.schedule {
        let desired = schedule_desired334(&repo, &job)?;
        let state = maw_schedule_launchd::sync_job(&desired, &domain, maw_schedule_launchd::SyncMode::Check, &mut runner)?.before;
        rows.push((repo.file_name().unwrap_or_default().to_string_lossy().into_owned(), job, state.loaded));
    }}
    if rows.is_empty() { return Ok("(no schedules configured)\n".into()); }
    let mut out = format!("{:<22} {:<14} {:<14} {:>4} {:<30} {:>4} Loaded\n{}\n", "Oracle", "ID", "Cadence", "At", "Cmd", "Cap", "─".repeat(100));
    for (oracle, job, loaded) in rows { let at = job.at_minute.map_or("-".into(), |v| v.to_string());
        let _ = writeln!(out, "{oracle:<22} {:<14} {:<14} {at:>4} {:<30} {:>4} {}", job.id, job.cadence, job.command, job.max_fires_per_day, if loaded { "✓" } else { "✗" }); }
    Ok(out)
}

fn schedule_sync334(argv: &[String]) -> Result<String, String> {
    let mode = match argv { [] => maw_schedule_launchd::SyncMode::Apply,
        [flag] if flag == "--check" || flag == "--dry-run" => maw_schedule_launchd::SyncMode::Check,
        _ => return Err(SCHEDULE_USAGE334.to_owned()) };
    schedule_sync_jobs334(&schedule_repos334(), mode, true)
}

fn schedule_peek334(argv: &[String]) -> Result<String, String> {
    let [id] = argv else { return Err(SCHEDULE_USAGE334.to_owned()); }; let repo = schedule_repo334()?; let label = schedule_label334(&repo, id)?; let plist = schedule_plist_root334()?.join(format!("{label}.plist"));
    let mut out = format!("label:  {label}\nplist:  {}\n", plist.display()); if let Ok(xml) = std::fs::read_to_string(&plist) { let _ = write!(out, "\n── plist contents ──\n{xml}"); }
    let printed = std::process::Command::new("launchctl").args(["print", &format!("{}/{label}", schedule_domain334()?)]).output().map_err(|e| format!("launchctl print: {e}"))?;
    let _ = write!(out, "── launchctl print ──\n{}{}", String::from_utf8_lossy(&printed.stdout), String::from_utf8_lossy(&printed.stderr)); Ok(out)
}
fn schedule_toggle334(argv: &[String], enabled: bool) -> Result<String, String> {
    let [id] = argv else { return Err(SCHEDULE_USAGE334.to_owned()); }; let label = schedule_label334(&schedule_repo334()?, id)?; let target = format!("{}/{label}", schedule_domain334()?); let action = if enabled { "enable" } else { "disable" };
    let output = maw_schedule_launchd::LaunchctlRunner::run(&mut maw_schedule_launchd::SystemLaunchctl, &[action.into(), target])?;
    if output.success { Ok(format!("✓ {} {label}\n", if enabled { "resumed" } else { "paused" })) } else { Err(format!("launchctl {action} failed: {}", output.stderr)) }
}
fn schedule_logs334(argv: &[String]) -> Result<String, String> {
    let (id, count) = match argv { [id] => (id, 30), [id, flag, count] if flag == "-n" => (id, count.parse::<usize>().map_err(|_| "invalid log count".to_owned())?), _ => return Err(SCHEDULE_USAGE334.to_owned()) };
    let repo = schedule_repo334()?; let oracle = repo.file_name().and_then(|v| v.to_str()).ok_or_else(|| "repository has no oracle name".to_owned())?; schedule_safe334(id, "job id")?;
    let path = maw_state_dir(&current_xdg_env()).join("logs").join(format!("{oracle}.{id}.log")); let body = match std::fs::read_to_string(&path) { Ok(body) => body, Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(format!("(no log at {})\n", path.display())), Err(e) => return Err(format!("read {}: {e}", path.display())) };
    Ok(body.lines().rev().take(count).collect::<Vec<_>>().into_iter().rev().fold(String::new(), |mut out, line| { let _ = writeln!(out, "{line}"); out }))
}
fn schedule_cost334(argv: &[String]) -> Result<String, String> {
    if !argv.is_empty() { return Err(SCHEDULE_USAGE334.to_owned()); } let mut total = 0_u64; let mut out = String::new(); for repo in schedule_repos334() { let oracle = repo.file_name().unwrap_or_default().to_string_lossy();
        for job in maw_schedule_launchd::load_config(&repo.join(".maw/schedule.toml"))?.schedule { total += u64::from(job.max_fires_per_day); let _ = writeln!(out, "  {oracle}.{:<14}  max {:>3} fires/day", job.id, job.max_fires_per_day); } }
    let _ = write!(out, "\nTotal max fires/day across fleet: {total}\n(actual cost depends on per-fire Claude tokens; this is the upper-bound count)\n"); Ok(out)
}

type ScheduleAdd334 = (String, String, String, Option<u8>, Option<u8>, u32, maw_schedule::ExecMode);
fn schedule_add_args334(argv: &[String]) -> Result<ScheduleAdd334, String> {
    let [id, command, rest @ ..] = argv else { return Err(SCHEDULE_USAGE334.to_owned()); }; schedule_safe334(id, "job id")?;
    let (mut cadence, mut minute, mut hour, mut cap, mut exec) = (None, None, None, 24, maw_schedule::ExecMode::ClaudeHeadless);
    let mut index = 0; while index < rest.len() { let value = rest.get(index + 1).ok_or_else(|| SCHEDULE_USAGE334.to_owned())?;
        match rest[index].as_str() { "--every" => cadence = Some(if value.starts_with("every ") || value.starts_with("daily ") { value.clone() } else { format!("every {value}") }),
            "--at" => { let raw = value.trim_start_matches(':'); let parts = raw.split(':').collect::<Vec<_>>();
                if parts.len() == 1 { minute = Some(parts[0].parse().map_err(|_| "invalid --at minute".to_owned())?); }
                else if parts.len() == 2 { if !parts[0].is_empty() { hour = Some(parts[0].parse().map_err(|_| "invalid --at hour".to_owned())?); } minute = Some(parts[1].parse().map_err(|_| "invalid --at minute".to_owned())?); }
                else { return Err("invalid --at value".to_owned()); } },
            "--max-fires" => cap = value.parse::<u32>().map_err(|_| "invalid --max-fires".to_owned())?,
            "--exec" if value == "shell" => exec = maw_schedule::ExecMode::Shell,
            "--exec" if value == "claude-headless" => exec = maw_schedule::ExecMode::ClaudeHeadless,
            "--exec" => return Err("--exec must be shell or claude-headless".to_owned()), _ => return Err(SCHEDULE_USAGE334.to_owned()) } index += 2; }
    Ok((id.clone(), command.clone(), cadence.ok_or_else(|| "--every is required".to_owned())?, minute, hour, cap, exec))
}

fn schedule_append334(body: &str, job: &maw_schedule::Schedule) -> Result<String, String> {
    let quote = |value: &str| serde_json::to_string(value).map_err(|e| e.to_string()); let mut out = body.to_owned();
    if out.trim().is_empty() { out = "# Generated by maw-schedule. Edit by hand or via `maw-schedule add/rm`.\n".into(); }
    if !out.ends_with('\n') { out.push('\n'); }
    if !out.ends_with("\n\n") { out.push('\n'); }
    let mode = if job.exec == maw_schedule::ExecMode::Shell { "shell" } else { "claude-headless" };
    let _ = write!(out, "[[schedule]]\nid = {}\ncommand = {}\ncadence = {}\nmax_fires_per_day = {}\nexec = {}\n", quote(&job.id)?, quote(&job.command)?, quote(&job.cadence)?, job.max_fires_per_day, quote(mode)?);
    if let Some(created) = &job.created { let _ = writeln!(out, "created = {}", quote(created)?); }
    if let Some(hour) = job.at_hour { let _ = writeln!(out, "at_hour = {hour}"); }
    if let Some(minute) = job.at_minute { let _ = writeln!(out, "at_minute = {minute}"); }
    out.push('\n'); Ok(out)
}

fn schedule_remove334(body: &str, id: &str) -> Result<String, String> {
    let mut starts = Vec::new(); let mut offset = 0; for line in body.split_inclusive('\n') { if line.trim() == "[[schedule]]" { starts.push(offset); } offset += line.len(); }
    let mut found = Vec::new(); for (index, start) in starts.iter().copied().enumerate() { let end = starts.get(index + 1).copied().unwrap_or(body.len());
        if maw_schedule::parse_schedule(&body[start..end]).map_err(|e| format!("parse schedule block: {e}"))?.schedule.first().is_some_and(|job| job.id == id) { found.push((start, end)); } }
    match found.as_slice() { [] => Err(format!("id '{id}' not found")), [(start, end)] => { let start = if body[..*start].ends_with("\n\n") { start - 1 } else { *start }; let mut out = body.to_owned(); out.replace_range(start..*end, ""); Ok(out) }, _ => Err(format!("duplicate id '{id}'")) }
}

fn schedule_write_config334(path: &Path, body: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "schedule path has no parent".to_owned())?; std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
    let temp = path.with_extension(format!("toml.{}.tmp", std::process::id())); std::fs::write(&temp, body).map_err(|e| format!("write {}: {e}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|e| format!("replace {}: {e}", path.display()))
}

fn schedule_repo334() -> Result<std::path::PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|e| format!("current directory: {e}"))?;
    cwd.ancestors().find(|path| path.join(".git").exists()).map(std::path::Path::to_path_buf).ok_or_else(|| "not inside a repository".to_owned())
}
fn schedule_repos334() -> Vec<std::path::PathBuf> {
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from); let mut roots = vec![std::path::PathBuf::from("/opt/Code/github.com")]; if let Some(home) = home { roots.push(home.join("Code")); }
    let mut repos = Vec::new(); for root in roots { let Ok(orgs) = std::fs::read_dir(root) else { continue }; for org in orgs.flatten() { let Ok(entries) = std::fs::read_dir(org.path()) else { continue };
        for repo in entries.flatten() { if repo.path().join(".maw/schedule.toml").is_file() { repos.push(repo.path()); } } } } repos.sort(); repos
}

fn schedule_label334(repo: &Path, id: &str) -> Result<String, String> {
    let oracle = repo.file_name().and_then(|v| v.to_str()).ok_or_else(|| "repository has no oracle name".to_owned())?; schedule_safe334(oracle, "oracle")?; schedule_safe334(id, "job id")?; Ok(format!("com.maw.schedule.{oracle}.{id}"))
}
fn schedule_plist_root334() -> Result<std::path::PathBuf, String> { std::env::var_os("HOME").map(std::path::PathBuf::from).map(|home| home.join("Library/LaunchAgents")).ok_or_else(|| "HOME is not set".to_owned()) }
fn schedule_domain334() -> Result<String, String> { let output = std::process::Command::new("/usr/bin/id").arg("-u").output().map_err(|e| format!("id: {e}"))?;
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned(); if output.status.success() && uid.bytes().all(|b| b.is_ascii_digit()) { Ok(format!("gui/{uid}")) } else { Err("user id unavailable".to_owned()) } }
fn schedule_created334() -> Result<String, String> { let output = std::process::Command::new("/bin/date").env("TZ", "Asia/Bangkok").arg("+%Y-%m-%dT%H:%M:%S%z").output().map_err(|e| format!("date: {e}"))?;
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_owned(); if !output.status.success() || raw.len() < 5 { return Err("date returned invalid timestamp".to_owned()); } let split = raw.len() - 2; Ok(format!("{}:{}", &raw[..split], &raw[split..])) }
fn schedule_desired334(repo: &Path, job: &maw_schedule::Schedule) -> Result<maw_schedule_launchd::DesiredJob, String> {
    let label = schedule_label334(repo, &job.id)?; let home = std::env::var("HOME").map_err(|_| "HOME is not set".to_owned())?; let log = maw_state_dir(&current_xdg_env()).join("logs").join(format!("{}.{}.log", repo.file_name().unwrap_or_default().to_string_lossy(), job.id));
    let model = maw_schedule::plist::LaunchdPlist { label: label.clone(), program_arguments: vec![std::env::current_exe().map_err(|e| format!("maw executable: {e}"))?.to_string_lossy().into_owned(), "schedule".into(), "fire".into(), repo.file_name().unwrap_or_default().to_string_lossy().into_owned(), job.id.clone(), repo.to_string_lossy().into_owned()],
        cadence: maw_schedule::plist::parse_cadence(job).map_err(|e| e.to_string())?, standard_out_path: log.to_string_lossy().into_owned(), standard_error_path: log.to_string_lossy().into_owned(), home, path: "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin".into(), run_at_load: false };
    Ok(maw_schedule_launchd::DesiredJob { plist_path: schedule_plist_root334()?.join(format!("{label}.plist")), label, xml: maw_schedule::plist::render_plist(&model) })
}

fn schedule_sync_jobs334(repos: &[std::path::PathBuf], mode: maw_schedule_launchd::SyncMode, prune: bool) -> Result<String, String> {
    let domain = schedule_domain334()?; let mut runner = maw_schedule_launchd::SystemLaunchctl; let mut desired = std::collections::BTreeSet::new(); let (mut changed, mut current) = (0, 0);
    for repo in repos { for job in maw_schedule_launchd::load_config(&repo.join(".maw/schedule.toml"))?.schedule { let target = schedule_desired334(repo, &job)?; desired.insert(target.label.clone());
        let result = maw_schedule_launchd::sync_job(&target, &domain, mode, &mut runner)?; if result.changed || !result.before.is_healthy() { changed += 1; } else { current += 1; } } }
    let mut stale = 0; if prune { if let Ok(entries) = std::fs::read_dir(schedule_plist_root334()?) { for entry in entries.flatten() { let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(label) = name.strip_suffix(".plist").filter(|label| label.starts_with("com.maw.schedule.") && !desired.contains(*label)) { if maw_schedule_launchd::remove_job(label, &entry.path(), &domain, mode, &mut runner)? { stale += 1; } } } } }
    Ok(format!("{changed} changed · {current} current · {stale} stale{}\n", if mode == maw_schedule_launchd::SyncMode::Check { " (check only)" } else { "" }))
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
        assert!(schedule_run_command334(&["--help".into()]).stdout.contains("schedule add"));
    }
    #[test] fn management_config_append_remove_round_trips_existing_bytes() {
        let original = "# keep this operator comment\nunknown = 'root-key'\n";
        let args = vec!["daily".into(), "printf \"$x\"\nnext".into(), "--every".into(), "1h".into(), "--at".into(), ":07".into(), "--max-fires".into(), "3".into(), "--exec".into(), "shell".into()];
        let (id, command, cadence, minute, hour, cap, exec) = schedule_add_args334(&args).unwrap();
        let job = maw_schedule::Schedule { id, command, cadence, max_fires_per_day: cap, exec, expected_output: None, token_name: "t2".into(), created: None, at_minute: minute, at_hour: hour };
        let added = schedule_append334(original, &job).unwrap(); let parsed = maw_schedule::parse_schedule(&added).unwrap();
        assert_eq!(parsed.schedule[0], job); assert!(added.starts_with(original)); assert_eq!(schedule_remove334(&added, "daily").unwrap(), original);
    }
    #[test] fn management_rejects_duplicates_and_invalid_cadence() {
        let body = "[[schedule]]\nid='same'\ncommand='one'\ncadence='every 1h'\n\n[[schedule]]\nid='same'\ncommand='two'\ncadence='every 1h'\n";
        assert!(schedule_remove334(body, "same").unwrap_err().contains("duplicate"));
        let args = vec!["bad".into(), "cmd".into(), "--every".into(), "25h".into()]; let (_, command, cadence, minute, hour, cap, exec) = schedule_add_args334(&args).unwrap();
        let job = maw_schedule::Schedule { id: "bad".into(), command, cadence, max_fires_per_day: cap, exec, expected_output: None, token_name: "t2".into(), created: None, at_minute: minute, at_hour: hour };
        assert!(maw_schedule::plist::parse_cadence(&job).is_err());
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
