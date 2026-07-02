const DISPATCH_70: &[DispatcherEntry] = &[
    DispatcherEntry {
        command: "agents",
        handler: Handler::Sync(agents_run_command),
    },
    DispatcherEntry {
        command: "agent",
        handler: Handler::Sync(agents_run_command),
    },
];

const AGENTS_USAGE: &str = "usage: maw agents [--json] [--all] [--node <node>]";
const AGENTS_ORACLE_SUFFIX: &str = "-oracle";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct AgentsOptions {
    json: bool,
    all: bool,
    node: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, PartialEq, Eq)]
struct AgentsRow {
    node: String,
    session: String,
    window: String,
    oracle: String,
    state: String,
    pid: Option<u32>,
}

trait AgentsRuntime {
    fn agents_node(&self) -> String;
    fn agents_routes(&self) -> HashMap<String, String>;
    fn agents_sessions(&mut self) -> Vec<TmuxSession>;
    fn agents_panes(&mut self) -> Vec<TmuxPane>;
}

struct AgentsSystemRuntime;

impl AgentsRuntime for AgentsSystemRuntime {
    fn agents_node(&self) -> String {
        agents_load_node().unwrap_or_else(|| "local".to_owned())
    }

    fn agents_routes(&self) -> HashMap<String, String> {
        load_hey_config().route.agents
    }

    fn agents_sessions(&mut self) -> Vec<TmuxSession> {
        TmuxClient::local().list_all()
    }

    fn agents_panes(&mut self) -> Vec<TmuxPane> {
        TmuxClient::local().list_panes()
    }
}

fn agents_run_command(argv: &[String]) -> CliOutput {
    match agents_run(argv, &mut AgentsSystemRuntime) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn agents_run(argv: &[String], runtime: &mut impl AgentsRuntime) -> Result<String, String> {
    if agents_has_help(argv) {
        return Ok(format!("{AGENTS_USAGE}\n"));
    }
    let options = agents_parse_args(argv)?;
    if let Some(node) = &options.node {
        let local_node = runtime.agents_node();
        let routes = runtime.agents_routes();
        let rows = agents_build_node_rows(&routes, node, &local_node);
        if options.json {
            return agents_render_json(&rows);
        }
        return Ok(agents_render_table(&rows));
    }
    let node = runtime.agents_node();
    let sessions = runtime.agents_sessions();
    let panes = runtime.agents_panes();
    let rows = agents_build_rows(&panes, &sessions, &node, options.all);
    if options.json {
        return agents_render_json(&rows);
    }
    Ok(agents_render_table(&rows))
}

fn agents_parse_args(argv: &[String]) -> Result<AgentsOptions, String> {
    if argv
        .iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Err(AGENTS_USAGE.to_owned());
    }
    let mut options = AgentsOptions::default();
    let mut index = 0_usize;
    while index < argv.len() {
        agents_parse_arg(argv, &mut index, &mut options)?;
        index += 1;
    }
    Ok(options)
}

fn agents_has_help(argv: &[String]) -> bool {
    argv.iter()
        .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
}

fn agents_parse_arg(
    argv: &[String],
    index: &mut usize,
    options: &mut AgentsOptions,
) -> Result<(), String> {
    match argv[*index].as_str() {
        "--json" => options.json = true,
        "--all" => options.all = true,
        "--node" => {
            let value = agents_required_value(argv, *index, "--node")?;
            agents_validate_value(value, "node")?;
            options.node = Some(value.to_owned());
            *index += 1;
        }
        value if value.starts_with("--node=") => {
            let value = value.trim_start_matches("--node=");
            agents_validate_value(value, "node")?;
            options.node = Some(value.to_owned());
        }
        value if value.starts_with('-') => return Err(format!("agents: unknown argument {value}")),
        value => return Err(format!("agents: unexpected argument {value}")),
    }
    Ok(())
}

fn agents_required_value<'a>(
    argv: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1) else {
        return Err(format!("agents: missing {flag} value"));
    };
    if value.starts_with('-') {
        return Err(format!("agents: {flag} value must not start with '-'"));
    }
    Ok(value)
}

