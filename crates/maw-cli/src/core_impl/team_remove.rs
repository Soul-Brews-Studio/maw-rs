const DISPATCH_130: &[DispatcherEntry] = &[];

#[derive(Debug, Clone, Default)]
struct TeamRemoveOptions130 { selector: String, keep_branch: bool, dry_run: bool }

#[derive(Debug, Clone)]
struct TeamRemoveBlock130 { start: usize, end: usize, member: TeamCharterMember122 }

fn team_remove(argv: &[String]) -> Result<String, String> {
    let opts = team_remove_parse(argv)?;
    let team = team_remove_context_team()?;
    let charter_path = team_t3_resolve_charter_path(&team, None)?;
    let charter_text = std::fs::read_to_string(&charter_path).map_err(|error| format!("team remove: read charter failed: {error}"))?;
    let charter = team_parse_charter(&charter_text)?;
    let updated = team_remove_charter_text(&charter_text, &opts.selector)?;
    let session = team_remove_session(&charter)?;
    let selected = team_remove_select(&charter, &opts.selector, &session)?;
    let target = selected.pane.as_ref().map_or_else(|| selected.identity.clone(), |pane| pane.window.clone());
    team_t3_validate_token(&target, "remove target")?;
    let has_worktree = team_remove_has_worktree(&charter, &selected.role);
    let mut actions = Vec::new();
    if !has_worktree {
        actions.push(format!("teardown: no worktree for {target} (worktree: false)"));
    } else if opts.dry_run {
        let suffix = if opts.keep_branch { " (keep branch)" } else { "" };
        actions.push(format!("teardown: would maw done {target}{suffix}"));
    } else {
        team_remove_validate_live_target(&session, &target)?;
        team_remove_archive_before_done(&team, &selected.role, &charter_path)?;
        team_remove_done(&session, &target, opts.keep_branch)?;
        let suffix = if opts.keep_branch { " (kept branch)" } else { "" };
        actions.push(format!("teardown: maw done {target}{suffix}"));
    }
    if opts.dry_run { actions.push(format!("charter: would remove '{}' from {charter_path}", selected.role)); }
    else { team_atomic_write_0600(std::path::Path::new(&charter_path), &updated)?; actions.push(format!("charter: removed '{}' from {charter_path}", selected.role)); }
    Ok(team_remove_render(&team, &selected.role, &target, &selected.state, &session, &actions, opts.dry_run))
}

fn team_remove_parse(argv: &[String]) -> Result<TeamRemoveOptions130, String> {
    let mut opts = TeamRemoveOptions130::default();
    let mut selector = None;
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--keep-branch" => opts.keep_branch = true,
            "--dry-run" => opts.dry_run = true,
            value if value.starts_with('-') && selector.is_none() => return Err(format!("unsafe member selector '{value}': leading dash rejected")),
            value if value.starts_with('-') => return Err(format!("team remove: unknown argument {value}")),
            value if selector.is_none() => selector = Some(value.to_owned()),
            value => return Err(format!("team remove: unexpected argument {value}")),
        }
        index += 1;
    }
    opts.selector = selector.ok_or_else(|| "usage: maw team remove <member> [--keep-branch] [--dry-run]".to_owned())?;
    team_t3_validate_token(&opts.selector, "member selector")?;
    Ok(opts)
}

fn team_remove_context_team() -> Result<String, String> {
    let team = std::env::var("MAW_TEAM").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "default".to_owned());
    team_validate_name(&team)?;
    Ok(team)
}

fn team_remove_session(charter: &TeamCharter122) -> Result<String, String> {
    if let Ok(session) = std::env::var("MAW_RS_TEAM_SESSION") { if !session.is_empty() { team_t3_validate_session(&session)?; return Ok(session); } }
    let session = charter.session.clone().unwrap_or_default();
    team_t3_validate_session(&session)?;
    Ok(session)
}

