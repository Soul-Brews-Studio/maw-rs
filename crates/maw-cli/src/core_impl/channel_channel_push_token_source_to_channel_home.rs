fn channel_push_token_source(stdout: &mut String, config: &ChannelConfig, indent: usize) {
    use std::fmt::Write as _;

    if let Some(token_source) = &config.token_source {
        let token_source = channel_display_token_source(token_source);
        let _ = writeln!(stdout, "{}\x1b[90mtoken: {token_source}\x1b[0m", " ".repeat(indent));
    }
}

fn channel_display_env_value(key: &str, value: &str) -> String {
    if channel_is_secret_key(key) { "<redacted>".to_owned() } else { value.to_owned() }
}

fn channel_display_token_source(value: &str) -> String {
    if matches!(value.split_once(':'), Some(("pass" | "env" | "keychain", _))) { value.to_owned() } else { "<redacted>".to_owned() }
}

fn channel_is_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "PASS", "PRIVATE_KEY"].iter().any(|needle| upper.contains(needle))
}

fn channel_get_providers() -> Vec<ChannelProvider> {
    let mut providers = vec![
        channel_provider("discord", "plugin:discord@claude-plugins-official", "chat"),
        channel_provider("telegram", "plugin:telegram@claude-plugins-official", "chat"),
        channel_provider("imessage", "plugin:imessage@claude-plugins-official", "chat"),
        channel_provider("fakechat", "plugin:fakechat@claude-plugins-official", "chat"),
    ];
    providers.extend(channel_custom_providers());
    providers
}

fn channel_provider(short_name: &str, plugin_id: &str, kind: &'static str) -> ChannelProvider {
    ChannelProvider { short_name: short_name.to_owned(), plugin_id: plugin_id.to_owned(), kind }
}

fn channel_custom_providers() -> Vec<ChannelProvider> {
    let mut providers = Vec::new();
    for path in [std::env::current_dir().ok().map(|cwd| cwd.join(".mcp.json")), Some(channel_home().join(".claude.json"))].into_iter().flatten() {
        let Some(json) = channel_read_json(&path) else { continue; };
        let Some(servers) = json.get("mcpServers").and_then(serde_json::Value::as_object) else { continue; };
        for name in servers.keys() {
            if channel_validate_name("server", name).is_ok() { providers.push(channel_provider(name, &format!("server:{name}"), "custom")); }
        }
    }
    providers
}

fn channel_is_plugin_installed(short_name: &str) -> bool {
    channel_home().join(".claude/plugins/cache/claude-plugins-official").join(short_name).exists()
}

fn channel_checks(plugin: &ChannelPlugin, config: &ChannelConfig, env: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    let mut checks = Vec::new();
    if plugin.id.starts_with("plugin:") {
        let name = plugin.id.split(':').nth(1).and_then(|value| value.split('@').next()).unwrap_or_default();
        if channel_is_plugin_installed(name) { checks.push("\x1b[32m✓ plugin installed\x1b[0m".to_owned()); } else { checks.push("\x1b[31m✗ plugin not installed\x1b[0m".to_owned()); }
    }
    if let Some(dir) = env.get("DISCORD_STATE_DIR").or_else(|| plugin.env.as_ref().and_then(|map| map.get("DISCORD_STATE_DIR"))) {
        if std::path::Path::new(dir).exists() { checks.push("\x1b[32m✓ state dir exists\x1b[0m".to_owned()); } else { checks.push(format!("\x1b[31m✗ state dir missing: {dir}\x1b[0m")); }
    }
    if env.contains_key("DISCORD_BOT_TOKEN") || env.contains_key("TELEGRAM_BOT_TOKEN") { checks.push("\x1b[32m✓ token available\x1b[0m".to_owned()); } else if let Some(token_source) = &config.token_source { checks.push(format!("\x1b[32m✓ token source: {token_source}\x1b[0m")); } else { checks.push("\x1b[33m⚠ no token configured\x1b[0m".to_owned()); }
    checks
}

fn channel_effective_env(config: &ChannelConfig) -> std::collections::BTreeMap<String, String> {
    let mut env = std::collections::BTreeMap::new();
    for plugin in &config.plugins {
        if let Some(plugin_env) = &plugin.env { env.extend(plugin_env.clone()); }
    }
    let home = channel_home();
    for value in env.values_mut() {
        if let Some(stripped) = value.strip_prefix("~/") { *value = home.join(stripped).to_string_lossy().into_owned(); }
    }
    env
}


