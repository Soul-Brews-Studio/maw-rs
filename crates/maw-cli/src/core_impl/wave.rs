const DISPATCH_326: &[DispatcherEntry] = &[DispatcherEntry { command: "wave", handler: Handler::Sync(wave_run_command) }];

const WAVE_USAGE: &str = "usage: maw wave <start|status|dispatch|teardown> ...";
const WAVE_BASE_REF: &str = "origin/alpha";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaveState325 {
    team: String,
    session: String,
    mission_dir: String,
    members: Vec<WaveMember325>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaveMember325 {
    role: String,
    worktree: String,
    branch: String,
}

fn wave_run_command(argv: &[String]) -> CliOutput {
    if wants_help_before_positionals(argv, &[]) { return help_output(WAVE_USAGE); }
    match wave_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) if message == WAVE_USAGE => help_output(WAVE_USAGE),
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("wave: {message}\n") },
    }
}

fn wave_run(argv: &[String]) -> Result<String, String> {
    match argv.first().map_or("status", String::as_str) {
        "help" | "--help" | "-h" => Err(WAVE_USAGE.to_owned()),
        "start" => wave_start(argv),
        "status" => wave_status(argv),
        "dispatch" => wave_dispatch(argv),
        "teardown" | "down" => wave_teardown(argv),
        other => Err(format!("unknown wave subcommand {other}\n{WAVE_USAGE}")),
    }
}

fn wave_start(argv: &[String]) -> Result<String, String> {
    let (team, coders) = wave_parse_start(argv)?;
    let root = wave_primary_root()?;
    let state = wave_state_for(&team, coders, &root);
    for member in &state.members {
        if !root.join(&member.worktree).exists() {
            worktree_run_with(&wave_strings(&["add", &member.worktree_name(), "--base", WAVE_BASE_REF]), &mut WorktreeSystemRuntime).map_err(|(_, message)| message)?;
        }
    }
    wave_write_state(&state)?;
    let charter = wave_charter(&state);
    wave_ensure_session(&state.session, &root)?;
    wave_at_root(&root, || {
        let opts = TeamT3Options124 { team: Some(team.clone()), session: Some(state.session.clone()), engine: Some("codex".to_owned()), ..Default::default() };
        let mut out = team_t5b_exec_up(&charter, &opts)?;
        let heals = wave_heal(&state, &charter)?;
        let _ = writeln!(out, "wave start: {team} coders={}", state.members.len());
        for heal in heals { let _ = writeln!(out, "heal\t{heal}"); }
        Ok(out)
    })
}

fn wave_status(argv: &[String]) -> Result<String, String> {
    let Some(team) = argv.get(1) else { return Ok(wave_list()); };
    team_validate_name(team)?;
    let state = wave_read_state(team)?;
    let rows = wave_rows(&state);
    let mut out = format!("wave status: {team}\nrole\thealth\tworktree\tpane\n");
    for (member, health, pane) in rows {
        let _ = writeln!(out, "{}\t{}\t{}\t{}", member.role, health, member.worktree, pane.unwrap_or_else(|| "-".to_owned()));
    }
    Ok(out)
}

fn wave_dispatch(argv: &[String]) -> Result<String, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw wave dispatch <team> <task-description>".to_owned())?;
    team_validate_name(team)?;
    let task = argv.iter().skip(2).map(String::as_str).collect::<Vec<_>>().join(" ");
    team_validate_message(&task)?;
    let state = wave_read_state(team)?;
    let rows = wave_rows(&state);
    let (member, _, pane) = rows.into_iter().find(|(_, health, pane)| *health == "idle" && pane.is_some()).ok_or_else(|| format!("no idle coder in wave '{team}'"))?;
    let pane = pane.expect("pane checked");
    let mission = std::path::Path::new(&state.mission_dir).join(format!("{}.md", member.role));
    let body = format!("# Mission: {}\n\n{}\n\n## ACK contract\nReply ACK starting, ACK blocked:<reason>, or ACK done with evidence.\n", member.role, task);
    team_atomic_write_0600(&mission, &body)?;
    let pointer = format!("Mission for {}: {} — ACK starting/blocked/done.", member.role, mission.display());
    wave_send_text(&pane, &pointer)?;
    Ok(format!("wave dispatch: {team} -> {}\nmission: {}\n", member.role, mission.display()))
}

