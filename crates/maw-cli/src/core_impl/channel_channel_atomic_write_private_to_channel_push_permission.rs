fn channel_atomic_write_private(path: &std::path::Path, contents: &str, mode: u32) -> Result<(), (i32, String)> {
    let parent = path.parent().ok_or_else(|| (1, "channel: private path has no parent".to_owned()))?;
    std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel: create private dir failed: {error}")))?;
    let tmp_path = parent.join(channel_tmp_file_name(path));
    std::fs::write(&tmp_path, contents).map_err(|error| (1, format!("channel: write temp private failed: {error}")))?;
    channel_chmod(&tmp_path, mode)?;
    std::fs::rename(&tmp_path, path).map_err(|error| (1, format!("channel: rename temp private failed: {error}")))?;
    channel_chmod(path, mode)
}

#[cfg(unix)]
fn channel_chmod(path: &std::path::Path, mode: u32) -> Result<(), (i32, String)> {
    use std::os::unix::fs::PermissionsExt as _;

    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions).map_err(|error| (1, format!("channel: chmod failed: {error}")))
}

#[cfg(not(unix))]
fn channel_chmod(_path: &std::path::Path, _mode: u32) -> Result<(), (i32, String)> { Ok(()) }

fn channel_tilde_path(path: &std::path::Path) -> String {
    let home = channel_home();
    path.strip_prefix(&home).map_or_else(|_| path.display().to_string(), |rest| format!("~/{}", rest.display()))
}

fn channel_platform_is_macos() -> bool {
    std::env::var("MAW_RS_CHANNEL_FAKE_PLATFORM").map_or(cfg!(target_os = "macos"), |value| value == "darwin")
}

fn channel_ls(argv: &[String]) -> Result<String, (i32, String)> {
    let (target, json, verbose) = channel_parse_ls(argv)?;
    if json { return Ok(channel_ls_json(target.as_deref())); }
    if let Some(target) = target { return Ok(channel_ls_one(&target, verbose)); }
    Ok(channel_ls_all(verbose))
}

fn channel_parse_ls(argv: &[String]) -> Result<(Option<String>, bool, bool), (i32, String)> {
    let mut target = None;
    let mut json = false;
    let mut verbose = false;
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            "--verbose" | "-v" => verbose = true,
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("channel: unknown ls flag {value}"))),
            value => {
                if target.is_some() { return Err((2, "channel ls accepts at most one oracle".to_owned())); }
                target = Some(channel_validate_name("oracle", value)?);
            }
        }
    }
    Ok((target, json, verbose))
}

fn channel_providers(argv: &[String]) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    channel_reject_extra_args("providers", argv)?;
    let providers = channel_get_providers();
    let mut stdout = format!("  \x1b[36;1mChannel Providers\x1b[0m ({} available)\n\n", providers.len());
    stdout.push_str("  Provider        Type       Plugin ID                                     Status\n");
    stdout.push_str("  ─────────────── ────────── ───────────────────────────────────────────── ──────────\n");
    for provider in providers {
        let status = if channel_is_plugin_installed(&provider.short_name) { "\x1b[32m✓ installed\x1b[0m" } else { "\x1b[90mnot installed\x1b[0m" };
        let _ = writeln!(stdout, "  {:<15} {:<10} {:<45} {status}", provider.short_name, provider.kind, provider.plugin_id);
    }
    stdout.push_str("\n  Install: \x1b[36m/plugin install <provider>@claude-plugins-official\x1b[0m\n");
    stdout.push_str("  Custom:  \x1b[36mmaw channel add <oracle> server:<name>\x1b[0m (for .mcp.json servers)\n");
    Ok(stdout)
}

fn channel_test(argv: &[String]) -> Result<String, (i32, String)> {
    let target = channel_parse_test(argv)?;
    let Some(config) = channel_load_oracle_config(&target) else {
        return Err((1, format!("  \x1b[31m✗\x1b[0m no channels for {target}")));
    };
    if config.plugins.is_empty() { return Err((1, format!("  \x1b[31m✗\x1b[0m no channels for {target}"))); }
    let env = channel_effective_env(&config);
    let mut stdout = format!("  \x1b[36;1mChannel Test: {target}\x1b[0m\n\n");
    for plugin in &config.plugins {
        stdout.push_str("  ");
        stdout.push_str(&plugin.id);
        stdout.push('\n');
        for check in channel_checks(plugin, &config, &env) {
            stdout.push_str("    ");
            stdout.push_str(&check);
            stdout.push('\n');
        }
    }
    Ok(stdout)
}