fn channel_config_path_for_add(oracle: &str, repo_path: Option<&std::path::Path>) -> std::path::PathBuf {
    repo_path.map_or_else(|| channel_oracle_config_path(oracle), channel_repo_config_path)
}

fn channel_oracle_config_path(oracle: &str) -> std::path::PathBuf {
    channel_channels_base().join(oracle).join("config.json")
}

fn channel_repo_config_path(repo_path: &std::path::Path) -> std::path::PathBuf {
    repo_path.join(".claude").join("channel.json")
}

fn channel_load_config_at(path: &std::path::Path) -> Option<ChannelConfig> {
    channel_read_json(path).and_then(|value| serde_json::from_value(value).ok())
}

fn channel_save_config_at(path: &std::path::Path, config: &ChannelConfig) -> Result<(), (i32, String)> {
    let json = serde_json::to_string_pretty(config).map_err(|error| (1, format!("channel: serialize config failed: {error}")))?;
    channel_atomic_write(path, &(json + "\n"))
}

fn channel_save_config_private_at(path: &std::path::Path, config: &ChannelConfig) -> Result<(), (i32, String)> {
    let json = serde_json::to_string_pretty(config).map_err(|error| (1, format!("channel: serialize config failed: {error}")))?;
    channel_atomic_write_private(path, &(json + "\n"), 0o600)
}

fn channel_atomic_write(path: &std::path::Path, contents: &str) -> Result<(), (i32, String)> {
    let parent = path.parent().ok_or_else(|| (1, "channel: config path has no parent".to_owned()))?;
    std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel: create config dir failed: {error}")))?;
    let tmp_path = parent.join(channel_tmp_file_name(path));
    std::fs::write(&tmp_path, contents).map_err(|error| (1, format!("channel: write temp config failed: {error}")))?;
    std::fs::rename(&tmp_path, path).map_err(|error| (1, format!("channel: rename temp config failed: {error}")))
}

fn channel_tmp_file_name(path: &std::path::Path) -> String {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("config.json");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".{name}.tmp.{}.{}", std::process::id(), nanos)
}

fn channel_archive_existing_config(path: &std::path::Path) -> Result<(), (i32, String)> {
    let Ok(contents) = std::fs::read_to_string(path) else { return Ok(()); };
    let parent = path.parent().ok_or_else(|| (1, "channel: config path has no parent".to_owned()))?;
    let archive_dir = parent.join("archive");
    let archive_name = channel_archive_file_name(path);
    channel_atomic_write(&archive_dir.join(archive_name), &contents)
}

fn channel_archive_file_name(path: &std::path::Path) -> String {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("config.json");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{name}.{}.{}.bak", std::process::id(), nanos)
}

fn channel_save_repo_gitignore(repo_path: &std::path::Path) -> Result<(), (i32, String)> {
    let gitignore = repo_path.join(".gitignore");
    let entry = ".claude/.env";
    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == entry) { return Ok(()); }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') { next.push('\n'); }
    next.push_str("\n# Channel bot token — never commit\n.claude/.env\n");
    channel_atomic_write(&gitignore, &next)
}

fn channel_list_all_configs() -> Vec<(String, ChannelConfig)> {
    let base = channel_channels_base();
    let Ok(entries) = std::fs::read_dir(base) else { return Vec::new(); };
    let mut configs = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) { continue; }
        let oracle = entry.file_name().to_string_lossy().into_owned();
        if channel_validate_name("oracle", &oracle).is_err() { continue; }
        if let Some(config) = channel_load_oracle_config(&oracle) {
            if !config.plugins.is_empty() { configs.push((oracle, config)); }
        }
    }
    configs.sort_by(|left, right| left.0.cmp(&right.0));
    configs
}

fn channel_load_oracle_config(oracle: &str) -> Option<ChannelConfig> {
    let path = channel_channels_base().join(oracle).join("config.json");
    channel_read_json(&path).and_then(|value| serde_json::from_value(value).ok())
}

fn channel_read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn channel_channels_base() -> std::path::PathBuf { channel_home().join(".claude").join("channels") }

fn channel_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
}

