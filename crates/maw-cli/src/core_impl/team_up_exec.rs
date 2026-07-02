const DISPATCH_128: &[DispatcherEntry] = &[];

fn team_t5b_up(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team up <team> [--session <name>] [--members <roles>] [--only <a,b>] [--dry-run] [--status] [-e <engine>]")?;
    let charter = team_t3_load_or_quick_charter(&opts)?;
    team_t5b_validate_charter_members(&charter)?;
    if team_t3_has(&opts, TEAM_T3_STATUS) || team_t3_has(&opts, TEAM_T3_DRY_RUN) { return Ok(team_t3_render_up(&charter, &opts)); }
    team_t5b_exec_up(&charter, &opts)
}

fn team_t5b_bring(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team bring <team> [--session <session>] [--split] [--gather] [--dry-run] [-e <engine>]")?;
    let team = opts.team.as_ref().ok_or_else(|| "usage: maw team bring <team> [--session <session>] [--split] [--gather] [--dry-run] [-e <engine>]".to_owned())?;
    team_validate_name(team)?;
    if team_t3_has(&opts, TEAM_T3_DRY_RUN) { return Ok(team_t3_render_bring(team, &opts)); }
    team_t5b_exec_bring(team, &opts)
}

fn team_t5b_apply(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team apply <team|team.yaml> [--charter <path>] [--session <name>] [--apply]")?;
    let charter = team_t3_load_apply_charter(&opts)?;
    team_t5b_validate_charter_members(&charter)?;
    if !team_t3_has(&opts, TEAM_T3_APPLY) { return Ok(team_t3_render_apply(&charter, &opts)); }
    team_t5b_exec_apply(&charter, &opts)
}

fn team_t5b_exec_up(charter: &TeamCharter122, opts: &TeamT3Options124) -> Result<String, String> {
    let session = team_t3_session(charter, opts);
    team_t5b_validate_session(&session)?;
    let roster = team_t3_roster(charter, opts, &session, team_t3_up_action);
    let mut runner = TeamT5bTmuxRunner128::new();
    let mut actions = Vec::new();
    for item in &roster {
        if item.state == "skipped" || item.state == "live" { actions.push(team_t5b_action(item, "skip")); continue; }
        if team_t3_has(opts, TEAM_T3_FORCE) { team_t5b_kill_window(&mut runner, item, &session)?; }
        if item.state == "dead" && !team_t3_has(opts, TEAM_T3_FORCE) { team_t5b_resume_pane(&mut runner, item, opts, &session)?; actions.push(team_t5b_action(item, "resume in place")); }
        else { team_t5b_wake_window(&mut runner, item, opts, &session)?; actions.push(team_t5b_action(item, "fresh wake")); }
    }
    if team_t3_has(opts, TEAM_T3_GATHER) { team_t5b_gather(&mut runner, &roster, &session)?; actions.push("*\tlive\tgather main-vertical".to_owned()); }
    Ok(team_t5b_render_exec("team up", &charter.name, &session, &actions))
}

fn team_t5b_exec_bring(team: &str, opts: &TeamT3Options124) -> Result<String, String> {
    let session = opts.session.clone().unwrap_or_else(|| team.to_owned());
    team_t5b_validate_session(&session)?;
    let mut runner = TeamT5bTmuxRunner128::new();
    let mut actions = Vec::new();
    for oracle in team_message_targets(team) {
        team_t5b_validate_member(&oracle)?;
        let item = TeamRosterItem124 { role: oracle.clone(), identity: oracle.clone(), engine: opts.engine.clone().unwrap_or_else(|| "claude".to_owned()), worktree: oracle.clone(), state: "missing".to_owned(), action: String::new(), pane: None };
        team_t5b_wake_window(&mut runner, &item, opts, &session)?;
        actions.push(format!("{oracle}\tmissing\twake"));
    }
    Ok(team_t5b_render_exec("team bring", team, &session, &actions))
}

