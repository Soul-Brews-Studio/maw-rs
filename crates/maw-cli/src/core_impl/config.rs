const DISPATCH_136: &[DispatcherEntry] = &[DispatcherEntry {
    command: "config",
    handler: Handler::Sync(config_run_command),
}];

const CONFIG_USAGE: &str = "usage: maw config <show|sources|explain <key>|set <key> <value>> [--json]";

fn config_run_command(argv: &[String]) -> CliOutput {
    match config_dispatch(argv) {
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

fn config_dispatch(argv: &[String]) -> Result<String, String> {
    let sub = argv.first().map_or("show", String::as_str);
    let json = argv.iter().any(|arg| arg == "--json");
    let reveal = argv.iter().any(|arg| arg == "--reveal");
    match sub {
        "set" => config_set(argv, json),
        "show" => config_show(reveal),
        "sources" => config_sources(json),
        "explain" => config_explain(argv, json),
        _ => Err(CONFIG_USAGE.to_owned()),
    }
}

fn config_set(argv: &[String], json: bool) -> Result<String, String> {
    let key = argv
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| "usage: maw config set <key> <value>".to_owned())?;
    if key.starts_with('-') {
        return Err("usage: maw config set <key> <value>".to_owned());
    }
    let raw = argv
        .get(2)
        .map(String::as_str)
        .ok_or_else(|| "usage: maw config set <key> <value>".to_owned())?;
    let value = match key {
        "node" => serde_json::Value::String(config_validate_node(raw)?),
        "port" => serde_json::Value::Number(config_validate_port(raw)?.into()),
        _ => return Err("maw config: native set currently supports node|port".to_owned()),
    };
    let mut config = config_read_target()?;
    let object = config
        .as_object_mut()
        .ok_or_else(|| "maw config: root config must be a JSON object".to_owned())?;
    object.insert(key.to_owned(), value.clone());
    config_atomic_write(
        &config_target_path(),
        &format!(
            "{}\n",
            serde_json::to_string_pretty(&config)
                .map_err(|error| format!("maw config: failed to render JSON: {error}"))?
        ),
    )?;
    if json {
        Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&serde_json::json!({ "key": key, "value": value }))
                .map_err(|error| format!("maw config: failed to render JSON: {error}"))?
        ))
    } else {
        Ok(format!(
            "{key} = {}\n",
            serde_json::to_string(&value)
                .map_err(|error| format!("maw config: failed to render value: {error}"))?
        ))
    }
}

fn config_show(reveal: bool) -> Result<String, String> {
    let mut loaded = config_load_layers()?;
    if !reveal {
        config_redact_value(&mut loaded.config);
    }
    serde_json::to_string_pretty(&loaded.config)
        .map(|body| format!("{body}\n"))
        .map_err(|error| format!("maw config: failed to render JSON: {error}"))
}

fn config_sources(json: bool) -> Result<String, String> {
    let loaded = config_load_layers()?;
    if json {
        let rows: Vec<serde_json::Value> = loaded
            .sources
            .iter()
            .map(|source| {
                serde_json::json!({
                    "weight": source.weight,
                    "scope": source.scope,
                    "local": source.is_local,
                    "file": source.path.display().to_string(),
                })
            })
            .collect();
        return serde_json::to_string_pretty(&serde_json::json!({ "sources": rows, "warnings": loaded.warnings }))
            .map(|body| format!("{body}\n"))
            .map_err(|error| format!("maw config: failed to render JSON: {error}"));
    }
    let mut out = String::new();
    for source in loaded.sources {
        let local = if source.is_local { "local" } else { "     " };
        let _ = writeln!(
            out,
            "{:>3} {:<7} {} {}",
            source.weight,
            source.scope,
            local,
            source.path.display()
        );
    }
    for warning in loaded.warnings {
        let _ = writeln!(out, "{warning}");
    }
    Ok(out)
}

