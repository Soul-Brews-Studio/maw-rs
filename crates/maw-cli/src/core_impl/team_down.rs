const TEAM_DOWN_GUARD_WINDOW: &str = "maw-team-lifecycle-guard";

#[derive(Debug, Clone, Default)]
struct TeamDownOptions126 {
    team: String,
    keep: Vec<String>,
    all: bool,
    dry_run: bool,
    status: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamDownAction126 {
    role: String,
    state: String,
    action: String,
    target: Option<String>,
}

fn team_down(argv: &[String]) -> Result<String, String> {
    let opts = team_down_parse(argv)?;
    let charter_path = team_t3_resolve_charter_path(&opts.team, None)?;
    let charter = team_read_charter_path(&charter_path)?;
    let session = team_down_session(&charter)?;
    team_t3_validate_session(&session)?;
    let panes = team_down_panes()?;
    let actions = team_down_plan(&charter, &opts, &session, &panes)?;
    if opts.status || opts.dry_run {
        return Ok(team_down_render(&opts.team, &session, &charter, &actions, opts.dry_run));
    }
    team_down_execute(&opts.team, &session, &actions)?;
    Ok(team_down_render(&opts.team, &session, &charter, &actions, false))
}

fn team_down_parse(argv: &[String]) -> Result<TeamDownOptions126, String> {
    let mut opts = TeamDownOptions126::default();
    let mut index = 1;
    let mut team = None;
    while index < argv.len() {
        match argv[index].as_str() {
            "--all" => opts.all = true,
            "--dry-run" => opts.dry_run = true,
            "--status" => opts.status = true,
            "--keep" => {
                index += 1;
                opts.keep = team_t3_csv(&team_down_next(argv, index, "--keep")?);
            }
            value if value.starts_with('-') && team.is_none() => return Err(format!("unsafe team name '{value}': leading dash rejected")),
            value if value.starts_with('-') => return Err(format!("team down: unknown argument {value}")),
            value if team.is_none() => team = Some(value.to_owned()),
            value => return Err(format!("team down: unexpected argument {value}")),
        }
        index += 1;
    }
    opts.team = team.ok_or_else(|| "usage: maw team down <team> [--all] [--keep <a,b>] [--dry-run] [--status]".to_owned())?;
    team_validate_name(&opts.team)?;
    for keep in &opts.keep {
        team_t3_validate_token(keep, "keep selector")?;
    }
    Ok(opts)
}

fn team_down_next(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index).ok_or_else(|| format!("{flag} requires a value"))?;
    team_t3_validate_token(value, flag)?;
    Ok(value.clone())
}

fn team_down_session(charter: &TeamCharter122) -> Result<String, String> {
    if let Ok(session) = std::env::var("MAW_RS_TEAM_SESSION") {
        if !session.is_empty() {
            return Ok(session);
        }
    }
    charter.session.clone().filter(|session| !session.is_empty()).ok_or_else(|| "team down refuse no-session before teardown: set charter session or MAW_RS_TEAM_SESSION".to_owned())
}

fn team_down_panes() -> Result<Vec<TeamPane124>, String> {
    if std::env::var_os("MAW_RS_TEAM_TMUX_PANES").is_some() {
        return Ok(team_t3_panes());
    }
    let mut runner = maw_tmux::CommandTmuxRunner::default();
    let args = vec![
        "-a".to_owned(),
        "-F".to_owned(),
        "#{session_name}|#{window_name}|#{pane_current_command}|#{pane_current_path}|#{pane_id}".to_owned(),
    ];
    let raw = maw_tmux::TmuxRunner::run(&mut runner, "list-panes", &args).map_err(|error| error.message)?;
    Ok(raw.lines().filter_map(team_t3_parse_pane).collect())
}

fn team_down_plan(charter: &TeamCharter122, opts: &TeamDownOptions126, session: &str, panes: &[TeamPane124]) -> Result<Vec<TeamDownAction126>, String> {
    let mut actions = Vec::new();
    let roster: Vec<_> = charter.members.iter().map(|member| team_t3_classify(member, &TeamT3Options124::default(), session, panes)).collect();
    let mut killable = Vec::new();
    for (member, item) in charter.members.iter().zip(roster.iter()) {
        if let Some(reason) = team_down_keep_reason(member, item, &opts.keep, opts.all) {
            actions.push(TeamDownAction126 { role: item.role.clone(), state: item.state.clone(), action: format!("keep ({reason})"), target: None });
            continue;
        }
        if item.state == "missing" && !opts.status && !opts.dry_run {
            return Err(format!("team down refuse missing target before teardown: {session}:{}", item.role));
        }
        if item.state != "live" {
            actions.push(TeamDownAction126 { role: item.role.clone(), state: item.state.clone(), action: format!("skip {}", item.state), target: None });
            continue;
        }
        let target = item.pane.as_ref().map(|pane| pane.window.clone()).ok_or_else(|| format!("team down validation failed for {}: live member has no pane", item.role))?;
        team_down_validate_target(session, &target, panes)?;
        killable.push(target.clone());
        let action = if opts.status || opts.dry_run { format!("would maw done {target}") } else { format!("maw done {target}") };
        actions.push(TeamDownAction126 { role: item.role.clone(), state: item.state.clone(), action, target: Some(target) });
    }
    if !opts.status && !opts.dry_run && team_down_would_kill_last_window(session, panes, &killable) {
        actions.insert(0, TeamDownAction126 { role: "session".to_owned(), state: "guard".to_owned(), action: format!("create {TEAM_DOWN_GUARD_WINDOW}"), target: Some(TEAM_DOWN_GUARD_WINDOW.to_owned()) });
    }
    Ok(actions)
}

