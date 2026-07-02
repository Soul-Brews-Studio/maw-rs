const DISPATCH_95: &[DispatcherEntry] = &[DispatcherEntry {
    command: "serve-identity",
    handler: Handler::Sync(serveidentity_command),
}];

const SERVEIDENTITY_USAGE: &str = "usage: maw serve-identity";
#[allow(dead_code)]
const SERVEIDENTITY_DEFAULT_ORACLE: &str = "mawjs";
#[allow(dead_code)]
const SERVEIDENTITY_DEFAULT_HOST: &str = "local";
#[allow(dead_code)]
const SERVEIDENTITY_DEFAULT_PORT: u16 = 3456;
#[allow(dead_code)]
const SERVEIDENTITY_ENDPOINTS: &[&str] = &[
    "/api/agents",
    "/api/identity",
    "/api/messages",
    "/api/pane-keys",
    "/api/probe",
    "/api/send",
    "/api/sleep",
    "/api/wake",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(dead_code)]
struct ServeidentityConfig {
    node: Option<String>,
    oracle: Option<String>,
    port: Option<u16>,
    node_user: Option<String>,
    service_user: Option<String>,
    agents: HashMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct ServeidentityResolvedNode {
    node: String,
    host: String,
    user: Option<String>,
    port: Option<u16>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
struct ServeidentityDeps {
    version: String,
    uptime_seconds: u64,
    clock_utc: String,
    env_node_user: Option<String>,
    env_service_user: Option<String>,
    process_user: Option<String>,
}

fn serveidentity_command(argv: &[String]) -> CliOutput {
    match serveidentity_parse_args(argv) {
        Ok(()) => CliOutput {
            code: 0,
            stdout: "serve-identity registers GET /api/identity from the maw serve lifecycle hook\n".to_owned(),
            stderr: String::new(),
        },
        Err(message) => serveidentity_usage_error(&message),
    }
}

fn serveidentity_parse_args(argv: &[String]) -> Result<(), String> {
    let Some(arg) = argv.first() else { return Ok(()); };
    match arg.as_str() {
        "--help" | "-h" | "help" => Err(String::new()),
        "--" => Err("serve-identity: -- separator is not supported".to_owned()),
        value if value.starts_with('-') => Err(format!("serve-identity: unknown argument {value}")),
        value => Err(format!("serve-identity: unknown argument {value}")),
    }
}

fn serveidentity_usage_error(message: &str) -> CliOutput {
    let prefix = if message.is_empty() { String::new() } else { format!("{message}\n") };
    CliOutput { code: 2, stdout: String::new(), stderr: format!("{prefix}{SERVEIDENTITY_USAGE}\n") }
}


pub(crate) fn serveidentity_http_payload_read_only() -> serde_json::Value {
    let config = serveidentity_load_config();
    let deps = serveidentity_default_read_only_deps();
    serveidentity_identity_payload(&config, &deps)
}

fn serveidentity_default_read_only_deps() -> ServeidentityDeps {
    ServeidentityDeps {
        version: MAW_RS_BUILD_VERSION.to_owned(),
        uptime_seconds: current_epoch_seconds().saturating_sub(serveidentity_process_started_at()),
        clock_utc: serveidentity_now_utc(),
        env_node_user: std::env::var("MAW_NODE_USER").ok(),
        env_service_user: std::env::var("MAW_SERVICE_USER").ok(),
        process_user: std::env::var("USER").ok().or_else(|| std::env::var("LOGNAME").ok()),
    }
}

#[allow(dead_code)]
fn serveidentity_identity_payload(config: &ServeidentityConfig, deps: &ServeidentityDeps) -> serde_json::Value {
    let resolved = serveidentity_resolve_node(config, deps);
    let agents = serveidentity_hosted_agents(config, &resolved.node, &resolved.host);
    let mut payload = serde_json::json!({
        "node": resolved.node,
        "host": resolved.host,
        "oracle": config.oracle.as_deref().unwrap_or(SERVEIDENTITY_DEFAULT_ORACLE),
        "version": deps.version,
        "agents": agents,
        "uptime": deps.uptime_seconds,
        "clockUtc": deps.clock_utc,
        "endpoints": SERVEIDENTITY_ENDPOINTS,
    });
    serveidentity_insert_optional_fields(&mut payload, resolved.user.as_deref(), resolved.port);
    payload
}

#[allow(dead_code)]
fn serveidentity_insert_optional_fields(payload: &mut serde_json::Value, user: Option<&str>, port: Option<u16>) {
    if let Some(user) = user {
        payload["user"] = serde_json::Value::String(user.to_owned());
    }
    if let Some(port) = port {
        payload["port"] = serde_json::Value::Number(serde_json::Number::from(port));
    }
}

#[allow(dead_code)]
fn serveidentity_resolve_node(config: &ServeidentityConfig, deps: &ServeidentityDeps) -> ServeidentityResolvedNode {
    let host = serveidentity_clean(config.node.as_deref()).unwrap_or(SERVEIDENTITY_DEFAULT_HOST).to_owned();
    let explicit_user = serveidentity_first_clean(&[
        config.node_user.as_deref(),
        config.service_user.as_deref(),
        deps.env_node_user.as_deref(),
        deps.env_service_user.as_deref(),
    ]);
    let inferred_user = explicit_user.or_else(|| serveidentity_infer_process_user(config.port, deps.process_user.as_deref()));
    let node = canonical_node_identity(&host, inferred_user.as_deref());
    let user = inferred_user.filter(|user| node != host && !user.is_empty());
    ServeidentityResolvedNode { node, host, user, port: config.port }
}

#[allow(dead_code)]
fn serveidentity_infer_process_user(port: Option<u16>, process_user: Option<&str>) -> Option<String> {
    if port.is_some_and(|port| port != SERVEIDENTITY_DEFAULT_PORT) {
        return serveidentity_clean(process_user).map(ToOwned::to_owned);
    }
    None
}

#[allow(dead_code)]
fn serveidentity_first_clean(values: &[Option<&str>]) -> Option<String> {
    values.iter().find_map(|value| serveidentity_clean(*value).map(ToOwned::to_owned))
}

#[allow(dead_code)]
fn serveidentity_clean(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|trimmed| !trimmed.is_empty())
}

#[allow(dead_code)]
fn serveidentity_hosted_agents(config: &ServeidentityConfig, node: &str, host: &str) -> Vec<String> {
    let mut agents = hosted_agents(&config.agents, node);
    if host != node {
        agents.extend(hosted_agents(&config.agents, host));
    }
    serveidentity_unique_sorted(agents)
}

#[allow(dead_code)]
fn serveidentity_unique_sorted(values: Vec<String>) -> Vec<String> {
    values.into_iter().collect::<BTreeSet<_>>().into_iter().collect()
}

#[allow(dead_code)]
fn serveidentity_load_config() -> ServeidentityConfig {
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let Ok(raw) = std::fs::read_to_string(path) else { return ServeidentityConfig::default(); };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&raw) else { return ServeidentityConfig::default(); };
    ServeidentityConfig {
        node: serveidentity_json_string(&value, "node"),
        oracle: serveidentity_json_string(&value, "oracle"),
        port: serveidentity_json_port(&value),
        node_user: serveidentity_json_string(&value, "nodeUser"),
        service_user: serveidentity_json_string(&value, "serviceUser"),
        agents: serveidentity_json_agents(&value),
    }
}