fn config_explain(argv: &[String], json: bool) -> Result<String, String> {
    let key = argv
        .iter()
        .enumerate()
        .find_map(|(index, arg)| (index > 0 && !arg.starts_with('-')).then_some(arg.as_str()))
        .ok_or_else(|| "usage: maw config explain <key> [--json]".to_owned())?;
    let loaded = config_load_layers()?;
    let mut entries = config_provenance_at_path(&loaded.provenance, key);
    let mut final_value = config_value_at_path(&loaded.config, key).cloned().unwrap_or(serde_json::Value::Null);
    if config_is_secret_path(key) {
        final_value = config_mask_secret(&final_value);
        for entry in &mut entries {
            entry.value = config_mask_secret(&entry.value);
        }
    }
    if json {
        let rows: Vec<serde_json::Value> = entries
            .iter()
            .map(|entry| {
                serde_json::json!({
                    "path": entry.path,
                    "weight": entry.weight,
                    "scope": entry.scope,
                    "isLocal": entry.is_local,
                    "action": entry.action,
                    "value": entry.value,
                })
            })
            .collect();
        return serde_json::to_string_pretty(&serde_json::json!({ "key": key, "finalValue": final_value, "entries": rows }))
            .map(|body| format!("{body}\n"))
            .map_err(|error| format!("maw config: failed to render JSON: {error}"));
    }
    let mut out = String::new();
    let _ = writeln!(out, "key: {key}");
    for entry in &entries {
        let local = if entry.is_local { ".local" } else { "" };
        let value = serde_json::to_string(&entry.value)
            .map_err(|error| format!("maw config: failed to render value: {error}"))?;
        let _ = writeln!(
            out,
            "{} {}{} {} {}",
            entry.weight, entry.scope, local, entry.action, entry.path
        );
        let _ = writeln!(out, "  {value}");
    }
    let final_json = serde_json::to_string(&final_value)
        .map_err(|error| format!("maw config: failed to render value: {error}"))?;
    let _ = writeln!(out, "FINAL {final_json}");
    Ok(out)
}


#[derive(Clone, Debug)]
struct ConfigLayerSource {
    path: std::path::PathBuf,
    weight: u32,
    is_local: bool,
    scope: &'static str,
    scope_rank: u32,
}

#[derive(Clone, Debug)]
struct ConfigProvenanceEntry {
    path: String,
    weight: u32,
    scope: &'static str,
    is_local: bool,
    value: serde_json::Value,
    action: &'static str,
}

struct ConfigLoadedLayers {
    config: serde_json::Value,
    sources: Vec<ConfigLayerSource>,
    provenance: BTreeMap<String, Vec<ConfigProvenanceEntry>>,
    warnings: Vec<String>,
}

fn config_load_layers() -> Result<ConfigLoadedLayers, String> {
    let mut sources = config_discover_sources();
    if sources.is_empty() {
        sources.push(ConfigLayerSource {
            path: maw_config_path(&current_xdg_env(), &["maw.config.json"]),
            weight: 50,
            is_local: false,
            scope: "legacy",
            scope_rank: 20,
        });
    }
    let mut merged = serde_json::json!({});
    let mut provenance: BTreeMap<String, Vec<ConfigProvenanceEntry>> = BTreeMap::new();
    let mut loaded_any = false;
    for source in &sources {
        if !source.path.exists() {
            continue;
        }
        let raw = std::fs::read_to_string(&source.path)
            .map_err(|error| format!("maw config: failed to read config: {error}"))?;
        let layer = serde_json::from_str::<serde_json::Value>(&raw)
            .map_err(|error| format!("maw config: failed to parse config JSON: {error}"))?;
        if !layer.is_object() {
            continue;
        }
        loaded_any = true;
        config_record_provenance(&mut provenance, source, &layer, "");
        config_deep_merge(&mut merged, layer);
    }
    if !loaded_any {
        merged = serde_json::json!({});
    }
    Ok(ConfigLoadedLayers { config: merged, sources, provenance, warnings: Vec::new() })
}

