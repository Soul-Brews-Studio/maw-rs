const DISPATCH_132: &[DispatcherEntry] = &[];

#[derive(Debug, Clone, Default)]
struct TeamShutdownOptions132 { team: String, force: bool, merge: bool }

#[derive(Debug, Clone)]
struct TeamShutdownLive132 { member: TeamMember122, pane: TeamPane124 }

fn team_shutdown(argv: &[String]) -> Result<String, String> {
    let opts = team_shutdown_parse(argv)?;
    let paths = team_paths(&opts.team);
    let config = team_read_json::<TeamConfig122>(&paths.tool_config).ok_or_else(|| format!("team shutdown: team '{}' not found", opts.team))?;
    let panes = team_shutdown_panes();
    let alive = team_shutdown_alive(&config, &panes)?;
    team_shutdown_archive_before(&opts.team)?;
    let mut actions = Vec::new();
    if alive.is_empty() {
        actions.push("already exited".to_owned());
    } else {
        for live in &alive {
            team_shutdown_write_request(&opts.team, &live.member.name)?;
            actions.push(format!("request shutdown {}", live.member.name));
        }
        team_shutdown_wait()?;
        if opts.force {
            for live in &alive {
                team_shutdown_validate_pane_belongs(&opts.team, live)?;
                team_shutdown_kill(&live.pane.pane_id)?;
                actions.push(format!("force kill {} {}", live.member.name, live.pane.pane_id));
            }
        } else {
            for live in &alive { actions.push(format!("did not force kill {}", live.member.name)); }
        }
    }
    if opts.merge { team_shutdown_merge(&opts.team, &config)?; actions.push("merged team knowledge".to_owned()); }
    team_shutdown_cleanup(&opts.team)?;
    actions.push("cleaned up team directories".to_owned());
    Ok(team_shutdown_render(&opts.team, &actions))
}

fn team_shutdown_parse(argv: &[String]) -> Result<TeamShutdownOptions132, String> {
    let mut opts = TeamShutdownOptions132::default();
    let mut team = None;
    for arg in argv.iter().skip(1) {
        match arg.as_str() {
            "--force" => opts.force = true,
            "--merge" => opts.merge = true,
            value if value.starts_with('-') && team.is_none() => return Err(format!("unsafe team name '{value}': leading dash rejected")),
            value if value.starts_with('-') => return Err(format!("team shutdown: unknown argument {value}")),
            value if team.is_none() => team = Some(value.to_owned()),
            value => return Err(format!("team shutdown: unexpected argument {value}")),
        }
    }
    opts.team = team.ok_or_else(|| "usage: maw team shutdown <name> [--force] [--merge]".to_owned())?;
    team_validate_name(&opts.team)?;
    Ok(opts)
}

fn team_shutdown_panes() -> Vec<TeamPane124> { team_t3_panes() }

fn team_shutdown_alive(config: &TeamConfig122, panes: &[TeamPane124]) -> Result<Vec<TeamShutdownLive132>, String> {
    let mut out = Vec::new();
    for member in config.members.iter().filter(|member| member.agent_type.as_deref() != Some("team-lead")) {
        team_validate_target(&member.name)?;
        let Some(pane_id) = member.tmux_pane_id.as_deref().filter(|value| !value.is_empty()) else { continue; };
        team_shutdown_validate_pane_id(pane_id)?;
        if let Some(pane) = panes.iter().find(|pane| pane.pane_id == pane_id && team_t3_is_live_command(&pane.command)) {
            out.push(TeamShutdownLive132 { member: member.clone(), pane: pane.clone() });
        }
    }
    Ok(out)
}

fn team_shutdown_write_request(team: &str, member: &str) -> Result<(), String> {
    let path = team_paths(team).tool_dir.join("inboxes").join(format!("{member}.json"));
    let mut messages = team_read_json::<Vec<TeamInboxMessage122>>(&path).unwrap_or_default();
    messages.push(TeamInboxMessage122 { from: "maw-team-shutdown".to_owned(), text: serde_json::json!({"type":"shutdown_request","reason":"team teardown via maw team shutdown"}).to_string(), summary: "shutdown requested".to_owned(), timestamp: team_timestamp(), read: false });
    team_write_json_atomic_0600(&path, &messages)
}

fn team_shutdown_archive_before(team: &str) -> Result<(), String> {
    let archive = team_psi_dir().join("memory").join("mailbox").join("teams").join(team).join(format!("shutdown-archive-{}", team_shutdown_stamp()));
    std::fs::create_dir_all(&archive).map_err(|error| format!("team shutdown archive mkdir failed: {error}"))?;
    let paths = team_paths(team);
    team_shutdown_copy_tree(&paths.tool_dir, &archive.join("tool-team"))?;
    let tasks = team_state_dir().join("teams").join(team).join("tasks");
    team_shutdown_copy_tree(&tasks, &archive.join("tasks"))?;
    Ok(())
}

fn team_shutdown_stamp() -> String { std::env::var("MAW_RS_TEAM_SHUTDOWN_STAMP").unwrap_or_else(|_| team_now_millis().to_string()) }

fn team_shutdown_copy_tree(src: &std::path::Path, dst: &std::path::Path) -> Result<(), String> {
    if !src.exists() { return Ok(()); }
    let meta = std::fs::symlink_metadata(src).map_err(|error| format!("team shutdown archive stat failed: {error}"))?;
    if meta.is_file() { if let Some(parent) = dst.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("team shutdown archive parent failed: {error}"))?; } std::fs::copy(src, dst).map_err(|error| format!("team shutdown archive copy failed: {error}"))?; return Ok(()); }
    if !meta.is_dir() { return Ok(()); }
    std::fs::create_dir_all(dst).map_err(|error| format!("team shutdown archive dir failed: {error}"))?;
    for entry in std::fs::read_dir(src).map_err(|error| format!("team shutdown archive read failed: {error}"))?.flatten() { team_shutdown_copy_tree(&entry.path(), &dst.join(entry.file_name()))?; }
    Ok(())
}