fn wave_teardown(argv: &[String]) -> Result<String, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw wave teardown <team>".to_owned())?;
    team_validate_name(team)?;
    let state = wave_read_state(team)?;
    let root = wave_primary_root()?;
    let mut out = format!("wave teardown: {team}\n");
    for member in &state.members {
        if let Some(window) = wave_member_window(&state.session, &member.role) { wave_kill_window(&state.session, &window, &mut out); }
        let src = root.join(&member.worktree);
        if src.exists() {
            let dst = std::env::temp_dir().join(format!("maw-wave-{team}-{}-{}", member.role, team_now_millis()));
            match std::fs::rename(&src, &dst) {
                Ok(()) => { let _ = writeln!(out, "moved\t{}\t{}", member.worktree, dst.display()); }
                Err(error) => { let _ = writeln!(out, "kept\t{}\tmove failed: {error}", member.worktree); }
            }
        }
    }
    let _ = wave_git(&root, &["worktree", "prune"]);
    for member in &state.members {
        match wave_git(&root, &["branch", "-d", &member.branch]) {
            Ok(_) => { let _ = writeln!(out, "branch-deleted\t{}", member.branch); }
            Err(error) => { let _ = writeln!(out, "branch-kept\t{}\t{}", member.branch, error.trim()); }
        }
    }
    Ok(out)
}

fn wave_parse_start(argv: &[String]) -> Result<(String, usize), String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw wave start <team> [--coders N]".to_owned())?.clone();
    team_validate_name(&team)?;
    let mut coders = 1usize;
    let mut index = 2;
    while index < argv.len() {
        match argv[index].as_str() {
            "--coders" => { index += 1; coders = argv.get(index).ok_or_else(|| "wave start: --coders requires a value".to_owned())?.parse().map_err(|_| "wave start: --coders must be a positive integer".to_owned())?; },
            value => return Err(format!("wave start: unknown argument {value}")),
        }
        index += 1;
    }
    if coders == 0 { return Err("wave start: --coders must be positive".to_owned()); }
    Ok((team, coders))
}

fn wave_primary_root() -> Result<std::path::PathBuf, String> {
    let mut runtime = WorktreeSystemRuntime;
    worktree_list_records(&mut runtime)?.first().map(|record| record.path.clone()).ok_or_else(|| "wave: no git worktrees found".to_owned())
}

fn wave_at_root<T>(root: &std::path::Path, run: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
    let previous = std::env::current_dir().map_err(|error| error.to_string())?;
    std::env::set_current_dir(root).map_err(|error| format!("wave: chdir {}: {error}", root.display()))?;
    let result = run();
    match (result, std::env::set_current_dir(previous).map_err(|error| format!("wave: restore cwd: {error}"))) { (Ok(value), Ok(())) => Ok(value), (Err(error), _) | (Ok(_), Err(error)) => Err(error) }
}

fn wave_state_for(team: &str, coders: usize, root: &std::path::Path) -> WaveState325 {
    let members = (1..=coders).map(|number| {
        let role = format!("coder-{number}");
        let slug = format!("{team}-{role}");
        WaveMember325 { role, worktree: format!("agents/{slug}"), branch: format!("agents/{slug}") }
    }).collect();
    WaveState325 { team: team.to_owned(), session: team.to_owned(), mission_dir: root.join("ψ").join("missions").join(team).display().to_string(), members }
}

impl WaveMember325 {
    fn worktree_name(&self) -> String { self.worktree.rsplit('/').next().unwrap_or(&self.worktree).to_owned() }
}

fn wave_charter(state: &WaveState325) -> TeamCharter122 {
    TeamCharter122 { name: state.team.clone(), session: Some(state.session.clone()), members: state.members.iter().map(|member| TeamCharterMember122 { role: member.role.clone(), name: Some(member.role.clone()), engine: Some("codex".to_owned()), worktree: Some(member.worktree.clone()), branch: Some(member.branch.clone()), ..Default::default() }).collect(), ..Default::default() }
}

fn wave_state_path(team: &str) -> std::path::PathBuf { team_state_dir().join("waves").join(format!("{team}.json")) }
fn wave_write_state(state: &WaveState325) -> Result<(), String> { team_write_json_atomic_0600(&wave_state_path(&state.team), state) }
fn wave_read_state(team: &str) -> Result<WaveState325, String> { team_read_json(&wave_state_path(team)).ok_or_else(|| format!("wave '{team}' not found; run maw wave start {team}")) }

fn wave_list() -> String {
    let dir = team_state_dir().join("waves");
    let mut names = std::fs::read_dir(dir).map_or_else(|_| Vec::new(), |entries| entries.flatten().filter_map(|entry| entry.path().file_stem().and_then(std::ffi::OsStr::to_str).map(str::to_owned)).collect::<Vec<_>>());
    names.sort();
    if names.is_empty() { "wave status: no waves\n".to_owned() } else { format!("wave status: {}\n", names.join(", ")) }
}