fn team_t5b_exec_apply(charter: &TeamCharter122, opts: &TeamT3Options124) -> Result<String, String> {
    let session = team_t3_session(charter, opts);
    team_t5b_validate_session(&session)?;
    let roster = team_t3_roster(charter, opts, &session, |item, _| match item.state.as_str() { "missing" => "spawn member".to_owned(), "live" => "skip live".to_owned(), "dead" => "skip dead member (team up can resume)".to_owned(), _ => "skip".to_owned() });
    let mut runner = TeamT5bTmuxRunner128::new();
    let mut actions = Vec::new();
    for item in &roster {
        if item.state == "missing" { team_t5b_wake_window(&mut runner, item, opts, &session)?; actions.push(team_t5b_action(item, "spawn member")); }
        else { actions.push(team_t5b_action(item, &item.action)); }
    }
    Ok(team_t5b_render_exec("team apply", &charter.name, &session, &actions))
}

#[derive(Debug, Clone)]
struct TeamT5bTmuxRunner128 { log: Option<std::path::PathBuf> }

impl TeamT5bTmuxRunner128 {
    fn new() -> Self { Self { log: std::env::var_os("MAW_RS_TEAM_FAKE_TMUX_LOG").map(std::path::PathBuf::from) } }
    fn run(&mut self, args: &[String]) -> Result<String, String> {
        if let Some(path) = &self.log {
            let mut body = std::fs::read_to_string(path).unwrap_or_default();
            body.push_str(&(serde_json::json!({"program":"tmux","args":args}).to_string() + "\n"));
            return team_atomic_write_0600(path, &body).map(|()| String::new());
        }
        let out = std::process::Command::new("tmux").args(args).output().map_err(|error| format!("team tmux failed: {error}"))?;
        if out.status.success() { Ok(String::from_utf8_lossy(&out.stdout).to_string()) } else { Err(String::from_utf8_lossy(&out.stderr).trim().to_owned()) }
    }
}

fn team_t5b_wake_window(runner: &mut TeamT5bTmuxRunner128, item: &TeamRosterItem124, opts: &TeamT3Options124, session: &str) -> Result<(), String> {
    team_t5b_validate_item(item)?;
    let target = format!("{session}:{}", item.identity);
    runner.run(&team_t5b_strings(&["new-window", "-t", session, "-n", &item.identity]))?;
    team_t5b_send_fixed_maw(runner, &target, &team_t5b_maw_wake_args(item, opts, session)?)
}

fn team_t5b_resume_pane(runner: &mut TeamT5bTmuxRunner128, item: &TeamRosterItem124, opts: &TeamT3Options124, session: &str) -> Result<(), String> {
    team_t5b_validate_item(item)?;
    let pane = item.pane.as_ref().ok_or_else(|| "team resume: missing pane".to_owned())?;
    team_t5b_validate_pane_id(&pane.pane_id)?;
    runner.run(&team_t5b_strings(&["send-keys", "-t", &pane.pane_id, "C-u"]))?;
    team_t5b_send_fixed_maw(runner, &pane.pane_id, &team_t5b_maw_resume_args(item, opts, session)?)
}

fn team_t5b_send_fixed_maw(runner: &mut TeamT5bTmuxRunner128, target: &str, args: &[String]) -> Result<(), String> {
    let command = team_t5b_shell_command(args)?;
    runner.run(&team_t5b_strings(&["send-keys", "-t", target, "-l", "--", &command]))?;
    runner.run(&team_t5b_strings(&["send-keys", "-t", target, "Enter"]))?;
    Ok(())
}

fn team_t5b_kill_window(runner: &mut TeamT5bTmuxRunner128, item: &TeamRosterItem124, session: &str) -> Result<(), String> {
    if let Some(pane) = &item.pane { team_t5b_validate_window(&pane.window)?; runner.run(&team_t5b_strings(&["kill-window", "-t", &format!("{}:{}", pane.session, pane.window)]))?; }
    else { let _ = session; }
    Ok(())
}

fn team_t5b_gather(runner: &mut TeamT5bTmuxRunner128, roster: &[TeamRosterItem124], session: &str) -> Result<(), String> {
    for item in roster.iter().filter(|item| item.state == "live") {
        if let Some(pane) = &item.pane { team_t5b_validate_pane_id(&pane.pane_id)?; runner.run(&team_t5b_strings(&["join-pane", "-s", &pane.pane_id, "-t", session]))?; }
    }
    runner.run(&team_t5b_strings(&["select-layout", "main-vertical"]))?;
    Ok(())
}

