fn channel_setup_token(args: &ChannelSetupArgs, stdout: &mut String, total: usize) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let provider = args.provider.name();
    channel_push_setup_step(stdout, 2, total, "Bot token");
    if let Some(pass_key) = &args.pass_key {
        match channel_pass_show(pass_key) {
            Ok(token) if !token.is_empty() => {
                let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m token from pass: {pass_key}");
                if provider == "discord" {
                    if let Some(client_id) = channel_extract_client_id(&token) {
                        let _ = writeln!(stdout, "  \x1b[90mclient: {client_id}\x1b[0m");
                    }
                }
                return Ok(token);
            }
            _ => {
                let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m pass key '{pass_key}' not found");
                let _ = writeln!(stdout, "  \x1b[90mrun: pass insert {pass_key}\x1b[0m");
                return Err((0, stdout.clone()));
            }
        }
    }
    let env_file = channel_state_dir(&args.oracle).join(".env");
    if let Some(token) = channel_read_env_token(provider, &env_file) {
        stdout.push_str("  \x1b[32m✓\x1b[0m token found in .env\n");
        if provider == "discord" {
            if let Some(client_id) = channel_extract_client_id(&token) {
                let _ = writeln!(stdout, "  \x1b[90mclient: {client_id}\x1b[0m");
            }
        }
        return Ok(token);
    }
    let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m no token found");
    let _ = writeln!(stdout, "  \x1b[90mstore with: pass insert {provider}/{}-token\x1b[0m", args.oracle);
    let _ = writeln!(stdout, "  \x1b[90mthen: maw channel setup {} {provider} --pass {provider}/{}-token\x1b[0m", args.oracle, args.oracle);
    Err((0, stdout.clone()))
}

fn channel_setup_discord_guild(args: &ChannelSetupArgs, token: &str, stdout: &mut String, total: usize) {
    use std::fmt::Write as _;

    channel_push_setup_step(stdout, 4, total, "Guild / Server");
    let guilds = channel_discord_guilds(token);
    if guilds.is_empty() {
        stdout.push_str("  \x1b[33m⚠\x1b[0m no guilds found — bot may need to be invited first\n");
        if let Some(client_id) = channel_extract_client_id(token) {
            let _ = writeln!(stdout, "  \x1b[90minvite: https://discord.com/oauth2/authorize?client_id={client_id}&scope=bot&permissions=101376\x1b[0m");
        }
        return;
    }
    for (index, guild) in guilds.iter().enumerate() {
        let selected = if args.guild_id.as_ref().is_some_and(|id| id == &guild.id) { " ←" } else { "" };
        let _ = writeln!(stdout, "    {}. {} ({}){selected}", index + 1, guild.name, guild.id);
    }
    let chosen = args
        .guild_id
        .as_ref()
        .and_then(|id| guilds.iter().find(|guild| &guild.id == id))
        .or_else(|| (args.guild_id.is_none()).then(|| guilds.first()).flatten());
    if let Some(guild) = chosen { let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m guild: {}", guild.name); }
}

fn channel_setup_register(args: &ChannelSetupArgs, plugin_id: &str, stdout: &mut String, total: usize) -> Result<(), (i32, String)> {
    use std::fmt::Write as _;

    channel_push_setup_step(stdout, 6, total, "Register channel");
    let path = channel_oracle_config_path(&args.oracle);
    let mut config = channel_load_config_at(&path).unwrap_or_default();
    let mut env = args.env.clone();
    if matches!(args.provider, ChannelSetupProvider::Discord) {
        env.entry("DISCORD_STATE_DIR".to_owned()).or_insert_with(|| format!("~/.claude/channels/{}", args.oracle));
    }
    let plugin = ChannelPlugin { id: plugin_id.to_owned(), env: (!env.is_empty()).then_some(env), ..ChannelPlugin::default() };
    if let Some(pass_key) = &args.pass_key { config.token_source = Some(format!("pass:{pass_key}")); }
    if config.plugins.iter().any(|existing| existing.id == plugin_id) {
        stdout.push_str("  \x1b[32m✓\x1b[0m already registered\n");
    } else {
        config.plugins.push(plugin);
        channel_archive_existing_config(&path)?;
        channel_save_config_at(&path, &config)?;
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m registered: {} → {plugin_id}", args.oracle);
    }
    Ok(())
}

fn channel_push_setup_step(stdout: &mut String, step: usize, total: usize, label: &str) {
    use std::fmt::Write as _;

    let _ = writeln!(stdout, "\n  \x1b[36mStep {step}/{total}: {label}\x1b[0m");
}

fn channel_push_setup_done(stdout: &mut String, oracle: &str, provider: &str) {
    use std::fmt::Write as _;

    stdout.push_str("\n  \x1b[32m✅ Setup complete!\x1b[0m\n\n");
    stdout.push_str("  Start oracle with channels:\n");
    let _ = writeln!(stdout, "    \x1b[36mmaw wake {oracle}\x1b[0m\n");
    stdout.push_str("  \x1b[90mNat pre-approved — no pairing needed. Bot responds immediately.\x1b[0m\n");
    if provider != "imessage" { stdout.push_str("  \x1b[90mAdd others: /discord:access allow <user-id>\x1b[0m\n"); }
}

fn channel_state_dir(oracle: &str) -> std::path::PathBuf {
    channel_channels_base().join(oracle)
}

fn channel_create_private_dir(path: &std::path::Path) -> Result<(), (i32, String)> {
    std::fs::create_dir_all(path).map_err(|error| (1, format!("channel: create state dir failed: {error}")))?;
    channel_chmod(path, 0o700)
}

fn channel_rewrite_existing_env(provider: &str, state_dir: &std::path::Path, token: &str, stdout: &mut String) -> Result<(), (i32, String)> {
    let token_key = if provider == "discord" { "DISCORD_BOT_TOKEN" } else { "TELEGRAM_BOT_TOKEN" };
    let env_file = state_dir.join(".env");
    if !env_file.exists() { return Ok(()); }
    channel_atomic_write_private(&env_file, &format!("{token_key}={token}\n"), 0o600)?;
    stdout.push_str("  \x1b[32m✓\x1b[0m .env written (0o600)\n");
    Ok(())
}

fn channel_read_env_token(provider: &str, env_file: &std::path::Path) -> Option<String> {
    let token_key = if provider == "discord" { "DISCORD_BOT_TOKEN" } else { "TELEGRAM_BOT_TOKEN" };
    let raw = std::fs::read_to_string(env_file).ok()?;
    for line in raw.lines() {
        let Some((key, value)) = line.split_once('=') else { continue; };
        if key == token_key && !value.trim().is_empty() { return Some(value.trim().to_owned()); }
    }
    None
}

fn channel_pass_show(pass_key: &str) -> Result<String, ()> {
    if let Some(fake) = std::env::var_os("MAW_RS_CHANNEL_FAKE_PASS_TOKEN") {
        return Ok(fake.to_string_lossy().trim().to_owned());
    }
    let output = std::process::Command::new("pass").arg("show").arg(pass_key).output().map_err(|_| ())?;
    if !output.status.success() { return Err(()); }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn channel_discord_guilds(_token: &str) -> Vec<ChannelDiscordGuild> {
    let Ok(raw) = std::env::var("MAW_RS_CHANNEL_FAKE_DISCORD_GUILDS") else { return Vec::new(); };
    raw.split(';')
        .filter_map(|entry| {
            let (id, name) = entry.split_once(':')?;
            Some(ChannelDiscordGuild { id: id.to_owned(), name: name.to_owned() })
        })
        .collect()
}

fn channel_extract_client_id(token: &str) -> Option<String> {
    let first = token.split('.').next()?;
    channel_decode_base64_segment(first).ok().filter(|value| !value.is_empty())
}

fn channel_decode_base64_segment(value: &str) -> Result<String, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut bytes = Vec::new();
    for byte in value.bytes() {
        let normalized = match byte { b'-' => b'+', b'_' => b'/', b'=' => continue, other => other };
        let Some(index) = TABLE.iter().position(|candidate| *candidate == normalized) else { return Err(()); };
        let sextet = u32::try_from(index).map_err(|_| ())?;
        bits = (bits << 6) | sextet;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            bytes.push(((bits >> bit_count) & 0xff) as u8);
        }
    }
    String::from_utf8(bytes).map_err(|_| ())
}