#[allow(dead_code)]
fn serveidentity_process_started_at() -> u64 {
    static STARTED_AT: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *STARTED_AT.get_or_init(current_epoch_seconds)
}

#[allow(dead_code)]
fn serveidentity_now_utc() -> String {
    let seconds = current_epoch_seconds();
    let (year, month, day, hour, minute, second) = serveidentity_utc_from_unix(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z")
}

#[allow(dead_code)]
fn serveidentity_utc_from_unix(seconds: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let rem = seconds % 86_400;
    let (year, month, day) = serveidentity_date_from_days(days);
    let hour = u32::try_from(rem / 3_600).unwrap_or(0);
    let minute = u32::try_from((rem % 3_600) / 60).unwrap_or(0);
    let second = u32::try_from(rem % 60).unwrap_or(0);
    (year, month, day, hour, minute, second)
}

#[allow(dead_code)]
fn serveidentity_date_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (i32::try_from(year).unwrap_or(i32::MAX), u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

#[allow(dead_code)]
fn serveidentity_json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    value.get(key).and_then(serde_json::Value::as_str).and_then(|item| serveidentity_clean(Some(item))).map(ToOwned::to_owned)
}

#[allow(dead_code)]
fn serveidentity_json_port(value: &serde_json::Value) -> Option<u16> {
    value.get("port").and_then(serde_json::Value::as_u64).and_then(|port| u16::try_from(port).ok())
}

#[allow(dead_code)]
fn serveidentity_json_agents(value: &serde_json::Value) -> HashMap<String, String> {
    value
        .get("agents")
        .and_then(serde_json::Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|node| (key.clone(), node.to_owned())))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod serveidentity_tests {
    use super::*;

    fn serveidentity_deps() -> ServeidentityDeps {
        ServeidentityDeps {
            version: "1.2.3".to_owned(),
            uptime_seconds: 42,
            clock_utc: "2026-06-08T02:03:04.000Z".to_owned(),
            env_node_user: None,
            env_service_user: None,
            process_user: Some("agent".to_owned()),
        }
    }

    fn serveidentity_config() -> ServeidentityConfig {
        ServeidentityConfig {
            node: Some("white".to_owned()),
            oracle: Some("gm-bo".to_owned()),
            port: Some(4567),
            node_user: None,
            service_user: None,
            agents: HashMap::from([
                ("nova".to_owned(), "local".to_owned()),
                ("wish".to_owned(), "white".to_owned()),
                ("remote".to_owned(), "black".to_owned()),
            ]),
        }
    }

    fn serveidentity_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn serveidentity_command_reports_mounted_identity_route_without_stub_warning() {
        let output = serveidentity_command(&[]);
        assert_eq!(output.code, 0);
        assert!(output.stdout.contains("registers GET /api/identity"));
        assert!(output.stderr.is_empty());
    }

    #[test]
    fn serveidentity_parser_rejects_separator_and_flags_before_work() {
        assert!(serveidentity_parse_args(&serveidentity_strings(&["--"])).unwrap_err().contains("separator"));
        assert!(serveidentity_parse_args(&serveidentity_strings(&["--token"])).unwrap_err().contains("unknown argument"));
        assert!(serveidentity_parse_args(&serveidentity_strings(&["value"])).unwrap_err().contains("unknown argument"));
    }

    #[test]
    fn serveidentity_payload_matches_identity_route_shape_without_real_secret() {
        let payload = serveidentity_identity_payload(&serveidentity_config(), &serveidentity_deps());
        assert_eq!(payload["node"], "agent@white");
        assert_eq!(payload["host"], "white");
        assert_eq!(payload["user"], "agent");
        assert_eq!(payload["port"], 4567);
        assert_eq!(payload["oracle"], "gm-bo");
        assert_eq!(payload["version"], "1.2.3");
        assert_eq!(payload["uptime"], 42);
        assert_eq!(payload["clockUtc"], "2026-06-08T02:03:04.000Z");
        assert!(payload.get("pubkey").is_none(), "identity payload must not expose peer_key: {payload}");
        assert!(!payload.to_string().contains("pub"), "identity payload leaked peer_key: {payload}");
        assert_eq!(payload["endpoints"].as_array().expect("endpoints").len(), 8);
        assert_eq!(payload["agents"], serde_json::json!(["nova", "wish"]));
    }

    #[test]
    fn serveidentity_explicit_user_precedence_matches_js() {
        let mut config = serveidentity_config();
        let mut deps = serveidentity_deps();
        config.node_user = Some("node-user".to_owned());
        config.service_user = Some("service-user".to_owned());
        deps.env_node_user = Some("env-node".to_owned());
        let resolved = serveidentity_resolve_node(&config, &deps);
        assert_eq!(resolved.node, "node-user@white");
        assert_eq!(resolved.user.as_deref(), Some("node-user"));
    }

    #[test]
    fn serveidentity_default_port_does_not_infer_process_user() {
        let mut config = serveidentity_config();
        config.port = Some(SERVEIDENTITY_DEFAULT_PORT);
        let resolved = serveidentity_resolve_node(&config, &serveidentity_deps());
        assert_eq!(resolved.node, "white");
        assert_eq!(resolved.user, None);
    }

    #[test]
    fn serveidentity_http_provider_does_not_read_or_publish_peer_key() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore_home = EnvVarRestore::capture("HOME");
        let _restore_maw_home = EnvVarRestore::capture("MAW_HOME");
        let _restore_maw_state = EnvVarRestore::capture("MAW_STATE_DIR");
        let _restore_maw_config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let _restore_peer = EnvVarRestore::capture("MAW_PEER_KEY");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-serveidentity-{}",
            current_epoch_seconds()
        ));
        let state = root.join("state");
        let config = root.join("config");
        std::fs::create_dir_all(&state).expect("state");
        std::fs::create_dir_all(&config).expect("config");
        std::env::set_var("HOME", &root);
        std::env::set_var("MAW_STATE_DIR", &state);
        std::env::set_var("MAW_CONFIG_DIR", &config);
        std::env::remove_var("MAW_HOME");
        std::env::remove_var("MAW_PEER_KEY");

        let payload = serveidentity_http_payload_read_only();
        assert!(payload.get("pubkey").is_none(), "identity payload must not expose peer_key: {payload}");
        assert!(!state.join("peer-key").exists());

        std::fs::write(state.join("peer-key"), "pub-from-file\n").expect("peer-key");
        let payload = serveidentity_http_payload_read_only();
        assert!(payload.get("pubkey").is_none(), "identity payload must not expose peer_key: {payload}");
        assert!(!payload.to_string().contains("pub-from-file"), "identity payload leaked peer_key: {payload}");
    }

    #[test]
    fn serveidentity_unix_time_format_is_stable() {
        assert_eq!(serveidentity_utc_from_unix(1_780_884_184), (2026, 6, 8, 2, 3, 4));
    }
}
