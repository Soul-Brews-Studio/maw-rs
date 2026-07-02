fn team_send(argv: &[String]) -> Result<String, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw team send <team> <message>".to_owned())?;
    team_validate_name(team)?;
    let send_args = argv.iter().skip(2).cloned().collect::<Vec<_>>();
    let mode = team_resolve_send_mode(&send_args, &team_message_targets(team))?;
    match mode {
        TeamSendMode122::Single { agent, message } => team_send_single(team, &agent, &message),
        TeamSendMode122::Broadcast { message } => team_send_broadcast(team, &message),
    }
}

fn team_broadcast(argv: &[String]) -> Result<String, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw team broadcast <team> <message>".to_owned())?;
    team_validate_name(team)?;
    let message = argv.iter().skip(2).map(String::as_str).collect::<Vec<_>>().join(" ");
    team_validate_message(&message)?;
    team_send_broadcast(team, &message)
}

fn team_inbox(argv: &[String]) -> Result<String, String> {
    let team = argv.get(1).ok_or_else(|| "usage: maw team inbox <team> <agent>".to_owned())?;
    let agent = argv.get(2).ok_or_else(|| "usage: maw team inbox <team> <agent>".to_owned())?;
    team_validate_name(team)?;
    team_validate_target(agent)?;
    Ok(team_render_inbox(team, agent))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TeamSendMode122 { Broadcast { message: String }, Single { agent: String, message: String } }

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
struct TeamInboxMessage122 {
    from: String,
    text: String,
    summary: String,
    timestamp: String,
    read: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq, Default)]
struct TeamVaultMessage122 {
    from: String,
    team: String,
    text: String,
    timestamp: String,
}

fn team_resolve_send_mode(args: &[String], targets: &[String]) -> Result<TeamSendMode122, String> {
    if args.is_empty() { return Err("usage: maw team send <team> <message>".to_owned()); }
    if args.len() == 1 { team_validate_message(&args[0])?; return Ok(TeamSendMode122::Broadcast { message: args[0].clone() }); }
    let first = args[0].clone();
    if targets.is_empty() || targets.iter().any(|target| target == &first) {
        team_validate_target(&first)?;
        let message = args[1..].join(" ");
        team_validate_message(&message)?;
        return Ok(TeamSendMode122::Single { agent: first, message });
    }
    let message = args.join(" ");
    team_validate_message(&message)?;
    Ok(TeamSendMode122::Broadcast { message })
}

fn team_send_single(team: &str, agent: &str, message: &str) -> Result<String, String> {
    if team_read_json::<TeamConfig122>(&team_paths(team).tool_config).is_some() {
        team_write_live_message(team, agent, message)?;
        return Ok(format!("\x1b[32m✓\x1b[0m message sent to {agent} in live team '{team}'\n"));
    }
    team_write_vault_message(team, agent, message)?;
    Ok(format!("\x1b[32m✓\x1b[0m message written to ψ/memory/mailbox/{agent}/ (team not live)\n"))
}

fn team_send_broadcast(team: &str, message: &str) -> Result<String, String> {
    use std::fmt::Write as _;
    let targets = team_message_targets(team);
    if targets.is_empty() { return Err(format!("no members in team '{team}' — add members before broadcasting")); }
    let mut out = format!("\x1b[36m⚡\x1b[0m broadcast to {} member(s) in team '{team}':\n", targets.len());
    for target in &targets { team_send_single_quiet(team, target, message)?; }
    writeln!(out, "\x1b[32m✓\x1b[0m broadcast delivered to {} member(s)", targets.len()).expect("write string");
    Ok(out)
}

fn team_send_single_quiet(team: &str, agent: &str, message: &str) -> Result<(), String> {
    if team_read_json::<TeamConfig122>(&team_paths(team).tool_config).is_some() { team_write_live_message(team, agent, message) } else { team_write_vault_message(team, agent, message) }
}

fn team_message_targets(team: &str) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    if let Some(registry) = team_read_oracle_registry(team) {
        for member in registry.members { team_push_unique_target(&mut seen, &mut out, &member.oracle); }
    }
    if let Some(config) = team_read_json::<TeamConfig122>(&team_paths(team).tool_config) {
        for member in config.members.iter().filter(|m| m.agent_type.as_deref() != Some("team-lead")) { team_push_unique_target(&mut seen, &mut out, &member.name); }
    }
    out
}