fn team_down_keep_reason(member: &TeamCharterMember122, item: &TeamRosterItem124, keep: &[String], include_lead: bool) -> Option<String> {
    if item.state == "skipped" {
        return Some("selector".to_owned());
    }
    if !include_lead && matches!(member.role.as_str(), "lead" | "bridge") {
        return Some("lead".to_owned());
    }
    if team_down_matches_keep(member, keep) {
        return Some("--keep".to_owned());
    }
    None
}

fn team_down_matches_keep(member: &TeamCharterMember122, keep: &[String]) -> bool {
    keep.iter().any(|selector| selector == &member.role || member.name.as_ref().is_some_and(|name| selector == name) || member.cwd.as_ref().is_some_and(|cwd| selector == cwd))
}

fn team_down_validate_target(session: &str, target: &str, panes: &[TeamPane124]) -> Result<(), String> {
    team_t3_validate_token(target, "down target")?;
    let matches = panes.iter().filter(|pane| pane.session == session && pane.window == target).count();
    match matches {
        1 => Ok(()),
        0 => Err(format!("team down refuse missing target before teardown: {session}:{target}")),
        _ => Err(format!("team down refuse ambiguous target before teardown: {session}:{target}")),
    }
}

fn team_down_would_kill_last_window(session: &str, panes: &[TeamPane124], killable: &[String]) -> bool {
    let windows = panes.iter().filter(|pane| pane.session == session && pane.window != TEAM_DOWN_GUARD_WINDOW).map(|pane| pane.window.as_str()).collect::<std::collections::BTreeSet<_>>();
    !windows.is_empty() && windows.iter().all(|window| killable.iter().any(|target| target == window))
}

fn team_down_execute(team: &str, session: &str, actions: &[TeamDownAction126]) -> Result<(), String> {
    for action in actions {
        if action.role == "session" && action.target.as_deref() == Some(TEAM_DOWN_GUARD_WINDOW) {
            team_down_create_guard(session)?;
        }
    }
    for action in actions.iter().filter(|action| action.target.is_some() && action.role != "session") {
        let target = action.target.as_deref().expect("target checked");
        team_down_archive_before_done(team, &action.role)?;
        team_down_done(session, target)?;
    }
    Ok(())
}

fn team_down_create_guard(session: &str) -> Result<(), String> {
    if team_down_fake_mode() {
        team_down_record_fake("guard", &format!("{session}:{TEAM_DOWN_GUARD_WINDOW}"))?;
        return Ok(());
    }
    let mut runner = maw_tmux::CommandTmuxRunner::default();
    let args = vec!["-d".to_owned(), "-t".to_owned(), format!("{session}:"), "-n".to_owned(), TEAM_DOWN_GUARD_WINDOW.to_owned()];
    maw_tmux::TmuxRunner::run(&mut runner, "new-window", &args).map(|_| ()).map_err(|error| error.message)
}

fn team_down_archive_before_done(team: &str, member: &str) -> Result<(), String> {
    let archive_dir = team_psi_dir().join("memory").join("mailbox").join(member).join(format!("team-{team}-archive"));
    std::fs::create_dir_all(&archive_dir).map_err(|error| format!("team down archive mkdir failed: {error}"))?;
    let paths = team_paths(team);
    let inbox = paths.tool_dir.join("inboxes").join(format!("{member}.json"));
    if inbox.exists() {
        std::fs::copy(&inbox, archive_dir.join("inbox.json")).map_err(|error| format!("team down archive inbox failed: {error}"))?;
    }
    let member_dir = paths.tool_dir.join(member);
    if let Ok(entries) = std::fs::read_dir(&member_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { continue; };
            if name.ends_with("_findings.md") {
                std::fs::copy(&path, archive_dir.join(name)).map_err(|error| format!("team down archive findings failed: {error}"))?;
            }
        }
    }
    if paths.tool_config.exists() {
        let team_archive = team_psi_dir().join("memory").join("mailbox").join("teams").join(team);
        std::fs::create_dir_all(&team_archive).map_err(|error| format!("team down archive team mkdir failed: {error}"))?;
        std::fs::copy(&paths.tool_config, team_archive.join("manifest.json")).map_err(|error| format!("team down archive manifest failed: {error}"))?;
    }
    team_down_record_fake("archive", member)
}

fn team_down_done(session: &str, target: &str) -> Result<(), String> {
    if team_down_fake_mode() {
        return team_down_record_fake("done", &format!("{session}:{target}"));
    }
    let options = DoneOptions { target: Some(target.to_owned()), ..Default::default() };
    let mut local = DoneLocal::default();
    done_run_one(target, &options, Some(session), &mut local).map(|_| ())
}

fn team_down_fake_mode() -> bool {
    std::env::var_os("MAW_RS_TEAM_DOWN_FAKE_LOG").is_some()
}

fn team_down_record_fake(kind: &str, value: &str) -> Result<(), String> {
    use std::io::Write as _;
    let Some(path) = std::env::var_os("MAW_RS_TEAM_DOWN_FAKE_LOG") else { return Ok(()); };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("team down fake log mkdir failed: {error}"))?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|error| format!("team down fake log open failed: {error}"))?;
    writeln!(file, "{kind}\t{value}").map_err(|error| format!("team down fake log write failed: {error}"))
}

fn team_down_render(team: &str, session: &str, _charter: &TeamCharter122, actions: &[TeamDownAction126], dry_run: bool) -> String {
    use std::fmt::Write as _;
    let mut out = format!("team down: {team} ({session})\nrole\tstate\taction\n");
    for action in actions {
        writeln!(out, "{}\t{}\t{}", action.role, action.state, action.action).expect("write string");
    }
    if dry_run {
        out.push_str("\nNo changes made\n");
    }
    out
}