fn config_discover_sources() -> Vec<ConfigLayerSource> {
    let env = current_xdg_env();
    let mut found = Vec::new();
    let config_dir = maw_config_dir(&env);
    let user_weighted = config_scan_dir(&config_dir, "user", 20);
    if user_weighted.is_empty() {
        let legacy = maw_config_path(&env, &["maw.config.json"]);
        if legacy.exists() {
            found.push(ConfigLayerSource { path: legacy, weight: 50, is_local: false, scope: "legacy", scope_rank: 20 });
        }
    } else {
        found.extend(user_weighted);
    }

    let mut chain = Vec::new();
    let mut dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    for _ in 0..32 {
        chain.push(dir.clone());
        let Some(parent) = dir.parent() else { break; };
        if parent == dir { break; }
        dir = parent.to_path_buf();
    }
    chain.reverse();
    for (index, dir) in (0u32..).zip(chain) {
        found.extend(config_scan_dir(&dir.join(".maw"), "project", 30 + index));
    }
    found.sort_by(|a, b| {
        a.weight
            .cmp(&b.weight)
            .then(a.scope_rank.cmp(&b.scope_rank))
            .then(a.is_local.cmp(&b.is_local))
            .then(a.path.cmp(&b.path))
    });
    found
}

fn config_scan_dir(dir: &std::path::Path, scope: &'static str, scope_rank: u32) -> Vec<ConfigLayerSource> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new(); };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue; };
        let Some((weight, is_local)) = config_parse_layer_name(name) else { continue; };
        out.push(ConfigLayerSource { path: entry.path(), weight, is_local, scope, scope_rank });
    }
    out
}

fn config_parse_layer_name(name: &str) -> Option<(u32, bool)> {
    let rest = name.strip_prefix("maw.config.")?;
    let (digits, is_local) = rest
        .strip_suffix(".local.json")
        .map_or_else(|| rest.strip_suffix(".json").map(|value| (value, false)), |value| Some((value, true)))?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, is_local))
}

fn config_deep_merge(target: &mut serde_json::Value, layer: serde_json::Value) {
    let (Some(target_map), Some(layer_map)) = (target.as_object_mut(), layer.as_object()) else {
        *target = layer;
        return;
    };
    for (key, value) in layer_map {
        if value.is_null() {
            target_map.remove(key);
        } else if value.is_object() && target_map.get(key).is_some_and(serde_json::Value::is_object) {
            if let Some(target_child) = target_map.get_mut(key) {
                config_deep_merge(target_child, value.clone());
            }
        } else if value.is_object() {
            let mut child = serde_json::json!({});
            config_deep_merge(&mut child, value.clone());
            target_map.insert(key.clone(), child);
        } else {
            target_map.insert(key.clone(), value.clone());
        }
    }
}

fn config_record_provenance(
    provenance: &mut BTreeMap<String, Vec<ConfigProvenanceEntry>>,
    source: &ConfigLayerSource,
    value: &serde_json::Value,
    parent: &str,
) {
    let Some(map) = value.as_object() else { return; };
    for (key, child) in map {
        let key_path = if parent.is_empty() { key.clone() } else { format!("{parent}.{key}") };
        if child.is_null() {
            provenance.entry(key_path).or_default().push(config_provenance_entry(source, child.clone(), "delete"));
        } else if child.is_object() {
            config_record_provenance(provenance, source, child, &key_path);
        } else {
            provenance.entry(key_path).or_default().push(config_provenance_entry(source, child.clone(), "set"));
        }
    }
}

fn config_provenance_entry(source: &ConfigLayerSource, value: serde_json::Value, action: &'static str) -> ConfigProvenanceEntry {
    ConfigProvenanceEntry {
        path: source.path.display().to_string(),
        weight: source.weight,
        scope: source.scope,
        is_local: source.is_local,
        value,
        action,
    }
}

fn config_provenance_at_path(provenance: &BTreeMap<String, Vec<ConfigProvenanceEntry>>, key_path: &str) -> Vec<ConfigProvenanceEntry> {
    if let Some(entries) = provenance.get(key_path) {
        return entries.clone();
    }
    let mut parts: Vec<&str> = key_path.split('.').collect();
    while parts.len() > 1 {
        parts.pop();
        if let Some(entries) = provenance.get(&parts.join(".")) {
            return entries.clone();
        }
    }
    Vec::new()
}

