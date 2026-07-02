const DISPATCH_117: &[DispatcherEntry] = &[DispatcherEntry { command: "swarm", handler: Handler::Sync(swarm_run_command) }];

const SWARM_USAGE: &str = "usage: maw swarm [agents...] [--tiled] [--count N] [--parent-session-id <id>] [--session-id <id>]\n\n  maw swarm                         3 claude agents (default)\n  maw swarm claude codex opencode    one of each\n  maw swarm codex codex codex        3 codex agents\n  maw swarm --count 5                5 claude agents\n  maw swarm --tiled                  equal layout\n\nSupported: claude, codex, opencode, aider, or any command";
const SWARM_TEAM_NAME: &str = "swarm";
const SWARM_MAX_AGENTS: usize = 10;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SwarmOptions {
    agents: Vec<String>,
    tiled: bool,
    parent_session_id: Option<String>,
    session_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SwarmAgent {
    name: String,
    command: String,
    label: String,
    color: &'static str,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwarmTeamConfig {
    name: String,
    description: String,
    members: Vec<SwarmTeamMember>,
    created_at: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SwarmTeamMember {
    name: String,
    agent_id: String,
    tmux_pane_id: String,
    color: String,
    model: String,
}

fn swarm_run_command(argv: &[String]) -> CliOutput {
    let mut runner = swarm_runner_from_env();
    match swarm_with_runner(argv, runner.as_mut()) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((0, message)) => CliOutput { code: 0, stdout: format!("{message}\n"), stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn swarm_runner_from_env() -> Box<dyn maw_tmux::TmuxRunner> {
    if std::env::var_os("MAW_RS_SWARM_FAKE_TMUX").is_some() {
        Box::new(SwarmFakeTmux::default())
    } else {
        Box::new(maw_tmux::CommandTmuxRunner::new())
    }
}

fn swarm_with_runner(
    argv: &[String],
    runner: &mut dyn maw_tmux::TmuxRunner,
) -> Result<String, (i32, String)> {
    let anchor = swarm_anchor_from_env()?;
    let options = swarm_parse_args(argv)?;
    let agents = swarm_build_agents(&options)?;
    let mut panes = Vec::new();
    for _agent in &agents {
        let pane = swarm_split_pane(runner, anchor.as_deref()).map_err(|error| swarm_tmux_error(&error))?;
        swarm_validate_tmux_target(&pane).map_err(|message| (1, message))?;
        panes.push(pane);
    }
    let window = swarm_window_target(runner).unwrap_or_else(|_| anchor.clone().unwrap_or_else(|| SWARM_TEAM_NAME.to_owned()));
    swarm_apply_layout(runner, &window, options.tiled, anchor.is_some()).map_err(|error| swarm_tmux_error(&error))?;
    let mut stdout = String::new();
    let mut members = Vec::new();
    for (agent, pane) in agents.iter().zip(panes.iter()) {
        swarm_start_agent(runner, agent, pane, &options).map_err(|error| swarm_tmux_error(&error))?;
        stdout.push_str(&swarm_agent_line(agent, pane));
        members.push(swarm_member(agent, pane));
    }
    swarm_write_team_config(&members).map_err(|message| (1, message))?;
    let layout = if options.tiled { "tiled" } else { "main-vertical" };
    swarm_push_success_line(&mut stdout, agents.len(), layout);
    Ok(stdout)
}

fn swarm_parse_args(argv: &[String]) -> Result<SwarmOptions, (i32, String)> {
    if argv.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Err((0, SWARM_USAGE.to_owned()));
    }
    let mut agents = Vec::new();
    let mut tiled = false;
    let mut count = None;
    let mut parent_session_id = None;
    let mut session_id = None;
    let mut index = 0usize;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--tiled" => tiled = true,
            "--count" => {
                count = Some(swarm_parse_count(argv.get(index + 1))?);
                index += 1;
            }
            "--parent" | "--parent-session-id" => parent_session_id = Some(swarm_take_value(argv, &mut index, arg)?),
            "--session-id" => session_id = Some(swarm_take_value(argv, &mut index, arg)?),
            "--" => return Err((2, "swarm does not accept -- separator".to_owned())),
            value if value.starts_with("--count=") => count = Some(swarm_parse_count_value(&value[8..])?),
            "--wt" | "--worktree" => return Err((1, "✗ unknown flag for swarm: --wt. maw swarm is shared-cwd only; for an isolated worktree-per-member use: maw wake <oracle> --wt <slot> --split -e <engine>".to_owned())),
            value if value.starts_with('-') => return Err((1, format!("✗ unknown flag for swarm: {value} (supported: --tiled, --count, --help, -h, --parent, --parent-session-id, --session-id)"))),
            value => agents.push(swarm_validate_agent_value(value)?),
        }
        index += 1;
    }
    if agents.is_empty() { agents = vec!["claude".to_owned(); count.unwrap_or(3)]; }
    if agents.len() > SWARM_MAX_AGENTS { return Err((1, "⚠ max 10".to_owned())); }
    Ok(SwarmOptions { agents, tiled, parent_session_id, session_id })
}

fn swarm_parse_count(value: Option<&String>) -> Result<usize, (i32, String)> {
    let Some(value) = value else { return Err((2, "swarm: --count requires a value".to_owned())); };
    swarm_parse_count_value(value)
}

fn swarm_parse_count_value(value: &str) -> Result<usize, (i32, String)> {
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err((2, "swarm: --count requires a positive integer".to_owned()));
    }
    value.parse::<usize>().ok().filter(|count| (1..=SWARM_MAX_AGENTS).contains(count)).ok_or_else(|| (1, "⚠ max 10".to_owned()))
}

