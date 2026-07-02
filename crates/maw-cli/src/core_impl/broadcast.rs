const DISPATCH_55: &[DispatcherEntry] = &[ DispatcherEntry { command: "broadcast", handler: Handler::Sync(run_broadcast_command) } ];

const BROADCAST_USAGE: &str = "usage: maw broadcast <message> [--session <name>] [--team <name>] [--fleet <name>]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct BroadcastScope { session: Option<String>, team: Option<String>, fleet: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct BroadcastOptions { message: String, scope: BroadcastScope }

trait BroadcastTmux {
    fn broadcast_current_window(&mut self) -> Option<String>;
    fn broadcast_list_all(&mut self) -> Vec<TmuxSession>;
    fn broadcast_pane_command(&mut self, target: &str) -> Result<String, String>;
    fn broadcast_send_text(&mut self, target: &str, text: &str) -> Result<(), String>;
}

#[derive(Default)]
struct BroadcastLocalTmux { runner: maw_tmux::CommandTmuxRunner }

impl BroadcastTmux for BroadcastLocalTmux {
    fn broadcast_current_window(&mut self) -> Option<String> {
        maw_tmux::TmuxRunner::run(&mut self.runner, "display-message", &["-p".to_owned(), "#{window_name}".to_owned()]).ok().map(|value| value.trim().to_owned()).filter(|value| !value.is_empty())
    }

    fn broadcast_list_all(&mut self) -> Vec<TmuxSession> {
        maw_tmux::TmuxRunner::run(&mut self.runner, "list-windows", &["-a".to_owned(), "-F".to_owned(), "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}".to_owned()]).map(|raw| maw_tmux::parse_list_all_windows(&raw)).unwrap_or_default()
    }

    fn broadcast_pane_command(&mut self, target: &str) -> Result<String, String> {
        broadcast_validate_tmux_target(target)?;
        maw_tmux::TmuxRunner::run(&mut self.runner, "display-message", &["-t".to_owned(), target.to_owned(), "-p".to_owned(), "#{pane_current_command}".to_owned()]).map_err(|error| error.message)
    }

    fn broadcast_send_text(&mut self, target: &str, text: &str) -> Result<(), String> {
        broadcast_validate_tmux_target(target)?;
        maw_tmux::TmuxRunner::run(&mut self.runner, "send-keys", &maw_tmux::tmux_send_keys_literal_args(target, text)).map_err(|error| error.message)?;
        maw_tmux::TmuxRunner::run(&mut self.runner, "send-keys", &maw_tmux::tmux_send_enter_args(target)).map_err(|error| error.message)?;
        Ok(())
    }
}

fn run_broadcast_command(argv: &[String]) -> CliOutput {
    let options = match broadcast_parse_args(argv) {
        Ok(options) => options,
        Err(message) => return CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    };
    match broadcast_run(&options, &mut BroadcastLocalTmux::default()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn broadcast_parse_args(argv: &[String]) -> Result<BroadcastOptions, String> {
    let mut scope = BroadcastScope::default();
    let mut message_parts = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        if matches!(arg.as_str(), "--session" | "--team" | "--fleet") {
            index += 1;
            let Some(value) = argv.get(index) else { return Err(format!("{arg} requires a value\n{BROADCAST_USAGE}")); };
            broadcast_validate_scope_value(value).map_err(|_| format!("{arg} requires a value\n{BROADCAST_USAGE}"))?;
            match arg.as_str() {
                "--session" => scope.session = Some(value.clone()),
                "--team" => scope.team = Some(value.clone()),
                "--fleet" => scope.fleet = Some(value.clone()),
                _ => unreachable!(),
            }
        } else {
            message_parts.push(arg.clone());
        }
        index += 1;
    }
    let message = message_parts.join(" ").trim().to_owned();
    if message.is_empty() { return Err(BROADCAST_USAGE.to_owned()); }
    Ok(BroadcastOptions { message, scope })
}

fn broadcast_run<T: BroadcastTmux>(options: &BroadcastOptions, tmux: &mut T) -> Result<String, String> {
    let sender = tmux.broadcast_current_window().unwrap_or_else(|| "unknown".to_owned());
    let message = format!("[broadcast from {sender}] {}", options.message);
    let team_members = options.scope.team.as_deref().map(broadcast_team_member_names);
    let fleet_sessions = options.scope.fleet.as_deref().map(broadcast_fleet_session_names);
    let mut sent = 0usize;
    let mut skipped = 0usize;
    let mut reasons = BTreeMap::<String, usize>::new();
    let mut out = String::new();

    for session in tmux.broadcast_list_all() {
        if broadcast_skip_session(&session.name) { continue; }
        if options.scope.session.as_deref().is_some_and(|wanted| wanted != session.name) { continue; }
        if fleet_sessions.as_ref().is_some_and(|names| !names.contains(&session.name)) { continue; }
        broadcast_validate_tmux_target(&session.name)?;
        for window in session.windows {
            if team_members.as_ref().is_some_and(|members| !broadcast_window_matches_team_member(&session.name, &window.name, members)) { continue; }
            let target = format!("{}:{}", session.name, window.index);
            broadcast_validate_tmux_target(&target)?;
            match tmux.broadcast_pane_command(&target) {
                Ok(command) if broadcast_is_agent_command(&command) => {
                    if tmux.broadcast_send_text(&target, &message).is_ok() {
                        let _ = writeln!(out, "\x1b[32msent\x1b[0m → {}:{}", session.name, window.name);
                        sent += 1;
                    } else {
                        skipped += 1;
                        *reasons.entry("exception".to_owned()).or_default() += 1;
                    }
                }
                Ok(_) => { skipped += 1; *reasons.entry("non-agent-pane".to_owned()).or_default() += 1; }
                Err(_) => { skipped += 1; *reasons.entry("exception".to_owned()).or_default() += 1; }
            }
        }
    }
    let _ = writeln!(out, "\n\x1b[32m✓\x1b[0m Broadcast to {sent} windows ({skipped} skipped) [scope: {}]", broadcast_scope_description(&options.scope));
    if skipped > 0 {
        out.push_str("  \x1b[90mskipped breakdown:\x1b[0m\n");
        for (reason, count) in reasons { let _ = writeln!(out, "    \x1b[90m{reason}: {count}\x1b[0m"); }
    }
    Ok(out)
}

fn broadcast_validate_scope_value(value: &str) -> Result<(), String> { broadcast_validate_tmux_target(value) }

fn broadcast_validate_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') { Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned()) } else { Ok(()) }
}

fn broadcast_skip_session(name: &str) -> bool { name == "99-overview" || name == "scratch" || name.ends_with("-view") }

fn broadcast_is_agent_command(command: &str) -> bool {
    let command = command.to_ascii_lowercase();
    command.contains("claude") || command.contains("codex") || command.contains("node") || command.contains("thclaws")
}

fn broadcast_scope_description(scope: &BroadcastScope) -> String {
    let mut parts = Vec::new();
    if let Some(value) = &scope.session { parts.push(format!("session={value}")); }
    if let Some(value) = &scope.team { parts.push(format!("team={value}")); }
    if let Some(value) = &scope.fleet { parts.push(format!("fleet={value}")); }
    if parts.is_empty() { "all agents".to_owned() } else { parts.join(", ") }
}

fn broadcast_normalized_names(value: &str) -> BTreeSet<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() { return BTreeSet::new(); }
    let stripped = broadcast_strip_numeric_prefix(trimmed);
    [trimmed.to_owned(), stripped.clone(), broadcast_strip_oracle_suffix(trimmed), broadcast_strip_oracle_suffix(&stripped), format!("{stripped}-oracle")].into_iter().filter(|value| !value.is_empty()).collect()
}

fn broadcast_strip_numeric_prefix(value: &str) -> String { value.split_once('-').filter(|(head, _)| head.chars().all(|ch| ch.is_ascii_digit())).map_or_else(|| value.to_owned(), |(_, tail)| tail.to_owned()) }

fn broadcast_strip_oracle_suffix(value: &str) -> String { value.strip_suffix("-oracle").or_else(|| value.strip_suffix("-ORACLE")).unwrap_or(value).to_owned() }

fn broadcast_window_matches_team_member(session: &str, window: &str, members: &BTreeSet<String>) -> bool {
    broadcast_normalized_names(session).into_iter().chain(broadcast_normalized_names(window)).any(|name| members.contains(&name))
}

fn broadcast_team_member_names(team: &str) -> BTreeSet<String> {
    let mut members = BTreeSet::new();
    for name in broadcast_team_config_member_names(team).into_iter().chain(broadcast_team_manifest_member_names(team)) { members.extend(broadcast_normalized_names(&name)); }
    members
}

fn broadcast_read_json(path: &std::path::Path) -> Option<serde_json::Value> { std::fs::read_to_string(path).ok().and_then(|text| serde_json::from_str(&text).ok()) }

fn broadcast_team_config_member_names(team: &str) -> Vec<String> {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let Some(json) = broadcast_read_json(&home.join(".claude").join("teams").join(team).join("config.json")) else { return Vec::new(); };
    json.get("members").and_then(serde_json::Value::as_array).into_iter().flatten().filter(|member| member.get("agentType").and_then(serde_json::Value::as_str) != Some("team-lead") && member.get("role").and_then(serde_json::Value::as_str) != Some("lead") && member.get("name").and_then(serde_json::Value::as_str) != Some("team-lead")).filter_map(|member| member.get("name").and_then(serde_json::Value::as_str).map(ToOwned::to_owned)).collect()
}

fn broadcast_resolve_psi() -> std::path::PathBuf {
    let mut dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    loop {
        if dir.join("ψ").is_dir() && dir.join("CLAUDE.md").is_file() { return dir.join("ψ"); }
        if !dir.pop() { break; }
    }
    std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")).join("ψ")
}

fn broadcast_team_manifest_member_names(team: &str) -> Vec<String> {
    let Some(json) = broadcast_read_json(&broadcast_resolve_psi().join("memory").join("mailbox").join("teams").join(team).join("manifest.json")) else { return Vec::new(); };
    let mut out = Vec::new();
    if let Some(values) = json.get("members").and_then(serde_json::Value::as_array) { for entry in values { if let Some(value) = entry.as_str().or_else(|| entry.get("name").and_then(serde_json::Value::as_str)) { out.push(value.to_owned()); } } }
    if let Some(values) = json.pointer("/charter/members").and_then(serde_json::Value::as_array) { for entry in values { if let Some(value) = entry.get("name").and_then(serde_json::Value::as_str).or_else(|| entry.get("role").and_then(serde_json::Value::as_str)) { out.push(value.to_owned()); } } }
    out
}

fn broadcast_fleet_session_names(fleet: &str) -> BTreeSet<String> {
    let wanted = broadcast_normalized_names(fleet);
    let fleet_dir = active_config_dir().join("fleet");
    let Ok(entries) = std::fs::read_dir(fleet_dir) else { return BTreeSet::new(); };
    let mut sessions = BTreeSet::new();
    for path in entries.flatten().map(|entry| entry.path()).filter(|path| path.extension().and_then(std::ffi::OsStr::to_str) == Some("json")) {
        let Some(json) = broadcast_read_json(&path) else { continue; };
        let name = json.get("name").and_then(serde_json::Value::as_str).unwrap_or_default();
        let file = path.file_stem().and_then(std::ffi::OsStr::to_str).unwrap_or_default();
        let candidates = [json.get("groupName").and_then(serde_json::Value::as_str).unwrap_or_default(), file, name, &broadcast_strip_numeric_prefix(name)];
        if candidates.iter().any(|candidate| wanted.contains(*candidate)) && !name.is_empty() { sessions.insert(name.to_owned()); }
    }
    sessions
}

#[cfg(test)]
mod broadcast_tests {
    use super::*;

    #[derive(Default)]
    struct BroadcastMockTmux { sessions: Vec<TmuxSession>, calls: Vec<(String, String)> }
    impl BroadcastTmux for BroadcastMockTmux {
        fn broadcast_current_window(&mut self) -> Option<String> { Some("sender".to_owned()) }
        fn broadcast_list_all(&mut self) -> Vec<TmuxSession> { self.sessions.clone() }
        fn broadcast_pane_command(&mut self, target: &str) -> Result<String, String> { self.calls.push(("cmd".to_owned(), target.to_owned())); Ok(if target.ends_with(":1") { "bash" } else { "codex" }.to_owned()) }
        fn broadcast_send_text(&mut self, target: &str, text: &str) -> Result<(), String> { self.calls.push(("send".to_owned(), format!("{target}|{text}"))); Ok(()) }
    }

    fn broadcast_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn broadcast_parser_matches_plugin_flags_and_guards_scope_values() {
        let parsed = broadcast_parse_args(&broadcast_strings(&["hello", "fleet", "--session", "alpha", "--team", "tk", "--fleet", "all"])).expect("parse");
        assert_eq!(parsed.message, "hello fleet");
        assert_eq!(parsed.scope.session.as_deref(), Some("alpha"));
        assert!(broadcast_parse_args(&broadcast_strings(&["hello", "--session", "-Sbad"])).expect_err("guard").contains(BROADCAST_USAGE));
    }

    #[test]
    fn broadcast_run_sends_to_agents_and_reports_skips() {
        let mut tmux = BroadcastMockTmux { sessions: vec![TmuxSession { name: "alpha".to_owned(), windows: vec![maw_tmux::TmuxWindow { index: 0, name: "agent".to_owned(), active: false, cwd: None }, maw_tmux::TmuxWindow { index: 1, name: "shell".to_owned(), active: false, cwd: None }] }], calls: Vec::new() };
        let out = broadcast_run(&BroadcastOptions { message: "hi".to_owned(), scope: BroadcastScope { session: Some("alpha".to_owned()), ..BroadcastScope::default() } }, &mut tmux).expect("run");
        assert!(out.contains("Broadcast to 1 windows (1 skipped) [scope: session=alpha]"));
        assert!(tmux.calls.iter().any(|call| call.1.contains("[broadcast from sender] hi")));
    }
}
