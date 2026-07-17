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

const AGENTS_USAGE: &str =
    "usage: maw agents [--json] [--all] [--node <node>] | maw agents gc [--dry-run|--apply]";
const AGENTS_ORACLE_SUFFIX: &str = "-oracle";
const AGENTS_GC_USAGE: &str = "usage: maw agents gc [--dry-run|--apply]\n\nlists manifest agent entries that match no disk repo, no live tmux session,\nand no fleet registry entry. checks are LOCAL-ONLY — entries routed to other\nnodes are always skipped (local checks cannot verify a remote agent), except\ninvalid names, which are junk on every node. default is a dry-run; --apply\nrewrites the config agents map without the phantom entries (entries defined\nin other config layers are reported but left untouched).";

/// Shared guard for agents-map names (#605): rejects argv junk (`--help`),
/// dot/backup names, empties, and shell-metacharacter/whitespace names before
/// they can become permanent manifest entries. Used by every agents-map
/// writer site (discover/route/fleet/federation-identity/federation-sync).
fn agent_name_is_valid(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('-')
        && !name.starts_with('.')
        && !name.chars().any(agent_name_char_is_refused)
}

fn agent_name_char_is_refused(ch: char) -> bool {
    ch.is_whitespace()
        || ch.is_control()
        || matches!(
            ch,
            ';' | '&'
                | '|'
                | '$'
                | '`'
                | '('
                | ')'
                | '<'
                | '>'
                | '"'
                | '\''
                | '\\'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '!'
                | '#'
                | '~'
        )
}

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
    if argv.first().map(String::as_str) == Some("gc") {
        return match agents_gc_run(&argv[1..], &mut AgentsGcSystemRuntime) {
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
        };
    }
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
    let value = merged_config_value();
    value
        .get("node")
        .and_then(serde_json::Value::as_str)
        .map(ToOwned::to_owned)
}

// --- agents gc (#605): drop phantom manifest entries -------------------------

#[derive(Debug)]
struct AgentsGcPhantom {
    name: String,
    node: String,
    reason: &'static str,
}

trait AgentsGcRuntime {
    fn gc_agents(&self) -> BTreeMap<String, String>;
    fn gc_local_node(&self) -> String;
    fn gc_live_names(&mut self) -> BTreeSet<String>;
    fn gc_registry_names(&self) -> BTreeSet<String>;
    fn gc_repo_exists(&self, name: &str) -> bool;
    fn gc_config_read(&self) -> Result<serde_json::Value, String>;
    fn gc_config_write(&mut self, value: &serde_json::Value) -> Result<(), String>;
    fn gc_config_path(&self) -> String;
}

struct AgentsGcSystemRuntime;

impl AgentsGcRuntime for AgentsGcSystemRuntime {
    fn gc_agents(&self) -> BTreeMap<String, String> {
        load_hey_config().route.agents.into_iter().collect()
    }

    fn gc_local_node(&self) -> String {
        agents_load_node().unwrap_or_else(|| "local".to_owned())
    }

    fn gc_live_names(&mut self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for session in TmuxClient::local().list_all() {
            names.insert(session.name.clone());
            for window in &session.windows {
                names.insert(window.name.clone());
            }
        }
        names
    }

    fn gc_registry_names(&self) -> BTreeSet<String> {
        let mut names = BTreeSet::new();
        for entry in fleet_load_entries().into_iter().filter(fleet_entry_is_session) {
            names.insert(entry.session.name.clone());
            for window in &entry.session.windows {
                names.insert(window.name.clone());
                if let Some(oracle) = native_fleet_window_oracle_name(window) {
                    names.insert(oracle);
                }
            }
        }
        names
    }

    fn gc_repo_exists(&self, name: &str) -> bool {
        locate_enrichment_names(name)
            .iter()
            .any(|alias| locate_find_oracle_repo_path(alias).is_some())
    }

    fn gc_config_read(&self) -> Result<serde_json::Value, String> {
        config_read_target()
    }