fn swarm_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, (i32, String)> {
    *index += 1;
    let Some(value) = argv.get(*index) else { return Err((2, format!("swarm: {flag} requires a value"))); };
    swarm_validate_session_value(flag, value).map_err(|message| (2, message))
}

fn swarm_validate_agent_value(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err((2, "swarm: agent values must be non-empty, unpadded, and not start with '-'".to_owned()));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':')) {
        return Err((2, "swarm: agent values must be command names or paths without shell metacharacters".to_owned()));
    }
    Ok(value.to_owned())
}

fn swarm_validate_session_value(flag: &str, value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("swarm: {flag} must be non-empty, unpadded, and not start with '-'"));
    }
    Ok(value.to_owned())
}

fn swarm_anchor_from_env() -> Result<Option<String>, (i32, String)> {
    if std::env::var_os("TMUX").is_none() { return Err((1, "⚠ swarm requires tmux".to_owned())); }
    match std::env::var("TMUX_PANE") {
        Ok(value) if !value.is_empty() => {
            swarm_validate_tmux_target(&value).map_err(|message| (1, message))?;
            Ok(Some(value))
        }
        _ => Ok(None),
    }
}

fn swarm_build_agents(options: &SwarmOptions) -> Result<Vec<SwarmAgent>, (i32, String)> {
    options.agents.iter().enumerate().map(|(index, raw)| {
        let (command, label) = swarm_engine(raw);
        let name = format!("{raw}-{}", index + 1);
        Ok(SwarmAgent { name, command, label, color: swarm_color(index) })
    }).collect()
}

fn swarm_engine(raw: &str) -> (String, String) {
    match raw {
        "claude" => ("claude".to_owned(), "Claude Code".to_owned()),
        "codex" => ("codex".to_owned(), "Codex CLI".to_owned()),
        "opencode" => ("opencode".to_owned(), "OpenCode".to_owned()),
        "aider" => ("aider".to_owned(), "Aider".to_owned()),
        custom => (custom.to_owned(), custom.to_owned()),
    }
}

fn swarm_color(index: usize) -> &'static str {
    ["blue", "green", "yellow", "cyan", "magenta", "red", "white", "blue", "green", "yellow"][index % 10]
}

fn swarm_split_pane(
    runner: &mut dyn maw_tmux::TmuxRunner,
    anchor: Option<&str>,
) -> Result<String, maw_tmux::TmuxError> {
    let mut args = Vec::new();
    if let Some(anchor) = anchor { args.extend(["-t".to_owned(), anchor.to_owned()]); }
    args.extend(["-h".to_owned(), "-P".to_owned(), "-F".to_owned(), "#{pane_id}".to_owned(), "exec zsh -li".to_owned()]);
    Ok(runner.run("split-window", &args)?.trim().to_owned())
}

fn swarm_window_target(runner: &mut dyn maw_tmux::TmuxRunner) -> Result<String, maw_tmux::TmuxError> {
    let raw = runner.run("display-message", &["-p".to_owned(), "#S:#I".to_owned()])?;
    Ok(raw.trim().to_owned())
}

fn swarm_apply_layout(
    runner: &mut dyn maw_tmux::TmuxRunner,
    window: &str,
    tiled: bool,
    _anchored: bool,
) -> Result<(), maw_tmux::TmuxError> {
    let layout = if tiled { "tiled" } else { "main-vertical" };
    runner.run("select-layout", &["-t".to_owned(), window.to_owned(), layout.to_owned()])?;
    runner.run("set-window-option", &["-t".to_owned(), window.to_owned(), "pane-border-status".to_owned(), "top".to_owned()])?;
    Ok(())
}

fn swarm_start_agent(
    runner: &mut dyn maw_tmux::TmuxRunner,
    agent: &SwarmAgent,
    pane: &str,
    options: &SwarmOptions,
) -> Result<(), maw_tmux::TmuxError> {
    let label = format!("{} ({})", agent.name, agent.label);
    runner.run("select-pane", &["-t".to_owned(), pane.to_owned(), "-T".to_owned(), label])?;
    let command = swarm_command_with_env(agent, options);
    let shell_line = format!("{}; printf '\\e[?1049l'; clear; exec zsh -li", swarm_shell_quote(&command));
    runner.run("send-keys", &["-t".to_owned(), pane.to_owned(), shell_line, "Enter".to_owned()])?;
    Ok(())
}

