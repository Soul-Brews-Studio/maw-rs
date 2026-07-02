const DISPATCH_240: &[DispatcherEntry] = &[
    DispatcherEntry { command: "team", handler: Handler::Sync(team_run_command) },
    DispatcherEntry { command: "t", handler: Handler::Sync(team_run_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamEnterOptions240 {
    subcommand: String,
    selector: String,
    text: Option<String>,
    team: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TeamEnterMember240 {
    name: String,
    display: String,
    pane_id: String,
}

#[derive(Debug, Clone)]
struct TeamEnterTmux240 {
    runner: maw_tmux::CommandTmuxRunner,
    fake_log: Option<std::path::PathBuf>,
}

impl Default for TeamEnterTmux240 {
    fn default() -> Self { Self { runner: maw_tmux::CommandTmuxRunner::default(), fake_log: std::env::var_os("MAW_RS_TEAM_ENTER_FAKE_TMUX_LOG").map(std::path::PathBuf::from) } }
}

impl TeamEnterTmux240 {
    fn team_send_literal(&mut self, pane_id: &str, text: &str) -> Result<(), String> {
        team_validate_pane_id240(pane_id)?;
        team_validate_text240(text)?;
        self.team_tmux_run240("send-keys", &maw_tmux::tmux_send_keys_literal_args(pane_id, text)).map(|_| ())
    }

    fn team_send_enter(&mut self, pane_id: &str) -> Result<(), String> {
        team_validate_pane_id240(pane_id)?;
        self.team_tmux_run240("send-keys", &maw_tmux::tmux_send_enter_args(pane_id)).map(|_| ())
    }

    fn team_probe_pane(&mut self, pane_id: &str) -> Result<(String, String), String> {
        team_validate_pane_id240(pane_id)?;
        if std::env::var_os("MAW_RS_TEAM_TMUX_PANES").is_some() {
            let matches = team_t3_panes().into_iter().filter(|pane| pane.pane_id == pane_id).collect::<Vec<_>>();
            return match matches.as_slice() {
                [pane] => Ok((pane.window.clone(), pane.command.clone())),
                [] => Err(format!("team enter refuse missing pane before send: {pane_id}")),
                _ => Err(format!("team enter refuse ambiguous pane before send: {pane_id}")),
            };
        }
        let raw = self.team_tmux_run240("display-message", &["-t".to_owned(), pane_id.to_owned(), "-p".to_owned(), "#{window_name}|#{pane_current_command}".to_owned()])?;
        let mut parts = raw.trim().splitn(2, '|');
        let window = parts.next().unwrap_or_default().to_owned();
        let command = parts.next().unwrap_or_default().to_owned();
        if window.is_empty() { return Err(format!("team enter refuse missing pane before send: {pane_id}")); }
        Ok((window, command))
    }

    fn team_tmux_run240(&mut self, command: &str, args: &[String]) -> Result<String, String> {
        if let Some(path) = &self.fake_log {
            let mut body = std::fs::read_to_string(path).unwrap_or_default();
            body.push_str(&(serde_json::json!({"program":"tmux","command":command,"args":args}).to_string() + "\n"));
            team_atomic_write_0600(path, &body)?;
            return Ok(String::new());
        }
        maw_tmux::TmuxRunner::run(&mut self.runner, command, args).map_err(|error| error.message)
    }
}

fn team_enter_send_enter(argv: &[String]) -> Result<String, String> {
    let opts = team_enter_parse240(argv)?;
    let config = team_read_json::<TeamConfig122>(&team_paths(&opts.team).tool_config).ok_or_else(|| format!("\x1b[33m⚠\x1b[0m team '{}' not found", opts.team))?;
    let members = team_enter_members240(&config, &opts)?;
    let mut tmux = TeamEnterTmux240::default();
    team_enter_run240(&opts, &members, &mut tmux)
}

fn team_enter_parse240(argv: &[String]) -> Result<TeamEnterOptions240, String> {
    let subcommand = argv.first().ok_or_else(|| "usage: maw team enter <agent|all>".to_owned())?;
    if subcommand != "enter" && subcommand != "send-enter" { return Err("usage: maw team enter <agent|all>".to_owned()); }
    let selector = argv.get(1).ok_or_else(|| "usage: maw team enter <agent|all>".to_owned())?.to_owned();
    team_validate_member_selector240(&selector)?;
    let text = if subcommand == "send-enter" && argv.len() > 2 {
        let text = argv.iter().skip(2).map(String::as_str).collect::<Vec<_>>().join(" ");
        team_validate_text240(&text)?;
        Some(text)
    } else {
        if subcommand == "enter" && argv.len() > 2 { return Err("usage: maw team enter <agent|all>".to_owned()); }
        None
    };
    Ok(TeamEnterOptions240 { subcommand: subcommand.to_owned(), selector, text, team: team_resolve_context240()? })
}

fn team_resolve_context240() -> Result<String, String> {
    if let Ok(team) = std::env::var("MAW_TEAM").map(|value| value.trim().to_owned()) {
        if !team.is_empty() { team_validate_name(&team)?; return Ok(team); }
    }
    let teams_dir = team_home_dir().join(".claude").join("teams");
    let mut live = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&teams_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if entry.path().join("config.json").exists() { live.push(name); }
        }
    }
    live.sort();
    let team = if live.len() == 1 { live.remove(0) } else { "default".to_owned() };
    team_validate_name(&team)?;
    Ok(team)
}

fn team_enter_members240(config: &TeamConfig122, opts: &TeamEnterOptions240) -> Result<Vec<TeamEnterMember240>, String> {
    let mut out = Vec::new();
    for member in config.members.iter().filter(|member| member.agent_type.as_deref() != Some("team-lead")) {
        let Some(pane_id) = member.tmux_pane_id.as_deref() else { continue; };
        if opts.selector != "all" && !team_member_matches240(member, &opts.selector, &opts.team) { continue; }
        team_validate_member_name240(&member.name)?;
        team_validate_pane_id240(pane_id)?;
        out.push(TeamEnterMember240 { name: member.name.clone(), display: member.agent_id.clone().unwrap_or_else(|| member.name.clone()), pane_id: pane_id.to_owned() });
    }
    if out.is_empty() {
        let available = config.members.iter().filter(|member| member.tmux_pane_id.is_some()).map(|member| member.name.as_str()).collect::<Vec<_>>().join(", ");
        return Err(format!("\x1b[33m⚠\x1b[0m agent '{}' not found or no pane ID\nAvailable: {}", opts.selector, if available.is_empty() { "none" } else { &available }));
    }
    Ok(out)
}

fn team_member_matches240(member: &TeamMember122, selector: &str, team: &str) -> bool {
    member.name == selector || member.agent_id.as_deref() == Some(selector) || member.agent_id.as_deref() == Some(&format!("{selector}@{team}"))
}

fn team_enter_run240(opts: &TeamEnterOptions240, members: &[TeamEnterMember240], tmux: &mut TeamEnterTmux240) -> Result<String, String> {
    use std::fmt::Write as _;
    let mut out = String::new();
    for member in members {
        team_validate_member_pane_belongs240(&opts.team, member, tmux)?;
        if let Some(text) = &opts.text { tmux.team_send_literal(&member.pane_id, text)?; }
        tmux.team_send_enter(&member.pane_id)?;
        writeln!(out, "\x1b[36m↵\x1b[0m enter sent to {}", member.display).expect("write string");
    }
    Ok(out)
}

fn team_validate_member_pane_belongs240(team: &str, member: &TeamEnterMember240, tmux: &mut TeamEnterTmux240) -> Result<(), String> {
    team_validate_name(team)?;
    team_validate_member_name240(&member.name)?;
    team_validate_pane_id240(&member.pane_id)?;
    let (window, command) = tmux.team_probe_pane(&member.pane_id)?;
    if window != member.name { return Err(format!("team enter refuse pane mismatch before send: {team}:{} != {}", window, member.name)); }
    if !team_t3_is_live_command(&command) { return Err(format!("team enter refuse dead pane before send: {team}:{}", member.pane_id)); }
    Ok(())
}

fn team_validate_member_selector240(value: &str) -> Result<(), String> {
    if value == "all" { return Ok(()); }
    team_validate_member_name240(value)
}

fn team_validate_member_name240(value: &str) -> Result<(), String> {
    if value.is_empty() { return Err("team member is empty".to_owned()); }
    if value.starts_with('-') { return Err(format!("invalid team member '{value}': leading dash rejected")); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err("invalid team member: control character rejected".to_owned()); }
    if value.contains("..") || value.contains('/') || value.contains('\\') { return Err(format!("invalid team member '{value}': path traversal rejected")); }
    Ok(())
}

fn team_validate_text240(value: &str) -> Result<(), String> {
    if value.is_empty() { return Err("team text is empty".to_owned()); }
    if value.starts_with('-') { return Err("invalid team text: leading dash rejected".to_owned()); }
    if value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err("invalid team text: control character rejected".to_owned()); }
    Ok(())
}

fn team_validate_pane_id240(value: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(|ch| ch.is_control() || ch == '\0') { return Err(format!("invalid pane id {value:?}")); }
    if !value.starts_with('%') { return Err(format!("invalid pane id {value:?}: expected tmux pane id")); }
    Ok(())
}

#[cfg(test)]
mod team_enter_tests240 {
    use super::*;

    #[test]
    fn team_enter_dispatch_fragment_owns_team_commands() {
        assert_eq!(DISPATCH_240[0].command, "team");
        assert_eq!(DISPATCH_240[1].command, "t");
    }

    #[test]
    fn team_enter_rejects_injection_values() {
        assert!(team_enter_parse240(&team_enter_strings240(&["enter", "-bad"])).expect_err("dash").contains("leading dash"));
        assert!(team_enter_parse240(&team_enter_strings240(&["send-enter", "builder", "-bad"])).expect_err("dash").contains("team text"));
        assert!(team_validate_pane_id240("pane").expect_err("pane").contains("expected tmux pane id"));
    }

    fn team_enter_strings240(args: &[&str]) -> Vec<String> { args.iter().map(|arg| (*arg).to_owned()).collect() }
}