fn config_value_at_path<'a>(root: &'a serde_json::Value, key_path: &str) -> Option<&'a serde_json::Value> {
    let mut cursor = root;
    for part in key_path.split('.') {
        cursor = cursor.get(part)?;
    }
    Some(cursor)
}

fn config_is_secret_path(key_path: &str) -> bool {
    key_path.split('.').any(config_is_secret_key)
}

fn config_redact_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, child) in map.iter_mut() {
                if config_is_secret_key(key) {
                    *child = config_mask_secret(child);
                } else {
                    config_redact_value(child);
                }
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                config_redact_value(item);
            }
        }
        _ => {}
    }
}

fn config_mask_secret(value: &serde_json::Value) -> serde_json::Value {
    let Some(raw) = value.as_str() else {
        return serde_json::Value::String("****".to_owned());
    };
    if raw.chars().count() <= 4 {
        return serde_json::Value::String("****".to_owned());
    }
    let tail: String = raw
        .chars()
        .rev()
        .take(4)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    serde_json::Value::String(format!("****...{tail}"))
}

fn config_is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "node" | "port" | "host" | "url" | "oracleurl" | "bind" | "keyprefix"
    ) {
        return false;
    }
    if lower == "federationtoken"
        || lower == "pubkey"
        || lower == "peerpubkey"
        || lower == "peerkey"
    {
        return true;
    }
    lower.contains("token")
        || lower.contains("secret")
        || lower.contains("password")
        || lower.contains("key")
}

fn config_read_target() -> Result<serde_json::Value, String> {
    let path = config_target_path();
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|error| format!("maw config: failed to read config: {error}"))?;
    serde_json::from_str::<serde_json::Value>(&raw)
        .map_err(|error| format!("maw config: failed to parse config JSON: {error}"))
}

fn config_target_path() -> std::path::PathBuf {
    let env = current_xdg_env();
    let weighted = maw_config_path(&env, &["maw.config.50.json"]);
    if weighted.exists() {
        weighted
    } else {
        maw_config_path(&env, &["maw.config.json"])
    }
}

fn config_atomic_write(path: &std::path::Path, body: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("maw config: failed to create config dir: {error}"))?;
    }
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("maw.config.json");
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    std::fs::write(&tmp, body)
        .map_err(|error| format!("maw config: failed to write temp file: {error}"))?;
    std::fs::rename(&tmp, path)
        .map_err(|error| format!("maw config: failed to replace config: {error}"))
}

fn config_validate_node(raw: &str) -> Result<String, String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 64
        || value.starts_with('-')
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err("maw config: invalid node name".to_owned());
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_')
    {
        return Err("maw config: invalid node name".to_owned());
    }
    Ok(value.to_owned())
}

fn config_validate_port(raw: &str) -> Result<u16, String> {
    if raw.starts_with('-') || raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("maw config: invalid port (expected 1-65535)".to_owned());
    }
    let port = raw
        .parse::<u16>()
        .map_err(|_| "maw config: invalid port (expected 1-65535)".to_owned())?;
    if port == 0 {
        return Err("maw config: invalid port (expected 1-65535)".to_owned());
    }
    Ok(port)
}

#[cfg(test)]
mod config_tests {
    use super::{
        config_dispatch, config_target_path, dispatcher_status, DispatchKind, EnvVarRestore,
    };

    #[test]
    fn config_dispatch_registers_native() {
        assert_eq!(dispatcher_status("config"), DispatchKind::Native);
    }

    #[test]
    fn config_rejects_bad_node_and_port_before_write() {
        assert!(
            config_dispatch(&["set".to_owned(), "node".to_owned(), "--bad".to_owned()])
                .expect_err("bad node")
                .contains("invalid node")
        );
        assert!(
            config_dispatch(&["set".to_owned(), "port".to_owned(), "0".to_owned()])
                .expect_err("bad port")
                .contains("invalid port")
        );
    }