fn wave_ensure_session(session: &str, root: &std::path::Path) -> Result<(), String> {
    if std::env::var_os("MAW_RS_TEAM_FAKE_TMUX_LOG").is_some() { return Ok(()); }
    let mut runner = TeamT5bTmuxRunner128::new();
    if runner.run(&wave_strings(&["has-session", "-t", &format!("={session}")])).is_ok() { return Ok(()); }
    runner.run(&wave_strings(&["new-session", "-d", "-s", session, "-n", "lead", "-c", &root.display().to_string()])).map(|_| ())
}

fn wave_heal(state: &WaveState325, charter: &TeamCharter122) -> Result<Vec<String>, String> {
    let panes = team_down_panes().unwrap_or_default();
    let roster: Vec<_> = charter.members.iter().map(|member| team_t3_classify(member, &TeamT3Options124::default(), &state.session, &panes)).collect();
    let mut healed = Vec::new();
    for item in roster {
        let Some(pane) = item.pane else { continue; };
        if !team_t3_is_live_command(&pane.command) {
            wave_send_raw(&pane.pane_id, &format!("maw wake {} --no-attach --session {} -e codex --repo-path {}", item.identity, state.session, item.worktree))?;
            healed.push(format!("{} relaunch", item.role));
        }
        if wave_capture(&pane.pane_id).is_some_and(|text| text.contains("[Y/n]") || text.contains("Update available")) {
            wave_send_enter(&pane.pane_id)?;
            healed.push(format!("{} update-prompt", item.role));
        }
    }
    Ok(healed)
}

fn wave_rows(state: &WaveState325) -> Vec<(WaveMember325, &'static str, Option<String>)> {
    let panes = team_down_panes().unwrap_or_default();
    state.members.iter().map(|member| {
        let pane = panes.iter().find(|pane| pane.session == state.session && pane.window == member.role);
        let health = pane.map_or("stalled", |pane| {
            if !team_t3_is_live_command(&pane.command) { "stalled" }
            else if wave_capture(&pane.pane_id).is_some_and(|text| text.contains("Working") || text.contains("tokens")) { "working" }
            else { "idle" }
        });
        (member.clone(), health, pane.map(|pane| pane.pane_id.clone()))
    }).collect()
}

fn wave_member_window(session: &str, role: &str) -> Option<String> {
    team_down_panes().ok()?.into_iter().find(|pane| pane.session == session && pane.window == role).map(|pane| pane.window)
}

fn wave_send_text(target: &str, text: &str) -> Result<(), String> {
    let mut runner = maw_tmux::CommandTmuxRunner::new();
    sendtext_send_text(&mut runner, target, text, std::thread::sleep).map(|_| ()).map_err(|error| error.message)
}

fn wave_send_raw(target: &str, text: &str) -> Result<(), String> { wave_send_text(target, text) }
fn wave_send_enter(target: &str) -> Result<(), String> { let mut runner = maw_tmux::CommandTmuxRunner::new(); maw_tmux::TmuxRunner::run(&mut runner, "send-keys", &maw_tmux::tmux_send_enter_args(target)).map(|_| ()).map_err(|error| error.message) }

fn wave_capture(target: &str) -> Option<String> {
    let mut runner = maw_tmux::CommandTmuxRunner::new();
    maw_tmux::TmuxRunner::run(&mut runner, "capture-pane", &["-t".to_owned(), target.to_owned(), "-p".to_owned(), "-S".to_owned(), "-80".to_owned()]).ok()
}

fn wave_kill_window(session: &str, window: &str, out: &mut String) {
    let mut runner = maw_tmux::CommandTmuxRunner::new();
    match maw_tmux::TmuxRunner::run(&mut runner, "kill-window", &["-t".to_owned(), format!("{session}:{window}")]) {
        Ok(_) => { let _ = writeln!(out, "killed\t{session}:{window}"); }
        Err(error) => { let _ = writeln!(out, "kept-window\t{session}:{window}\t{}", error.message); }
    }
}

fn wave_git(root: &std::path::Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("git").arg("-C").arg(root).args(args).output().map_err(|error| error.to_string())?;
    if output.status.success() { Ok(String::from_utf8_lossy(&output.stdout).into_owned()) } else { Err(String::from_utf8_lossy(&output.stderr).into_owned()) }
}

fn wave_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }
