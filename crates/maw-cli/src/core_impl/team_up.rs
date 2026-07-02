#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamPane124 {
    session: String,
    window: String,
    command: String,
    path: String,
    pane_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamRosterItem124 {
    role: String,
    identity: String,
    engine: String,
    worktree: String,
    state: String,
    action: String,
    pane: Option<TeamPane124>,
}

#[allow(dead_code)]
fn team_t3_up(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team up <team> [--session <name>] [--members <roles>] [--only <a,b>] [--dry-run] [--status] [-e <engine>]")?;
    if !(team_t3_has(&opts, TEAM_T3_STATUS) || team_t3_has(&opts, TEAM_T3_DRY_RUN)) {
        return Err("team up native T3 is read-only only: pass --status or --dry-run; exec wake is held for T5 design".to_owned());
    }
    let charter = team_t3_load_or_quick_charter(&opts)?;
    Ok(team_t3_render_up(&charter, &opts))
}

#[allow(dead_code)]
fn team_t3_bring(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team bring <team> [--session <session>] [--split] [--gather] [--dry-run] [-e <engine>]")?;
    if !team_t3_has(&opts, TEAM_T3_DRY_RUN) {
        return Err("team bring native T3 is dry-run only; exec wake/gather is held for T5 design".to_owned());
    }
    let team = opts.team.as_ref().ok_or_else(|| "usage: maw team bring <team> [--session <session>] [--split] [--gather] [--dry-run] [-e <engine>]".to_owned())?;
    team_validate_name(team)?;
    Ok(team_t3_render_bring(team, &opts))
}

#[allow(dead_code)]
fn team_t3_apply(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team apply <team|team.yaml> [--charter <path>] [--session <name]")?;
    if team_t3_has(&opts, TEAM_T3_APPLY) {
        return Err("team apply native T3 is dry-run only; --apply exec/teardown is held for T5/T4 design".to_owned());
    }
    let charter = team_t3_load_apply_charter(&opts)?;
    Ok(team_t3_render_apply(&charter, &opts))
}

fn team_t3_liveness(argv: &[String]) -> Result<String, String> {
    let opts = team_t3_parse_flags(argv, "usage: maw team liveness <team> [--session <name>]")?;
    let charter = team_t3_load_or_quick_charter(&opts)?;
    Ok(team_t3_render_liveness(&charter, &opts))
}

const TEAM_T3_DRY_RUN: u16 = 1 << 0;
const TEAM_T3_STATUS: u16 = 1 << 1;
const TEAM_T3_FORCE: u16 = 1 << 2;
const TEAM_T3_GATHER: u16 = 1 << 3;
const TEAM_T3_SPLIT: u16 = 1 << 4;
const TEAM_T3_APPLY: u16 = 1 << 5;

#[derive(Debug, Clone, Default)]
struct TeamT3Options124 {
    team: Option<String>,
    session: Option<String>,
    engine: Option<String>,
    only: Vec<String>,
    members: Vec<String>,
    flags: u16,
    quick: Option<usize>,
    charter_path: Option<String>,
}

fn team_t3_parse_flags(argv: &[String], usage: &str) -> Result<TeamT3Options124, String> {
    let mut opts = TeamT3Options124::default();
    let mut positional = Vec::new();
    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--dry-run" => opts.flags |= TEAM_T3_DRY_RUN,
            "--status" => opts.flags |= TEAM_T3_STATUS,
            "--force" => opts.flags |= TEAM_T3_FORCE,
            "--gather" => opts.flags |= TEAM_T3_GATHER,
            "--split" => opts.flags |= TEAM_T3_SPLIT,
            "--apply" => opts.flags |= TEAM_T3_APPLY,
            "--session" => { index += 1; opts.session = Some(team_t3_next(argv, index, "--session")?); },
            "--engine" | "-e" => { index += 1; opts.engine = Some(team_t3_next(argv, index, "--engine")?); },
            "--only" => { index += 1; opts.only = team_t3_csv(&team_t3_next(argv, index, "--only")?); },
            "--members" => { index += 1; opts.members = team_t3_csv(&team_t3_next(argv, index, "--members")?); },
            "--quick" => { index += 1; opts.quick = Some(team_t3_next(argv, index, "--quick")?.parse::<usize>().map_err(|_| "--quick must be a positive integer".to_owned())?); },
            "--charter" => { index += 1; opts.charter_path = Some(team_t3_next(argv, index, "--charter")?); },
            value if value.starts_with('-') => return Err(format!("team: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    opts.team = positional.first().cloned().or_else(|| opts.quick.map(|_| "quick".to_owned()));
    if opts.team.is_none() { return Err(usage.to_owned()); }
    if let Some(team) = &opts.team {
        if team_t3_is_path_input(team) { team_validate_path_arg(team)?; } else { team_validate_name(team)?; }
    }
    if let Some(session) = &opts.session { team_t3_validate_session(session)?; }
    if let Some(engine) = &opts.engine { team_t3_validate_token(engine, "engine")?; }
    for value in opts.only.iter().chain(opts.members.iter()) { team_t3_validate_token(value, "selector")?; }
    Ok(opts)
}

fn team_t3_has(opts: &TeamT3Options124, flag: u16) -> bool {
    opts.flags & flag != 0
}

fn team_t3_next(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index).ok_or_else(|| format!("{flag} requires a value"))?;
    team_t3_validate_token(value, flag)?;
    Ok(value.clone())
}

fn team_t3_csv(value: &str) -> Vec<String> {
    value.split(',').map(str::trim).filter(|item| !item.is_empty()).map(str::to_owned).collect()
}

fn team_t3_validate_session(value: &str) -> Result<(), String> {
    if value.is_empty() { return Err("team session is empty".to_owned()); }
    if value.starts_with('-') { return Err(format!("unsafe team session '{value}': leading dash rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err("unsafe team session: control character rejected".to_owned()); }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')) { return Err(format!("unsafe team session '{value}': invalid character rejected")); }
    Ok(())
}

fn team_t3_validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("team {label} is empty")); }
    if value.starts_with('-') { return Err(format!("unsafe team {label} '{value}': leading dash rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("unsafe team {label}: control character rejected")); }
    Ok(())
}

fn team_t3_is_path_input(value: &str) -> bool {
    let path = std::path::Path::new(value);
    value.contains('/') || value.contains('\\') || path.extension().is_some_and(|ext| ["yaml", "yml", "json"].iter().any(|want| ext.eq_ignore_ascii_case(want))) || path.exists()
}

fn team_t3_load_or_quick_charter(opts: &TeamT3Options124) -> Result<TeamCharter122, String> {
    if let Some(count) = opts.quick {
        if count == 0 { return Err("--quick must be a positive integer".to_owned()); }
        let team = opts.team.clone().unwrap_or_else(|| "quick".to_owned());
        let members = (1..=count).map(|number| TeamCharterMember122 { role: format!("builder-{number}"), name: Some(format!("builder-{number}")), engine: opts.engine.clone(), ..Default::default() }).collect();
        return Ok(TeamCharter122 { name: team, description: String::new(), goal: String::new(), session: opts.session.clone(), members, governance_requires_human_approval: false });
    }
    let team = opts.team.as_ref().ok_or_else(|| "team required".to_owned())?;
    let path = team_t3_resolve_charter_path(team, opts.charter_path.as_deref())?;
    team_read_charter_path(&path)
}

fn team_t3_load_apply_charter(opts: &TeamT3Options124) -> Result<TeamCharter122, String> {
    if let Some(path) = &opts.charter_path { return team_read_charter_path(path); }
    let team = opts.team.as_ref().ok_or_else(|| "team required".to_owned())?;
    if team_t3_is_path_input(team) { return team_read_charter_path(team); }
    let path = team_t3_resolve_charter_path(team, None)?;
    team_read_charter_path(&path)
}

fn team_t3_resolve_charter_path(team: &str, explicit: Option<&str>) -> Result<String, String> {
    if let Some(path) = explicit { team_validate_path_arg(path)?; return Ok(path.to_owned()); }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    for candidate in [cwd.join(".maw").join("teams").join(format!("{team}.yaml")), cwd.join("ψ").join("teams").join(format!("{team}.yaml")), cwd.join(".maw").join("teams").join(format!("{team}.json")), cwd.join("ψ").join("teams").join(format!("{team}.json"))] {
        if candidate.exists() { return Ok(candidate.display().to_string()); }
    }
    Err(format!("charter not found: {team}"))
}

fn team_t3_render_up(charter: &TeamCharter122, opts: &TeamT3Options124) -> String {
    let session = team_t3_session(charter, opts);
    let roster = team_t3_roster(charter, opts, &session, team_t3_up_action);
    let mode = if team_t3_has(opts, TEAM_T3_STATUS) { "status" } else { "dry-run" };
    let mut out = team_t3_render_roster(&format!("team up: {} ({session}) {mode}", charter.name), &roster);
    if team_t3_has(opts, TEAM_T3_DRY_RUN) { out.push_str("\nNo changes made\n"); }
    out
}

fn team_t3_render_bring(team: &str, opts: &TeamT3Options124) -> String {
    use std::fmt::Write as _;
    let session = opts.session.clone().unwrap_or_else(|| team.to_owned());
    let members = team_message_targets(team);
    let mut out = format!("\x1b[36m⚡\x1b[0m bringing {} oracle(s) into workspace '{session}' (dry-run)\n", members.len());
    for oracle in members {
        let suffix = if team_t3_has(opts, TEAM_T3_SPLIT) && !team_t3_has(opts, TEAM_T3_GATHER) { " --split" } else { "" };
        writeln!(out, "\x1b[90mwould wake {oracle} --session {session}{suffix}\x1b[0m").expect("write string");
    }
    out.push_str("No changes made\n");
    out
}

fn team_t3_render_apply(charter: &TeamCharter122, opts: &TeamT3Options124) -> String {
    let session = team_t3_session(charter, opts);
    let roster = team_t3_roster(charter, opts, &session, |item, _opts| match item.state.as_str() {
        "missing" => "would spawn member".to_owned(),
        "live" => "skip live".to_owned(),
        "dead" => "skip dead member (team up can resume)".to_owned(),
        _ => "skip".to_owned(),
    });
    let mut out = team_t3_render_roster(&format!("team apply: {} ({session}) dry-run", charter.name), &roster);
    out.push_str("\nNo changes made (pass --apply after T5/T4 design lands)\n");
    out
}

fn team_t3_render_liveness(charter: &TeamCharter122, opts: &TeamT3Options124) -> String {
    let session = team_t3_session(charter, opts);
    let roster = team_t3_roster(charter, opts, &session, |item, _opts| format!("liveness {}", item.state));
    team_t3_render_roster(&format!("team liveness: {} ({session})", charter.name), &roster)
}

fn team_t3_session(charter: &TeamCharter122, opts: &TeamT3Options124) -> String {
    opts.session.clone().or_else(|| charter.session.clone()).unwrap_or_else(|| charter.name.clone())
}

fn team_t3_roster<F>(charter: &TeamCharter122, opts: &TeamT3Options124, session: &str, action: F) -> Vec<TeamRosterItem124>
where
    F: Fn(&TeamRosterItem124, &TeamT3Options124) -> String,
{
    let panes = team_t3_panes();
    charter.members.iter().map(|member| {
        let mut item = team_t3_classify(member, opts, session, &panes);
        item.action = action(&item, opts);
        item
    }).collect()
}

fn team_t3_classify(member: &TeamCharterMember122, opts: &TeamT3Options124, session: &str, panes: &[TeamPane124]) -> TeamRosterItem124 {
    let role = member.role.clone();
    let identity = member.name.clone().unwrap_or_else(|| role.clone());
    let engine = opts.engine.clone().or_else(|| member.engine.clone()).or_else(|| member.model.clone()).unwrap_or_else(|| "claude".to_owned());
    let worktree = member.cwd.clone().unwrap_or_else(|| identity.clone());
    if !opts.only.is_empty() && !team_t3_matches_selectors(member, &opts.only, &identity, &worktree) { return TeamRosterItem124 { role, identity, engine, worktree, state: "skipped".to_owned(), action: String::new(), pane: None }; }
    if !opts.members.is_empty() && !opts.members.iter().any(|item| item == &member.role) { return TeamRosterItem124 { role, identity, engine, worktree, state: "skipped".to_owned(), action: String::new(), pane: None }; }
    let candidates = team_t3_window_candidates(member, &identity, &worktree);
    let pane = panes.iter().find(|pane| pane.session == session && candidates.iter().any(|candidate| candidate == &pane.window || pane.window.ends_with(&format!("-{candidate}")))).cloned();
    let state = pane.as_ref().map_or("missing", |p| if team_t3_is_live_command(&p.command) { "live" } else { "dead" }).to_owned();
    TeamRosterItem124 { role, identity, engine, worktree, state, action: String::new(), pane }
}

fn team_t3_matches_selectors(member: &TeamCharterMember122, selectors: &[String], identity: &str, worktree: &str) -> bool {
    selectors.iter().any(|selector| selector == &member.role || selector == identity || selector == worktree)
}

fn team_t3_window_candidates(member: &TeamCharterMember122, identity: &str, worktree: &str) -> Vec<String> {
    let mut out = vec![identity.to_owned(), worktree.to_owned(), member.role.clone()];
    if let Some(name) = &member.name { out.push(name.trim_end_matches("-oracle").to_owned()); }
    out.sort();
    out.dedup();
    out.into_iter().filter(|item| !item.is_empty()).collect()
}

fn team_t3_up_action(item: &TeamRosterItem124, opts: &TeamT3Options124) -> String {
    if item.state == "skipped" { return "skip (selector)".to_owned(); }
    if team_t3_has(opts, TEAM_T3_FORCE) { return format!("would force fresh wake --wt {} -e {} --session {}", item.worktree, item.engine, opts.session.as_deref().unwrap_or("<team>")); }
    match item.state.as_str() {
        "live" => "skip live".to_owned(),
        "dead" => "would relaunch in place with resume".to_owned(),
        _ => format!("would fresh wake --wt {} -e {}", item.worktree, item.engine),
    }
}

fn team_t3_render_roster(title: &str, roster: &[TeamRosterItem124]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("{title}\nrole\tidentity\tengine\tstate\taction\n");
    for item in roster { writeln!(out, "{}\t{}\t{}\t{}\t{}", item.role, item.identity, item.engine, item.state, item.action).expect("write string"); }
    out
}

fn team_t3_panes() -> Vec<TeamPane124> {
    let raw = std::env::var("MAW_RS_TEAM_TMUX_PANES").unwrap_or_default();
    raw.lines().filter_map(team_t3_parse_pane).collect()
}

fn team_t3_parse_pane(line: &str) -> Option<TeamPane124> {
    let mut parts = line.split('|');
    Some(TeamPane124 { session: parts.next()?.to_owned(), window: parts.next()?.to_owned(), command: parts.next()?.to_owned(), path: parts.next().unwrap_or_default().to_owned(), pane_id: parts.next().unwrap_or_default().to_owned() })
}

fn team_t3_is_live_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    if matches!(lower.as_str(), "sh" | "bash" | "zsh" | "fish" | "-sh" | "-bash" | "-zsh") { return false; }
    lower.contains("claude") || lower.contains("codex") || lower.contains("omx") || lower.contains("node") || lower.contains("bun")
}