fn team_t5b_maw_wake_args(item: &TeamRosterItem124, opts: &TeamT3Options124, session: &str) -> Result<Vec<String>, String> {
    let repo = team_t5b_bound_worktree(&item.worktree)?;
    let engine = opts.engine.clone().unwrap_or_else(|| item.engine.clone());
    team_t5b_validate_member(&engine)?;
    Ok(vec!["wake".to_owned(), item.identity.clone(), "--no-attach".to_owned(), "--session".to_owned(), session.to_owned(), "-e".to_owned(), engine, "--repo-path".to_owned(), repo.display().to_string()])
}

fn team_t5b_maw_resume_args(item: &TeamRosterItem124, opts: &TeamT3Options124, session: &str) -> Result<Vec<String>, String> {
    let mut args = team_t5b_maw_wake_args(item, opts, session)?;
    args.push("--resume".to_owned());
    Ok(args)
}

fn team_t5b_shell_command(args: &[String]) -> Result<String, String> {
    let mut parts = vec![team_t5b_shell_quote(&team_t5b_self_bin()?.display().to_string())];
    parts.extend(args.iter().map(|arg| team_t5b_shell_quote(arg)));
    Ok(parts.join(" "))
}

fn team_t5b_shell_quote(value: &str) -> String { format!("'{}'", value.replace('\'', "'\\''")) }

fn team_t5b_self_bin() -> Result<std::path::PathBuf, String> {
    std::env::var_os("MAW_RS_SELF_BIN").map(std::path::PathBuf::from).map_or_else(|| std::env::current_exe().map_err(|error| format!("team: current_exe failed: {error}")), Ok)
}

fn team_t5b_bound_worktree(path: &str) -> Result<std::path::PathBuf, String> { team_t5_canonical_work_path(path) }

fn team_t5b_validate_charter_members(charter: &TeamCharter122) -> Result<(), String> {
    for member in &charter.members {
        team_t5b_validate_member(&member.role)?;
        if let Some(name) = &member.name {
            team_t5b_validate_member(name)?;
        }
        if let Some(engine) = &member.engine {
            team_t5b_validate_member(engine)?;
        }
        if let Some(model) = &member.model {
            team_t5b_validate_member(model)?;
        }
        if let Some(cwd) = &member.cwd {
            team_t5_validate_work_path(cwd)?;
        }
    }
    Ok(())
}

fn team_t5b_validate_item(item: &TeamRosterItem124) -> Result<(), String> { team_t5b_validate_member(&item.role)?; team_t5b_validate_member(&item.identity)?; team_t5b_validate_member(&item.engine)?; team_t5_validate_work_path(&item.worktree)?; if let Some(pane) = &item.pane { team_t5b_validate_session(&pane.session)?; team_t5b_validate_window(&pane.window)?; team_t5b_validate_pane_id(&pane.pane_id)?; } Ok(()) }

fn team_t5b_validate_member(value: &str) -> Result<(), String> {
    if value.is_empty() { return Err("team member is empty".to_owned()); }
    if value.starts_with('-') { return Err(format!("invalid team member '{value}': leading dash rejected")); }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) { return Err(format!("invalid team member '{value}': metacharacter rejected")); }
    Ok(())
}

fn team_t5b_validate_session(value: &str) -> Result<(), String> { team_t3_validate_session(value) }

fn team_t5b_validate_window(value: &str) -> Result<(), String> { team_t5b_validate_member(value) }

fn team_t5b_validate_pane_id(value: &str) -> Result<(), String> {
    if value.is_empty() || !value.starts_with('%') || !value[1..].chars().all(|ch| ch.is_ascii_digit()) { return Err(format!("invalid tmux pane id '{value}'")); }
    Ok(())
}

fn team_t5b_action(item: &TeamRosterItem124, action: &str) -> String { format!("{}\t{}\t{}", item.role, item.state, action) }

fn team_t5b_render_exec(kind: &str, team: &str, session: &str, actions: &[String]) -> String {
    let mut out = format!("{kind}: {team} ({session}) execute\nrole\tstate\taction\n");
    for action in actions { out.push_str(action); out.push('\n'); }
    out
}

fn team_t5b_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

#[cfg(test)]
mod team_t5b_tests {
    use super::*;

    #[test]
    fn team_t5b_shell_quote_escapes_embedded_single_quote() {
        assert_eq!(team_t5b_shell_quote("builder'one"), "'builder'\\''one'");
        assert_eq!(team_t5b_shell_quote("plain"), "'plain'");
    }
}