    #[test]
    fn config_unknown_subcommand_reports_native_usage() {
        let output = super::config_run_command(&["unknown".to_owned()]);
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("usage: maw config <show|sources|explain <key>|set <key> <value>> [--json]"));
    }

    #[test]
    fn config_set_uses_weighted_target_when_present() {
        let _lock = super::env_test_lock().lock().expect("env lock");
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = std::env::temp_dir().join(format!("maw-rs-config-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::fs::create_dir_all(root.join("config")).expect("config dir");
        std::fs::write(
            root.join("config/maw.config.50.json"),
            "{\"node\":\"old\"}\n",
        )
        .expect("seed");
        assert_eq!(config_target_path(), root.join("config/maw.config.50.json"));
        let stdout = config_dispatch(&["set".to_owned(), "node".to_owned(), "new-node".to_owned()])
            .expect("set node");
        assert_eq!(stdout, "node = \"new-node\"\n");
        let body =
            std::fs::read_to_string(root.join("config/maw.config.50.json")).expect("config body");
        assert!(body.contains("\"node\": \"new-node\""));
        assert!(!root.join("config/maw.config.50.json.tmp").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn config_show_redacts_federation_token_by_default() {
        let _lock = super::env_test_lock().lock().expect("env lock");
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = config_seed_secret_fixture("redact-default");
        let stdout = config_dispatch(&["show".to_owned()]).expect("show");
        assert!(stdout.contains("\"federationToken\": \"****...1234\""));
        assert!(!stdout.contains("super-secret-token-1234"));
        assert!(stdout.contains("\"node\": \"bigboy-vps\""));
        assert!(stdout.contains("\"port\": 3456"));
        assert!(stdout.contains("\"url\": \"https://peer.example/api\""));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn config_show_reveal_prints_raw_secret_when_explicit() {
        let _lock = super::env_test_lock().lock().expect("env lock");
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = config_seed_secret_fixture("reveal");
        let stdout =
            config_dispatch(&["show".to_owned(), "--reveal".to_owned()]).expect("show reveal");
        assert!(stdout.contains("super-secret-token-1234"));
        assert!(!stdout.contains("****...1234"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn config_show_json_remains_parseable_after_redaction() {
        let _lock = super::env_test_lock().lock().expect("env lock");
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = config_seed_secret_fixture("json");
        let stdout = config_dispatch(&["show".to_owned(), "--json".to_owned()]).expect("show json");
        let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("parseable json");
        assert_eq!(parsed["federationToken"].as_str(), Some("****...1234"));
        assert_eq!(parsed["node"].as_str(), Some("bigboy-vps"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn config_show_redacts_nested_secret_keys_but_keeps_key_prefix_visible() {
        let _lock = super::env_test_lock().lock().expect("env lock");
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = config_seed_secret_fixture("nested");
        let stdout = config_dispatch(&["show".to_owned()]).expect("show");
        assert!(stdout.contains("\"apiToken\": \"****...5678\""));
        assert!(stdout.contains("\"peerKey\": \"****...4321\""));
        assert!(stdout.contains("\"pubKey\": \"****...8765\""));
        assert!(stdout.contains("\"keyPrefix\": \"maw/fleet\""));
        assert!(!stdout.contains("plugin-token-5678"));
        assert!(!stdout.contains("peer-secret-4321"));
        assert!(!stdout.contains("pubkey-secret-8765"));
        let _ = std::fs::remove_dir_all(root);
    }

    fn config_seed_secret_fixture(label: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("maw-rs-config-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::env::remove_var("MAW_HOME");
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::fs::create_dir_all(root.join("config")).expect("config dir");
        std::fs::write(
            root.join("config/maw.config.50.json"),
            r#"{
  "node": "bigboy-vps",
  "port": 3456,
  "federationToken": "super-secret-token-1234",
  "namedPeers": [
    {
      "name": "peer",
      "url": "https://peer.example/api",
      "pubKey": "pubkey-secret-8765"
    }
  ],
  "plugins": {
    "foo": {
      "apiToken": "plugin-token-5678",
      "peerKey": "peer-secret-4321",
      "keyPrefix": "maw/fleet"
    }
  }
}
"#,
        )
        .expect("seed config");
        root
    }
}