fn team_remove_select(charter: &TeamCharter122, selector: &str, session: &str) -> Result<TeamRosterItem124, String> {
    let panes = team_t3_panes();
    let matches = charter.members.iter().filter_map(|member| {
        let item = team_t3_classify(member, &TeamT3Options124::default(), session, &panes);
        if team_remove_member_matches(member, selector, &item.identity, &item.worktree) { Some(item) } else { None }
    }).collect::<Vec<_>>();
    match matches.len() {
        0 => Err(format!("member not found: {selector}")),
        1 => Ok(matches[0].clone()),
        _ => Err(format!("member '{selector}' is ambiguous across: {}", matches.iter().map(|item| item.role.as_str()).collect::<Vec<_>>().join(", "))),
    }
}

fn team_remove_member_matches(member: &TeamCharterMember122, selector: &str, identity: &str, worktree: &str) -> bool {
    selector == member.role || selector == identity || selector == worktree || member.name.as_deref() == Some(selector) || member.cwd.as_deref() == Some(selector)
}

fn team_remove_has_worktree(charter: &TeamCharter122, role: &str) -> bool {
    charter.members.iter().find(|member| member.role == role).and_then(|member| member.worktree.as_deref()).is_none_or(|value| value != "false")
}

fn team_remove_validate_live_target(session: &str, target: &str) -> Result<(), String> {
    let panes = team_t3_panes();
    let matches = panes.iter().filter(|pane| pane.session == session && pane.window == target && team_t3_is_live_command(&pane.command)).count();
    match matches {
        1 => Ok(()),
        0 => Err(format!("team remove refuse missing live target before teardown: {session}:{target}")),
        _ => Err(format!("team remove refuse ambiguous live target before teardown: {session}:{target}")),
    }
}

fn team_remove_archive_before_done(team: &str, member: &str, charter_path: &str) -> Result<(), String> {
    let archive_dir = team_psi_dir().join("memory").join("mailbox").join(member).join(format!("team-{team}-remove-archive"));
    std::fs::create_dir_all(&archive_dir).map_err(|error| format!("team remove archive mkdir failed: {error}"))?;
    let paths = team_paths(team);
    let inbox = paths.tool_dir.join("inboxes").join(format!("{member}.json"));
    if inbox.exists() { std::fs::copy(&inbox, archive_dir.join("inbox.json")).map_err(|error| format!("team remove archive inbox failed: {error}"))?; }
    if std::path::Path::new(charter_path).exists() { std::fs::copy(charter_path, archive_dir.join("charter.yaml")).map_err(|error| format!("team remove archive charter failed: {error}"))?; }
    if paths.tool_config.exists() { std::fs::copy(&paths.tool_config, archive_dir.join("config.json")).map_err(|error| format!("team remove archive config failed: {error}"))?; }
    team_remove_record_fake("archive", member)
}

fn team_remove_done(session: &str, target: &str, keep_branch: bool) -> Result<(), String> {
    if team_remove_fake_mode() { return team_remove_record_fake("done", &format!("{session}:{target}:keep_branch={keep_branch}")); }
    let options = DoneOptions { target: Some(target.to_owned()), ..Default::default() };
    let mut local = DoneLocal::default();
    done_run_one(target, &options, Some(session), &mut local).map(|_| ())
}

fn team_remove_fake_mode() -> bool { std::env::var_os("MAW_RS_TEAM_REMOVE_FAKE_LOG").is_some() }

fn team_remove_record_fake(kind: &str, value: &str) -> Result<(), String> {
    use std::io::Write as _;
    let Some(path) = std::env::var_os("MAW_RS_TEAM_REMOVE_FAKE_LOG") else { return Ok(()); };
    let path = std::path::PathBuf::from(path);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("team remove fake log mkdir failed: {error}"))?; }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(&path).map_err(|error| format!("team remove fake log open failed: {error}"))?;
    writeln!(file, "{kind}\t{value}").map_err(|error| format!("team remove fake log write failed: {error}"))
}

fn team_remove_render(team: &str, member: &str, target: &str, state: &str, session: &str, actions: &[String], dry_run: bool) -> String {
    let mut out = format!("team remove: {team}\nmember\t{member}\ntarget\t{target}\nstate\t{state}\naction\t{}\nsession\t{}\n\n", if dry_run { "dry-run" } else { "teardown+charter-edit" }, if session.is_empty() { "(none)" } else { session });
    for action in actions { out.push_str("  "); out.push_str(action); out.push('\n'); }
    if dry_run { out.push_str("\nNo changes made\n"); }
    out
}