fn team_push_unique_target(seen: &mut std::collections::BTreeSet<String>, out: &mut Vec<String>, value: &str) {
    if !value.is_empty() && seen.insert(value.to_owned()) { out.push(value.to_owned()); }
}

fn team_write_live_message(team: &str, agent: &str, message: &str) -> Result<(), String> {
    let path = team_paths(team).tool_dir.join("inboxes").join(format!("{agent}.json"));
    let mut messages = team_read_json::<Vec<TeamInboxMessage122>>(&path).unwrap_or_default();
    messages.push(TeamInboxMessage122 {
        from: "maw-team-send".to_owned(),
        text: serde_json::json!({"type":"message","content":message}).to_string(),
        summary: team_summary(message),
        timestamp: team_timestamp(),
        read: false,
    });
    team_write_json_atomic_0600(&path, &messages)
}

fn team_write_vault_message(team: &str, agent: &str, message: &str) -> Result<(), String> {
    let dir = team_psi_dir().join("memory").join("mailbox").join(agent);
    let stamp = team_now_millis();
    let path = dir.join(format!("msg-{stamp}.json"));
    let entry = TeamVaultMessage122 { from: "maw-team-send".to_owned(), team: team.to_owned(), text: message.to_owned(), timestamp: team_timestamp() };
    team_write_json_atomic_0600(&path, &entry)
}

fn team_render_inbox(team: &str, agent: &str) -> String {
    let live = team_paths(team).tool_dir.join("inboxes").join(format!("{agent}.json"));
    if let Some(messages) = team_read_json::<Vec<TeamInboxMessage122>>(&live) { return team_render_live_inbox(team, agent, &messages); }
    let vault = team_psi_dir().join("memory").join("mailbox").join(agent);
    team_render_vault_inbox(team, agent, &vault)
}

fn team_render_live_inbox(team: &str, agent: &str, messages: &[TeamInboxMessage122]) -> String {
    use std::fmt::Write as _;
    let mut out = format!("\x1b[36mℹ\x1b[0m inbox for {agent} in team '{team}' ({}):\n", messages.len());
    for message in messages { writeln!(out, "  - {}: {}", message.from, message.summary).expect("write string"); }
    out
}

fn team_render_vault_inbox(team: &str, agent: &str, dir: &std::path::Path) -> String {
    use std::fmt::Write as _;
    let mut entries = Vec::new();
    if let Ok(files) = std::fs::read_dir(dir) {
        for file in files.flatten() {
            if let Some(message) = team_read_json::<TeamVaultMessage122>(&file.path()).filter(|m| m.team == team) { entries.push(message); }
        }
    }
    let mut out = format!("\x1b[36mℹ\x1b[0m vault inbox for {agent} from team '{team}' ({}):\n", entries.len());
    for message in entries { writeln!(out, "  - {}: {}", message.from, team_summary(&message.text)).expect("write string"); }
    out
}

fn team_validate_target(target: &str) -> Result<(), String> {
    if target.is_empty() { return Err("team target is empty".to_owned()); }
    if target.starts_with('-') { return Err(format!("unsafe team target '{target}': leading dash rejected")); }
    if target.chars().any(|ch| ch.is_control() || ch == '\0') { return Err("unsafe team target: control character rejected".to_owned()); }
    Ok(())
}

fn team_validate_message(message: &str) -> Result<(), String> {
    if message.is_empty() { return Err("team message is empty".to_owned()); }
    if message.starts_with('-') { return Err("unsafe team message: leading dash rejected".to_owned()); }
    if message.chars().any(|ch| ch.is_control() || ch == '\0') { return Err("unsafe team message: control character rejected".to_owned()); }
    Ok(())
}

fn team_summary(message: &str) -> String {
    message.chars().take(80).collect()
}

fn team_timestamp() -> String {
    std::env::var("MAW_RS_TEAM_FIXED_TIME").unwrap_or_else(|_| team_now_millis().to_string())
}