fn agents_validate_value(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!("agents: invalid {label}"));
    }
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("agents: invalid {label}"));
    }
    Ok(())
}

fn agents_build_rows(
    panes: &[TmuxPane],
    sessions: &[TmuxSession],
    node: &str,
    all: bool,
) -> Vec<AgentsRow> {
    let window_names = agents_window_names(sessions);
    let mut rows = Vec::new();
    for pane in panes {
        if let Some(row) = agents_row_from_pane(pane, &window_names, node, all) {
            rows.push(row);
        }
    }
    rows
}

fn agents_window_names(sessions: &[TmuxSession]) -> HashMap<String, String> {
    let mut names = HashMap::new();
    for session in sessions {
        for window in &session.windows {
            names.insert(
                format!("{}:{}", session.name, window.index),
                window.name.clone(),
            );
        }
    }
    names
}

fn agents_row_from_pane(
    pane: &TmuxPane,
    window_names: &HashMap<String, String>,
    node: &str,
    all: bool,
) -> Option<AgentsRow> {
    let (session, win_part) = agents_parse_target(&pane.target)?;
    let window = agents_window_name(&session, &win_part, window_names);
    let is_oracle = window.ends_with(AGENTS_ORACLE_SUFFIX);
    if !all && !is_oracle {
        return None;
    }
    let oracle = if is_oracle {
        window
            .strip_suffix(AGENTS_ORACLE_SUFFIX)
            .unwrap_or_default()
            .to_owned()
    } else {
        String::new()
    };
    Some(AgentsRow {
        node: node.to_owned(),
        session,
        window,
        oracle,
        state: agents_state(&pane.command),
        pid: pane.pid,
    })
}

fn agents_parse_target(target: &str) -> Option<(String, String)> {
    let (session, rest) = target.rsplit_once(':')?;
    let (window, _pane) = rest.rsplit_once('.')?;
    if session.is_empty() || window.is_empty() {
        return None;
    }
    Some((session.to_owned(), window.to_owned()))
}

fn agents_window_name(
    session: &str,
    win_part: &str,
    window_names: &HashMap<String, String>,
) -> String {
    if win_part.bytes().all(|byte| byte.is_ascii_digit()) {
        window_names
            .get(&format!("{session}:{win_part}"))
            .cloned()
            .unwrap_or_default()
    } else {
        win_part.to_owned()
    }
}

fn agents_state(command: &str) -> String {
    if agents_is_shell_command(command) {
        "idle".to_owned()
    } else {
        "active".to_owned()
    }
}

fn agents_is_shell_command(command: &str) -> bool {
    matches!(
        command.to_ascii_lowercase().as_str(),
        "zsh" | "bash" | "sh" | "fish" | "dash"
    )
}

fn agents_build_node_rows(routes: &HashMap<String, String>, requested_node: &str, local_node: &str) -> Vec<AgentsRow> {
    let mut oracles = routes
        .iter()
        .filter(|(_, node)| agents_route_matches_node(node, requested_node, local_node))
        .map(|(oracle, _)| oracle.clone())
        .collect::<Vec<_>>();
    oracles.sort();
    oracles
        .into_iter()
        .map(|oracle| AgentsRow {
            node: requested_node.to_owned(),
            session: oracle.clone(),
            window: agents_oracle_window(&oracle),
            oracle: agents_oracle_name(&oracle),
            state: "idle".to_owned(),
            pid: None,
        })
        .collect()
}

fn agents_route_matches_node(route_node: &str, requested_node: &str, local_node: &str) -> bool {
    route_node == requested_node || (route_node == "local" && (requested_node == "local" || requested_node == local_node))
}

fn agents_oracle_window(oracle: &str) -> String {
    if oracle.ends_with(AGENTS_ORACLE_SUFFIX) { oracle.to_owned() } else { format!("{oracle}{AGENTS_ORACLE_SUFFIX}") }
}