    fn gc_config_write(&mut self, value: &serde_json::Value) -> Result<(), String> {
        let path = config_target_path();
        let before = config_read_target()?;
        let body = serde_json::to_string_pretty(value)
            .map_err(|error| format!("agents gc: failed to render config JSON: {error}"))?;
        config_atomic_write(&path, &format!("{body}\n"))?;
        config_audit_write(&path, &before, value);
        Ok(())
    }

    fn gc_config_path(&self) -> String {
        config_target_path().display().to_string()
    }
}

fn agents_gc_run(argv: &[String], runtime: &mut impl AgentsGcRuntime) -> Result<String, String> {
    let mut apply = false;
    for arg in argv {
        match arg.as_str() {
            "--apply" => apply = true,
            "--dry-run" => apply = false,
            "--help" | "-h" => return Ok(format!("{AGENTS_GC_USAGE}\n")),
            other => return Err(format!("agents gc: unknown argument {other}\n{AGENTS_GC_USAGE}")),
        }
    }
    let scan = agents_gc_find_phantoms(runtime);
    if apply {
        agents_gc_apply(runtime, &scan)
    } else {
        Ok(agents_gc_render_dry_run(&scan))
    }
}

struct AgentsGcScan {
    total: usize,
    phantoms: Vec<AgentsGcPhantom>,
    remote_skipped: usize,
}

fn agents_gc_find_phantoms(runtime: &mut impl AgentsGcRuntime) -> AgentsGcScan {
    let agents = runtime.gc_agents();
    let local_node = runtime.gc_local_node();
    let live = agents_gc_canonical_set(&runtime.gc_live_names());
    let registry = agents_gc_canonical_set(&runtime.gc_registry_names());
    let mut phantoms = Vec::new();
    let mut remote_skipped = 0_usize;
    for (name, node) in &agents {
        // Invalid names (argv junk, shell metacharacters) are junk on every
        // node — they can never be routed to, so gc them regardless of node.
        if !agent_name_is_valid(name) {
            phantoms.push(AgentsGcPhantom {
                name: name.clone(),
                node: node.clone(),
                reason: "invalid name",
            });
            continue;
        }
        // Entries routed to another node are the cross-node routing table:
        // local checks (tmux, fleet registry, disk repo) cannot see a live
        // remote agent, so never classify them as phantoms.
        if !agents_gc_node_is_local(node, &local_node) {
            remote_skipped += 1;
            continue;
        }
        let Some(reason) = agents_gc_phantom_reason(name, &live, &registry, runtime) else {
            continue;
        };
        phantoms.push(AgentsGcPhantom {
            name: name.clone(),
            node: node.clone(),
            reason,
        });
    }
    AgentsGcScan {
        total: agents.len(),
        phantoms,
        remote_skipped,
    }
}

fn agents_gc_node_is_local(route_node: &str, local_node: &str) -> bool {
    route_node == local_node || route_node == "local"
}

fn agents_gc_phantom_reason(
    name: &str,
    live: &BTreeSet<String>,
    registry: &BTreeSet<String>,
    runtime: &impl AgentsGcRuntime,
) -> Option<&'static str> {
    let canonical = agents_gc_canonical(name);
    if live.contains(&canonical) || registry.contains(&canonical) {
        return None;
    }
    if runtime.gc_repo_exists(name) {
        return None;
    }
    Some("no repo, no session, no registry entry")
}

/// Canonicalizes an agent/session/window name for phantom matching: trims,
/// strips the fleet's all-digit `NN-` slot prefix (sessions are named e.g.
/// `33-maw-rs`; same rule as `native_fleet_window_oracle_name`), and strips
/// the `-oracle` suffix.
fn agents_gc_canonical(name: &str) -> String {
    let trimmed = name.trim();
    let without_slot = trimmed
        .split_once('-')
        .filter(|(prefix, suffix)| {
            !prefix.is_empty() && !suffix.is_empty() && prefix.chars().all(|ch| ch.is_ascii_digit())
        })
        .map_or(trimmed, |(_, suffix)| suffix);
    without_slot
        .strip_suffix(AGENTS_ORACLE_SUFFIX)
        .unwrap_or(without_slot)
        .to_owned()
}