fn channel_parse_test(argv: &[String]) -> Result<String, (i32, String)> {
    match argv {
        [] => Err((1, "  usage: maw channel test <oracle>".to_owned())),
        [target] => channel_validate_name("oracle", target),
        _ => Err((2, "channel test accepts exactly one oracle".to_owned())),
    }
}

fn channel_reject_extra_args(subcommand: &str, argv: &[String]) -> Result<(), (i32, String)> {
    if argv.iter().any(|arg| arg == "--") { return Err((2, "channel: -- separator is not supported".to_owned())); }
    if let Some(arg) = argv.first() { return Err((2, format!("channel {subcommand}: unexpected argument {arg}"))); }
    Ok(())
}

fn channel_validate_name(label: &str, value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value == "."
        || value == ".."
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err((2, format!("channel: invalid {label}")));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return Err((2, format!("channel: invalid {label}")));
    }
    Ok(value.to_owned())
}

fn channel_ls_json(target: Option<&str>) -> String {
    if let Some(target) = target {
        let config = channel_redacted_config(channel_load_oracle_config(target).unwrap_or_default());
        let mut value = serde_json::to_value(config).expect("channel config json");
        if let serde_json::Value::Object(map) = &mut value { map.insert("oracle".to_owned(), serde_json::json!(target)); }
        return format!("{}\n", serde_json::to_string_pretty(&value).expect("json"));
    }
    let oracles = channel_list_all_configs()
        .into_iter()
        .map(|(oracle, config)| serde_json::json!({ "oracle": oracle, "plugins": channel_redacted_config(config).plugins }))
        .collect::<Vec<_>>();
    format!("{}\n", serde_json::to_string_pretty(&serde_json::json!({ "oracles": oracles })).expect("json"))
}

fn channel_redacted_config(mut config: ChannelConfig) -> ChannelConfig {
    for plugin in &mut config.plugins {
        if let Some(env) = &mut plugin.env {
            for (key, value) in env.iter_mut() {
                if channel_is_secret_key(key) { "<redacted>".clone_into(value); }
            }
        }
    }
    if let Some(token_source) = &config.token_source {
        config.token_source = Some(channel_display_token_source(token_source));
    }
    config
}

fn channel_ls_one(target: &str, verbose: bool) -> String {
    let Some(config) = channel_load_oracle_config(target) else { return format!("  \x1b[90mno channels for {target}\x1b[0m\n"); };
    if config.plugins.is_empty() { return format!("  \x1b[90mno channels for {target}\x1b[0m\n"); }
    let mut stdout = format!("  \x1b[36;1m{target}\x1b[0m\n");
    for plugin in &config.plugins {
        stdout.push_str("    ");
        stdout.push_str(&plugin.id);
        stdout.push('\n');
        channel_push_plugin_env(&mut stdout, plugin, 6);
    }
    channel_push_token_source(&mut stdout, &config, 4);
    if verbose { channel_push_permission(&mut stdout, &config, 4); }
    stdout
}

fn channel_ls_all(verbose: bool) -> String {
    use std::fmt::Write as _;

    let all = channel_list_all_configs();
    if all.is_empty() {
        return "  \x1b[90mno oracles have channels configured\x1b[0m\n  add one: \x1b[36mmaw channel add <oracle> discord\x1b[0m\n".to_owned();
    }
    let mut stdout = format!("  \x1b[36;1mOracle{}Channel\x1b[0m\n", " ".repeat(24));
    let _ = writeln!(stdout, "  {}  {}", "─".repeat(30), "─".repeat(45));
    for (oracle, config) in &all {
        for plugin in &config.plugins {
            let _ = writeln!(stdout, "  {oracle:<30}  {}", plugin.id);
            if verbose {
                channel_push_plugin_env(&mut stdout, plugin, 32);
                channel_push_permission(&mut stdout, config, 32);
                channel_push_token_source(&mut stdout, config, 32);
            }
        }
    }
    let _ = writeln!(stdout, "\n  {} oracle(s) with channels", all.len());
    stdout
}

fn channel_push_plugin_env(stdout: &mut String, plugin: &ChannelPlugin, indent: usize) {
    use std::fmt::Write as _;

    if let Some(env) = &plugin.env {
        for (key, value) in env {
            let value = channel_display_env_value(key, value);
            let _ = writeln!(stdout, "{}\x1b[90m{key}={value}\x1b[0m", " ".repeat(indent));
        }
    }
}

fn channel_push_permission(stdout: &mut String, config: &ChannelConfig, indent: usize) {
    use std::fmt::Write as _;

    if let Some(mode) = &config.permission_mode {
        let _ = writeln!(stdout, "{}\x1b[90mpermissionMode: {mode}\x1b[0m", " ".repeat(indent));
    }
}