fn channel_seed_access_json(state_dir: &std::path::Path, stdout: &mut String) -> Result<(), (i32, String)> {
    let access_path = state_dir.join("access.json");
    let seed = channel_access_seed();
    if !access_path.exists() {
        channel_save_access_json(&access_path, &seed)?;
        stdout.push_str("  \x1b[32m✓\x1b[0m access.json seeded (Nat pre-approved, dmPolicy: allowlist)\n");
        stdout.push_str("  \x1b[90mno pairing needed — Nat can DM immediately\x1b[0m\n");
        return Ok(());
    }
    if channel_read_access_json(&access_path).is_some() {
        stdout.push_str("  \x1b[32m✓\x1b[0m existing access.json preserved\n");
        return Ok(());
    }
    channel_archive_existing_config(&access_path)?;
    channel_save_access_json(&access_path, &seed)?;
    stdout.push_str("  \x1b[32m✓\x1b[0m access.json reset + Nat seeded\n");
    Ok(())
}

fn channel_access_seed() -> ChannelAccessConfig {
    ChannelAccessConfig {
        dm_policy: "allowlist".to_owned(),
        allow_from: vec!["691531480689541170".to_owned()],
        groups: serde_json::json!({}),
        pending: serde_json::json!({}),
    }
}

fn channel_read_access_json(path: &std::path::Path) -> Option<serde_json::Value> {
    let value = channel_read_json(path)?;
    let object = value.as_object()?;
    object.get("dmPolicy")?.as_str()?;
    object.get("allowFrom")?.as_array()?;
    Some(value)
}

fn channel_save_access_json(path: &std::path::Path, access: &ChannelAccessConfig) -> Result<(), (i32, String)> {
    let json = serde_json::to_string_pretty(access).map_err(|error| (1, format!("channel: serialize access failed: {error}")))?;
    channel_atomic_write(path, &(json + "\n"))
}