fn agents_oracle_name(oracle: &str) -> String {
    oracle.strip_suffix(AGENTS_ORACLE_SUFFIX).unwrap_or(oracle).to_owned()
}

fn agents_render_json(rows: &[AgentsRow]) -> Result<String, String> {
    serde_json::to_string_pretty(rows)
        .map(|json| format!("{json}\n"))
        .map_err(|error| format!("agents: failed to render json: {error}"))
}

fn agents_render_table(rows: &[AgentsRow]) -> String {
    if rows.is_empty() {
        return "no oracle agents found\n".to_owned();
    }
    let mut out = String::new();
    let header = agents_table_header();
    let _ = writeln!(out, "{header}");
    let _ = writeln!(out, "{}", "-".repeat(header.len()));
    for row in rows {
        agents_write_table_row(&mut out, row);
    }
    out
}

fn agents_table_header() -> String {
    format!(
        "{}{}{}{}{}PID",
        agents_pad("NODE", 14),
        agents_pad("SESSION", 22),
        agents_pad("WINDOW", 22),
        agents_pad("ORACLE", 16),
        agents_pad("STATE", 8)
    )
}

fn agents_write_table_row(out: &mut String, row: &AgentsRow) {
    let pid = row
        .pid
        .map_or_else(|| "?".to_owned(), |pid| pid.to_string());
    let _ = writeln!(
        out,
        "{}{}{}{}{}{}",
        agents_pad(&row.node, 14),
        agents_pad(&row.session, 22),
        agents_pad(&row.window, 22),
        agents_pad(&row.oracle, 16),
        agents_state_cell(&row.state),
        pid
    );
}

fn agents_pad(value: &str, width: usize) -> String {
    format!("{value:<width$}")
}

fn agents_state_cell(state: &str) -> String {
    let color = if state == "active" {
        "\x1b[32m"
    } else {
        "\x1b[33m"
    };
    format!(
        "{color}{state}\x1b[0m{}",
        " ".repeat(8_usize.saturating_sub(state.len()))
    )
}