fn team_shutdown_wait() -> Result<(), String> {
    if team_shutdown_fake_mode() { return team_shutdown_record_fake("wait", "fake-no-real-30s"); }
    std::thread::sleep(std::time::Duration::from_secs(30));
    Ok(())
}

fn team_shutdown_validate_pane_belongs(team: &str, live: &TeamShutdownLive132) -> Result<(), String> {
    team_shutdown_validate_pane_id(&live.pane.pane_id)?;
    if live.pane.window != live.member.name { return Err(format!("team shutdown refuse pane mismatch before force kill: {team}:{} != {}", live.pane.window, live.member.name)); }
    let panes = team_shutdown_panes();
    let matches = panes.iter().filter(|pane| pane.pane_id == live.pane.pane_id && pane.window == live.member.name && team_t3_is_live_command(&pane.command)).count();
    match matches {
        1 => Ok(()),
        0 => Err(format!("team shutdown refuse missing pane before force kill: {team}:{}", live.pane.pane_id)),
        _ => Err(format!("team shutdown refuse ambiguous pane before force kill: {team}:{}", live.pane.pane_id)),
    }
}

fn team_shutdown_validate_pane_id(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("unsafe pane id {value:?}")); }
    if !value.starts_with('%') { return Err(format!("unsafe pane id {value:?}: expected tmux pane id")); }
    Ok(())
}

fn team_shutdown_kill(pane_id: &str) -> Result<(), String> {
    if team_shutdown_fake_mode() { return team_shutdown_record_fake("kill", pane_id); }
    let mut runner = maw_tmux::CommandTmuxRunner::default();
    maw_tmux::TmuxRunner::run(&mut runner, "kill-pane", &["-t".to_owned(), pane_id.to_owned()]).map(|_| ()).map_err(|error| error.message)
}

fn team_shutdown_merge(team: &str, config: &TeamConfig122) -> Result<(), String> {
    let paths = team_paths(team);
    for member in config.members.iter().filter(|member| member.agent_type.as_deref() != Some("team-lead")) {
        let mailbox = team_psi_dir().join("memory").join("mailbox").join(&member.name);
        std::fs::create_dir_all(&mailbox).map_err(|error| format!("team shutdown merge mailbox failed: {error}"))?;
        let inbox = paths.tool_dir.join("inboxes").join(format!("{}.json", member.name));
        if inbox.exists() { std::fs::copy(&inbox, mailbox.join(format!("team-{team}-inbox.json"))).map_err(|error| format!("team shutdown merge inbox failed: {error}"))?; }
        let member_dir = paths.tool_dir.join(&member.name);
        if let Ok(entries) = std::fs::read_dir(member_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.ends_with("_findings.md") { std::fs::copy(entry.path(), mailbox.join(name)).map_err(|error| format!("team shutdown merge findings failed: {error}"))?; }
            }
        }
    }
    let team_archive = team_psi_dir().join("memory").join("mailbox").join("teams").join(team);
    std::fs::create_dir_all(&team_archive).map_err(|error| format!("team shutdown merge team dir failed: {error}"))?;
    if paths.tool_config.exists() { std::fs::copy(&paths.tool_config, team_archive.join("manifest.json")).map_err(|error| format!("team shutdown merge config failed: {error}"))?; }
    Ok(())
}

fn team_shutdown_cleanup(team: &str) -> Result<(), String> {
    let paths = team_paths(team);
    let tool_root = team_home_dir().join(".claude").join("teams");
    let tasks_root = team_state_dir().join("teams");
    team_shutdown_bounded_remove_dir_all(&tool_root, &paths.tool_dir, &format!("team shutdown {team}"))?;
    team_shutdown_bounded_remove_dir_all(&tasks_root, &tasks_root.join(team), &format!("team shutdown tasks {team}"))?;
    Ok(())
}

fn team_shutdown_bounded_remove_dir_all(root: &std::path::Path, target: &std::path::Path, label: &str) -> Result<bool, String> {
    if !target.exists() { return Ok(false); }
    let root_canon = root.canonicalize().map_err(|error| format!("{label}: canonicalize root {} failed: {error}", root.display()))?;
    let target_canon = target.canonicalize().map_err(|error| format!("{label}: canonicalize target {} failed: {error}", target.display()))?;
    if target_canon == root_canon || !target_canon.starts_with(&root_canon) { return Err(format!("{label}: refuse unbounded cleanup {} outside {}", target_canon.display(), root_canon.display())); }
    std::fs::remove_dir_all(&target_canon).map_err(|error| format!("{label}: cleanup {} failed: {error}", target_canon.display()))?;
    Ok(true)
}

fn team_shutdown_fake_mode() -> bool { std::env::var_os("MAW_RS_TEAM_SHUTDOWN_FAKE_LOG").is_some() }

fn team_shutdown_record_fake(kind: &str, value: &str) -> Result<(), String> {
    use std::io::Write as _;
    let Some(path) = std::env::var_os("MAW_RS_TEAM_SHUTDOWN_FAKE_LOG") else { return Ok(()); };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("team shutdown fake log mkdir failed: {error}"))?; }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|error| format!("team shutdown fake log open failed: {error}"))?;
    writeln!(file, "{kind}\t{value}").map_err(|error| format!("team shutdown fake log write failed: {error}"))
}

fn team_shutdown_render(team: &str, actions: &[String]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("team shutdown: {team}\n");
    for action in actions { writeln!(out, "  {action}").expect("write string"); }
    out
}