fn agents_gc_canonical_set(names: &BTreeSet<String>) -> BTreeSet<String> {
    names.iter().map(|name| agents_gc_canonical(name)).collect()
}

fn agents_gc_render_dry_run(scan: &AgentsGcScan) -> String {
    let total = scan.total;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "agents gc (dry-run; local-only checks: disk repo, tmux session, fleet registry)"
    );
    if scan.remote_skipped > 0 {
        let _ = writeln!(
            out,
            "skipped {} remote-node entries (local-only checks cannot verify them)",
            scan.remote_skipped
        );
    }
    if scan.phantoms.is_empty() {
        let _ = writeln!(out, "no phantom entries ({total} checked)");
        return out;
    }
    let _ = writeln!(out, "phantom entries: {} of {total}", scan.phantoms.len());
    for phantom in &scan.phantoms {
        let _ = writeln!(
            out,
            "  {} -> {} ({})",
            phantom.name, phantom.node, phantom.reason
        );
    }
    let _ = writeln!(out, "run with --apply to remove");
    out
}

fn agents_gc_apply(
    runtime: &mut impl AgentsGcRuntime,
    scan: &AgentsGcScan,
) -> Result<String, String> {
    let phantoms = &scan.phantoms;
    if phantoms.is_empty() {
        return Ok(format!(
            "agents gc: no phantom entries ({} checked, {} remote-node skipped); nothing to remove\n",
            scan.total, scan.remote_skipped
        ));
    }
    let mut config = runtime.gc_config_read()?;
    let mut removed = Vec::new();
    let mut skipped = Vec::new();
    if let Some(map) = config
        .get_mut("agents")
        .and_then(serde_json::Value::as_object_mut)
    {
        for phantom in phantoms {
            if map.remove(&phantom.name).is_some() {
                removed.push(phantom.name.clone());
            } else {
                skipped.push(phantom.name.clone());
            }
        }
    } else {
        skipped.extend(phantoms.iter().map(|phantom| phantom.name.clone()));
    }
    if removed.is_empty() {
        return Ok(format!(
            "agents gc: 0 of {} phantom entries are defined in {} (other config layers); nothing rewritten\n",
            phantoms.len(),
            runtime.gc_config_path()
        ));
    }
    runtime.gc_config_write(&config)?;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "agents gc: removed {} phantom entries from {}",
        removed.len(),
        runtime.gc_config_path()
    );
    for name in &removed {
        let _ = writeln!(out, "  {name}");
    }
    if scan.remote_skipped > 0 {
        let _ = writeln!(
            out,
            "skipped {} remote-node entries (local-only checks cannot verify them)",
            scan.remote_skipped
        );
    }
    if !skipped.is_empty() {
        let _ = writeln!(
            out,
            "skipped {} entries defined in other config layers: {}",
            skipped.len(),
            skipped.join(", ")
        );
    }
    Ok(out)
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
        let _guard = env_test_lock();
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
    fn agent_name_validation_table() {
        for name in [
            "maw-rs",
            "oracle-hall",
            "digger",
            "nova_2",
            "laris-co/unconference-oracle",
        ] {
            assert!(agent_name_is_valid(name), "{name:?} should be accepted");
        }
        for name in [
            "--help",
            "-h",
            ".bak202605130508discord-oracle",
            ".bak-x",
            "",
            " ",
            "a b",
            "x;y",
            "x|y",
            "$(x)",
            "x\ty",
            " padded",
        ] {
            assert!(!agent_name_is_valid(name), "{name:?} should be rejected");
        }
    }

    struct AgentsGcFakeRuntime {
        agents: BTreeMap<String, String>,
        local_node: String,
        live: BTreeSet<String>,
        registry: BTreeSet<String>,
        repos: BTreeSet<String>,
        config: serde_json::Value,
        wrote: bool,
    }

    impl AgentsGcRuntime for AgentsGcFakeRuntime {
        fn gc_agents(&self) -> BTreeMap<String, String> {
            self.agents.clone()
        }

        fn gc_local_node(&self) -> String {
            self.local_node.clone()
        }

        fn gc_live_names(&mut self) -> BTreeSet<String> {
            self.live.clone()
        }

        fn gc_registry_names(&self) -> BTreeSet<String> {
            self.registry.clone()
        }

        fn gc_repo_exists(&self, name: &str) -> bool {
            self.repos.contains(name)
        }

        fn gc_config_read(&self) -> Result<serde_json::Value, String> {
            Ok(self.config.clone())
        }

        fn gc_config_write(&mut self, value: &serde_json::Value) -> Result<(), String> {
            self.config = value.clone();
            self.wrote = true;
            Ok(())
        }

        fn gc_config_path(&self) -> String {
            "/fixture/maw.config.json".to_owned()
        }
    }

    fn agents_gc_fake_runtime() -> AgentsGcFakeRuntime {
        AgentsGcFakeRuntime {
            agents: BTreeMap::from([
                ("--help".to_owned(), "m5".to_owned()),
                ("digger".to_owned(), "m5".to_owned()),
                ("nova-oracle".to_owned(), "local".to_owned()),
                ("squad-x".to_owned(), "m5".to_owned()),
                ("unconference-oracle".to_owned(), "m5".to_owned()),
            ]),
            local_node: "m5".to_owned(),
            live: BTreeSet::from(["nova".to_owned()]),
            registry: BTreeSet::from(["squad-x".to_owned()]),
            repos: BTreeSet::from(["digger".to_owned()]),
            config: serde_json::json!({
                "node": "m5",
                "agents": {"--help": "m5", "digger": "m5", "unconference-oracle": "m5"}
            }),
            wrote: false,
        }
    }

    #[test]
    fn agents_gc_dry_run_lists_phantoms_and_touches_nothing() {
        let mut runtime = agents_gc_fake_runtime();
        let before = runtime.config.clone();
        let out = agents_gc_run(&Vec::new(), &mut runtime).expect("gc dry-run");
        assert!(out.contains("local-only"), "{out}");
        assert!(out.contains("phantom entries: 2 of 5"), "{out}");
        assert!(out.contains("--help -> m5 (invalid name)"), "{out}");
        assert!(
            out.contains("unconference-oracle -> m5 (no repo, no session, no registry entry)"),
            "{out}"
        );
        assert!(out.contains("run with --apply to remove"), "{out}");
        assert!(!out.contains("digger ->"), "{out}");
        assert!(!out.contains("nova-oracle ->"), "{out}");
        assert!(!out.contains("squad-x ->"), "{out}");
        assert!(!runtime.wrote);
        assert_eq!(runtime.config, before);
    }

    #[test]
    fn agents_gc_apply_removes_exactly_the_phantoms_from_config() {
        let mut runtime = agents_gc_fake_runtime();
        let out = agents_gc_run(&agents_args(&["--apply"]), &mut runtime).expect("gc apply");
        assert!(runtime.wrote);
        assert!(out.contains("removed 2 phantom entries"), "{out}");
        assert!(out.contains("--help"), "{out}");
        assert!(out.contains("unconference-oracle"), "{out}");
        let agents = runtime
            .config
            .get("agents")
            .and_then(serde_json::Value::as_object)
            .expect("agents object");
        assert_eq!(agents.len(), 1);
        assert!(agents.contains_key("digger"));
        assert_eq!(
            runtime.config.get("node").and_then(serde_json::Value::as_str),
            Some("m5")
        );
    }

    #[test]
    fn agents_gc_apply_skips_phantoms_from_other_config_layers() {
        let mut runtime = agents_gc_fake_runtime();
        runtime.config = serde_json::json!({"agents": {"digger": "m5"}});
        let out = agents_gc_run(&agents_args(&["--apply"]), &mut runtime).expect("gc apply");
        assert!(!runtime.wrote);
        assert!(out.contains("nothing rewritten"), "{out}");
    }

    #[test]
    fn agents_gc_remote_node_entries_survive_apply() {
        let mut runtime = AgentsGcFakeRuntime {
            agents: BTreeMap::from([
                ("phaith-oracle".to_owned(), "phaith".to_owned()),
                ("--junk".to_owned(), "phaith".to_owned()),
                ("ghost".to_owned(), "m5".to_owned()),
            ]),
            local_node: "m5".to_owned(),
            live: BTreeSet::new(),
            registry: BTreeSet::new(),
            repos: BTreeSet::new(),
            config: serde_json::json!({
                "node": "m5",
                "agents": {"phaith-oracle": "phaith", "--junk": "phaith", "ghost": "m5"}
            }),
            wrote: false,
        };
        let dry = agents_gc_run(&Vec::new(), &mut runtime).expect("gc dry-run");
        assert!(
            dry.contains("skipped 1 remote-node entries (local-only checks cannot verify them)"),
            "{dry}"
        );
        assert!(dry.contains("phantom entries: 2 of 3"), "{dry}");
        assert!(dry.contains("--junk -> phaith (invalid name)"), "{dry}");
        assert!(dry.contains("ghost -> m5"), "{dry}");
        assert!(!dry.contains("phaith-oracle ->"), "{dry}");
        let out = agents_gc_run(&agents_args(&["--apply"]), &mut runtime).expect("gc apply");
        assert!(runtime.wrote);
        assert!(out.contains("removed 2 phantom entries"), "{out}");
        assert!(out.contains("skipped 1 remote-node entries"), "{out}");
        let agents = runtime
            .config
            .get("agents")
            .and_then(serde_json::Value::as_object)
            .expect("agents object");
        assert_eq!(agents.len(), 1);
        assert!(
            agents.contains_key("phaith-oracle"),
            "live remote route must survive --apply: {agents:?}"
        );
    }

    #[test]
    fn agents_gc_slot_prefixed_sessions_protect_manifest_entries() {
        let mut runtime = AgentsGcFakeRuntime {
            agents: BTreeMap::from([
                ("maw-rs".to_owned(), "m5".to_owned()),
                ("maw-p2p".to_owned(), "m5".to_owned()),
            ]),
            local_node: "m5".to_owned(),
            live: BTreeSet::from(["33-maw-rs".to_owned()]),
            registry: BTreeSet::from(["19-maw-p2p".to_owned()]),
            repos: BTreeSet::new(),
            config: serde_json::json!({
                "node": "m5",
                "agents": {"maw-rs": "m5", "maw-p2p": "m5"}
            }),
            wrote: false,
        };
        let before = runtime.config.clone();
        let dry = agents_gc_run(&Vec::new(), &mut runtime).expect("gc dry-run");
        assert!(dry.contains("no phantom entries (2 checked)"), "{dry}");
        let out = agents_gc_run(&agents_args(&["--apply"]), &mut runtime).expect("gc apply");
        assert!(!runtime.wrote);
        assert!(out.contains("nothing to remove"), "{out}");
        assert_eq!(runtime.config, before);
    }

    #[test]
    fn agents_gc_canonical_strips_slot_prefix_and_oracle_suffix() {
        assert_eq!(agents_gc_canonical("33-maw-rs"), "maw-rs");
        assert_eq!(agents_gc_canonical("19-maw-p2p"), "maw-p2p");
        assert_eq!(agents_gc_canonical("12-nova-oracle"), "nova");
        assert_eq!(agents_gc_canonical("nova-oracle"), "nova");
        assert_eq!(agents_gc_canonical("3e-infra"), "3e-infra");
        assert_eq!(agents_gc_canonical(" maw-rs "), "maw-rs");
    }

    #[test]
    fn agents_gc_help_and_unknown_args() {
        let mut runtime = agents_gc_fake_runtime();
        let help = agents_gc_run(&agents_args(&["--help"]), &mut runtime).expect("help");
        assert!(help.contains("LOCAL-ONLY"), "{help}");
        assert!(help.contains("--apply"), "{help}");
        assert!(!runtime.wrote);
        assert!(agents_gc_run(&agents_args(&["--bogus"]), &mut runtime).is_err());
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