fn agents_load_node() -> Option<String> {
    let path = maw_config_path(&current_xdg_env(), &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    value
        .get("node")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod agents_tests {
    use super::*;

    struct AgentsFakeRuntime {
        node: String,
        routes: HashMap<String, String>,
        sessions: Vec<TmuxSession>,
        panes: Vec<TmuxPane>,
        touched_tmux: bool,
    }

    impl AgentsRuntime for AgentsFakeRuntime {
        fn agents_node(&self) -> String {
            self.node.clone()
        }

        fn agents_routes(&self) -> HashMap<String, String> {
            self.routes.clone()
        }

        fn agents_sessions(&mut self) -> Vec<TmuxSession> {
            self.touched_tmux = true;
            self.sessions.clone()
        }

        fn agents_panes(&mut self) -> Vec<TmuxPane> {
            self.touched_tmux = true;
            self.panes.clone()
        }
    }

    fn agents_fake_runtime() -> AgentsFakeRuntime {
        AgentsFakeRuntime {
            node: "test-node".to_owned(),
            routes: HashMap::from([
                ("nova".to_owned(), "edge".to_owned()),
                ("wish-oracle".to_owned(), "edge".to_owned()),
                ("localbot".to_owned(), "local".to_owned()),
            ]),
            sessions: vec![TmuxSession {
                name: "alpha".to_owned(),
                windows: vec![
                    maw_tmux::TmuxWindow {
                        index: 0,
                        name: "nova-oracle".to_owned(),
                        active: true,
                        cwd: None,
                    },
                    maw_tmux::TmuxWindow {
                        index: 1,
                        name: "notes".to_owned(),
                        active: false,
                        cwd: None,
                    },
                ],
            }],
            panes: vec![
                agents_pane("%1", "claude", "alpha:0.0", Some(1001)),
                agents_pane("%2", "bash", "alpha:notes.0", None),
            ],
            touched_tmux: false,
        }
    }

    fn agents_pane(id: &str, command: &str, target: &str, pid: Option<u32>) -> TmuxPane {
        TmuxPane {
            id: id.to_owned(),
            command: command.to_owned(),
            target: target.to_owned(),
            title: String::new(),
            pid,
            cwd: None,
            last_activity: None,
        }
    }

    fn agents_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn agents_dispatch_registers_both_core_routes() {
        assert_eq!(DISPATCH_70.len(), 2);
        assert_eq!(DISPATCH_70[0].command, "agents");
        assert_eq!(DISPATCH_70[1].command, "agent");
    }

    #[test]
    fn agents_table_filters_oracles_and_maps_numeric_windows() {
        let mut runtime = agents_fake_runtime();
        let out = agents_run(&Vec::new(), &mut runtime).expect("agents table");
        assert!(out.contains("NODE"), "{out}");
        assert!(out.contains("test-node"), "{out}");
        assert!(out.contains("nova-oracle"), "{out}");
        assert!(out.contains("nova"), "{out}");
        assert!(out.contains("active"), "{out}");
        assert!(!out.contains("notes"), "{out}");
        assert!(runtime.touched_tmux);
    }

    #[test]
    fn agents_all_json_includes_idle_non_oracle_rows() {
        let mut runtime = agents_fake_runtime();
        let out = agents_run(&agents_args(&["--all", "--json"]), &mut runtime).expect("json");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json parse");
        assert_eq!(value.as_array().expect("array").len(), 2);
        assert_eq!(value[0]["node"], "test-node");
        assert_eq!(value[0]["pid"], 1001);
        assert_eq!(value[1]["window"], "notes");
        assert_eq!(value[1]["oracle"], "");
        assert_eq!(value[1]["state"], "idle");
    }

    #[test]
    fn agents_node_and_help_do_not_touch_tmux() {
        let mut runtime = agents_fake_runtime();
        let node = agents_run(&agents_args(&["--node", "edge"]), &mut runtime).expect("node");
        assert_eq!(node, include_str!("../../tests/fixtures/native-agents/node-edge.stdout"));
        assert!(!runtime.touched_tmux);
        let help = agents_run(&agents_args(&["--help"]), &mut runtime).expect("help");
        assert_eq!(help, format!("{AGENTS_USAGE}\n"));
        assert!(!runtime.touched_tmux);
    }

    #[test]
    fn agents_node_json_is_metadata_only_and_ignores_missing_js_ref() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut runtime = agents_fake_runtime();
        let out = agents_run(&agents_args(&["--node=edge", "--json"]), &mut runtime).expect("json");
        let value: serde_json::Value = serde_json::from_str(&out).expect("json parse");
        assert_eq!(value.as_array().expect("array").len(), 2);
        assert_eq!(value[0]["node"], "edge");
        assert_eq!(value[0]["session"], "nova");
        assert_eq!(value[0]["window"], "nova-oracle");
        assert_eq!(value[0]["oracle"], "nova");
        assert_eq!(value[0]["state"], "idle");
        assert!(value[0]["pid"].is_null());
        assert!(!runtime.touched_tmux);
    }

    #[test]
    fn agents_rejects_leading_dash_and_unexpected_args() {
        let mut runtime = agents_fake_runtime();
        assert!(agents_run(&agents_args(&["--node", "-bad"]), &mut runtime).is_err());
        assert!(agents_run(&agents_args(&["--bogus"]), &mut runtime).is_err());
        assert!(agents_run(&agents_args(&["extra"]), &mut runtime).is_err());
    }

    #[test]
    fn agents_empty_table_matches_js_message() {
        let mut runtime = AgentsFakeRuntime {
            node: "local".to_owned(),
            routes: HashMap::new(),
            sessions: Vec::new(),
            panes: vec![agents_pane("%1", "bash", "alpha:notes.0", None)],
            touched_tmux: false,
        };
        let out = agents_run(&Vec::new(), &mut runtime).expect("empty");
        assert_eq!(out, "no oracle agents found\n");
    }
}