fn team_remove_charter_text(text: &str, selector: &str) -> Result<String, String> {
    if text.trim_start().starts_with('{') { return team_remove_json_charter_text(text, selector); }
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing = text.ends_with('\n');
    let mut lines = text.lines().map(str::to_owned).collect::<Vec<_>>();
    let blocks = team_remove_member_blocks(&lines);
    let matches = blocks.iter().filter(|block| team_remove_block_matches(block, selector)).collect::<Vec<_>>();
    if matches.is_empty() { return Err(format!("member not found in charter: {selector}")); }
    if matches.len() > 1 { return Err(format!("member '{selector}' is ambiguous in charter ({} matches)", matches.len())); }
    if blocks.len() == 1 { return Err(format!("refusing to remove the last member '{selector}'; a team charter requires at least one member")); }
    let block = matches[0];
    lines.drain(block.start..block.end);
    let mut out = lines.join(eol);
    if trailing { out.push_str(eol); }
    Ok(out)
}

fn team_remove_member_blocks(lines: &[String]) -> Vec<TeamRemoveBlock130> {
    let Some(members_idx) = lines.iter().position(|line| line.trim() == "members:") else { return Vec::new(); };
    let mut blocks = Vec::new();
    let mut index = members_idx + 1;
    let mut dash_indent = None;
    while index < lines.len() {
        let line = &lines[index];
        if line.trim().is_empty() { index += 1; continue; }
        let indent = team_remove_indent(line);
        if indent == 0 { break; }
        let is_dash = line.trim_start().starts_with('-');
        if dash_indent.is_none() && is_dash { dash_indent = Some(indent); }
        if !is_dash || Some(indent) != dash_indent { index += 1; continue; }
        let start = index;
        index += 1;
        while index < lines.len() && (lines[index].trim().is_empty() || team_remove_indent(&lines[index]) > indent) { index += 1; }
        let end = index;
        let member = team_remove_parse_block(&lines[start..end]);
        if !member.role.is_empty() { blocks.push(TeamRemoveBlock130 { start, end, member }); }
    }
    blocks
}

fn team_remove_indent(line: &str) -> usize { line.chars().take_while(|ch| *ch == ' ').count() }

fn team_remove_parse_block(lines: &[String]) -> TeamCharterMember122 {
    let mut member = TeamCharterMember122::default();
    for raw in lines {
        let line = raw.trim_start().trim_start_matches('-').trim_start();
        for (key, slot) in [("role:", &mut member.role), ("name:", member.name.get_or_insert_with(String::new)), ("cwd:", member.cwd.get_or_insert_with(String::new)), ("worktree:", member.worktree.get_or_insert_with(String::new))] {
            if let Some(rest) = line.strip_prefix(key) { *slot = team_unquote(rest); }
        }
    }
    member
}

fn team_remove_block_matches(block: &TeamRemoveBlock130, selector: &str) -> bool {
    selector == block.member.role || block.member.name.as_deref() == Some(selector) || block.member.cwd.as_deref() == Some(selector) || block.member.worktree.as_deref() == Some(selector)
}

fn team_remove_json_charter_text(text: &str, selector: &str) -> Result<String, String> {
    let mut value: serde_json::Value = serde_json::from_str(text).map_err(|error| format!("team remove json charter parse failed: {error}"))?;
    let members = value["members"].as_array_mut().ok_or_else(|| "team remove json charter missing members".to_owned())?;
    let matches = members.iter().enumerate().filter(|(_, member)| ["role", "name", "cwd", "worktree"].iter().any(|key| member[*key].as_str() == Some(selector))).map(|(index, _)| index).collect::<Vec<_>>();
    if matches.is_empty() { return Err(format!("member not found in charter: {selector}")); }
    if matches.len() > 1 { return Err(format!("member '{selector}' is ambiguous in charter ({} matches)", matches.len())); }
    if members.len() == 1 { return Err(format!("refusing to remove the last member '{selector}'; a team charter requires at least one member")); }
    members.remove(matches[0]);
    serde_json::to_string_pretty(&value).map(|body| body + "\n").map_err(|error| format!("team remove json charter encode failed: {error}"))
}