fn swarm_command_with_env(agent: &SwarmAgent, options: &SwarmOptions) -> String {
    let mut envs = Vec::new();
    if let Some(parent) = &options.parent_session_id { envs.push(format!("MAW_PARENT_SESSION_ID={}", swarm_shell_quote(parent))); }
    if options.agents.len() == 1 {
        if let Some(session) = &options.session_id { envs.push(format!("MAW_SESSION_ID={}", swarm_shell_quote(session))); }
    }
    if envs.is_empty() { agent.command.clone() } else { format!("{} {}", envs.join(" "), agent.command) }
}

fn swarm_shell_quote(value: &str) -> String {
    if value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '@')) {
        return value.to_owned();
    }
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn swarm_member(agent: &SwarmAgent, pane: &str) -> SwarmTeamMember {
    SwarmTeamMember {
        name: agent.name.clone(),
        agent_id: format!("{}@{SWARM_TEAM_NAME}", agent.name),
        tmux_pane_id: pane.to_owned(),
        color: agent.color.to_owned(),
        model: agent.command.clone(),
    }
}

fn swarm_agent_line(agent: &SwarmAgent, pane: &str) -> String {
    format!("  {}●\x1b[0m {} ({}) → {pane}\n", swarm_ansi(agent.color), agent.name, agent.label)
}


fn swarm_push_success_line(stdout: &mut String, count: usize, layout: &str) {
    stdout.push_str("\x1b[32m✓\x1b[0m swarm: ");
    stdout.push_str(&count.to_string());
    stdout.push_str(" agents (");
    stdout.push_str(layout);
    stdout.push_str(")\n");
}

fn swarm_ansi(color: &str) -> &'static str {
    match color {
        "blue" => "\x1b[34m",
        "green" => "\x1b[32m",
        "yellow" => "\x1b[33m",
        "cyan" => "\x1b[36m",
        "magenta" => "\x1b[35m",
        "red" => "\x1b[31m",
        _ => "\x1b[37m",
    }
}

fn swarm_config_path() -> std::path::PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    home.join(".claude").join("teams").join(SWARM_TEAM_NAME).join("config.json")
}

fn swarm_write_team_config(members: &[SwarmTeamMember]) -> Result<(), String> {
    let path = swarm_config_path();
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| format!("swarm: create team config dir: {error}"))?; }
    let mut config = swarm_read_team_config(&path).unwrap_or_else(swarm_default_config);
    for member in members {
        if let Some(existing) = config.members.iter_mut().find(|existing| existing.name == member.name) {
            *existing = member.clone();
        } else {
            config.members.push(member.clone());
        }
    }
    let body = serde_json::to_string_pretty(&config).map_err(|error| format!("swarm: serialize team config: {error}"))? + "\n";
    std::fs::write(&path, body).map_err(|error| format!("swarm: write team config: {error}"))
}

fn swarm_read_team_config(path: &std::path::Path) -> Option<SwarmTeamConfig> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn swarm_default_config() -> SwarmTeamConfig {
    let created_at = std::env::var("MAW_RS_SWARM_FAKE_NOW").ok().and_then(|raw| raw.parse::<u64>().ok()).unwrap_or(0);
    SwarmTeamConfig { name: SWARM_TEAM_NAME.to_owned(), description: "Multi-AI swarm".to_owned(), members: Vec::new(), created_at }
}

fn swarm_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err("swarm: tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err("swarm: tmux target must not contain whitespace or control characters".to_owned());
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '%' | '-')) {
        return Err("swarm: tmux target contains unsupported characters".to_owned());
    }
    if value.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("swarm: bare numeric tmux targets are refused; use session:window or %pane_id".to_owned());
    }
    Ok(())
}

fn swarm_tmux_error(error: &maw_tmux::TmuxError) -> (i32, String) { (1, format!("swarm tmux failed: {}", error.message)) }

#[derive(Debug, Default)]
struct SwarmFakeTmux { next_pane: usize }

impl maw_tmux::TmuxRunner for SwarmFakeTmux {
    fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
        if let Ok(path) = std::env::var("MAW_RS_SWARM_FAKE_LOG") {
            swarm_fake_append_log(&path, subcommand, args);
        }
        match subcommand {
            "split-window" => {
                self.next_pane += 1;
                Ok(format!("%pane{}\n", self.next_pane))
            }
            "display-message" => Ok("swarm-window:1\n".to_owned()),
            _ => Ok(String::new()),
        }
    }
}

fn swarm_fake_append_log(path: &str, subcommand: &str, args: &[String]) {
    use std::io::Write as _;
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{} {}", subcommand, args.join(" "));
    }
}
