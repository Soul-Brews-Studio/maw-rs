fn channel_parse_untrusted_mcp(raw: &str, repo_path: &std::path::Path) -> Result<ChannelGithubMcp, (i32, String)> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| (2, format!("channel setup: invalid .mcp.json: {error}")))?;
    let servers = value
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .filter(|object| !object.is_empty())
        .ok_or_else(|| (2, "channel setup: .mcp.json missing mcpServers".to_owned()))?;
    let (name, server) = servers
        .iter()
        .next()
        .ok_or_else(|| (2, "channel setup: .mcp.json missing mcpServers".to_owned()))?;
    let server_name = channel_validate_mcp_server_name(name)?;
    let command = server.get("command").and_then(serde_json::Value::as_str).unwrap_or("bun");
    let command = channel_validate_mcp_command(command)?;
    let args = server
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| (2, "channel setup: invalid .mcp.json args".to_owned()))
                        .and_then(|arg| channel_validate_mcp_arg(arg, repo_path))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(ChannelGithubMcp {
        plugin_id: format!("server:{server_name}"),
        config: ChannelMcpConfig { command, args, untrusted: Some(true) },
    })
}

fn channel_validate_mcp_server_name(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err((2, "channel setup: invalid .mcp.json server name".to_owned()));
    }
    Ok(value.to_owned())
}

fn channel_validate_mcp_command(value: &str) -> Result<String, (i32, String)> {
    if channel_mcp_token_invalid(value) || value.contains('/') || value.contains('\\') || value.chars().any(char::is_whitespace) {
        return Err((2, "channel setup: invalid .mcp.json command".to_owned()));
    }
    Ok(value.to_owned())
}

fn channel_validate_mcp_arg(value: &str, repo_path: &std::path::Path) -> Result<String, (i32, String)> {
    let resolved = value.replace("${CLAUDE_PLUGIN_ROOT}", &repo_path.display().to_string());
    if channel_mcp_token_invalid(&resolved) {
        return Err((2, "channel setup: invalid .mcp.json args".to_owned()));
    }
    Ok(resolved)
}

fn channel_mcp_token_invalid(value: &str) -> bool {
    value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || value.chars().any(|ch| matches!(ch, ';' | '&' | '|' | '$' | '<' | '>' | '`' | '"' | '\'' | '(' | ')' | '{' | '}'))
}

fn channel_display_args(args: &[String]) -> String {
    if args.is_empty() { String::new() } else { format!(" {}", args.join(" ")) }
}

fn channel_github_repo_path(root: &std::path::Path, repo: &str) -> std::path::PathBuf {
    let (org, name) = repo.split_once('/').unwrap_or((repo, ""));
    root.join("github.com").join(org).join(name)
}

fn channel_canonicalize_ghq_root(path: &std::path::Path) -> Result<std::path::PathBuf, (i32, String)> {
    let root = path.canonicalize().map_err(|error| (1, format!("channel setup: ghq root unavailable: {error}")))?;
    if !root.is_dir() {
        return Err((1, "channel setup: ghq root is not a directory".to_owned()));
    }
    Ok(root)
}

fn channel_canonicalize_github_repo(root: &std::path::Path, repo_path: &std::path::Path) -> Result<std::path::PathBuf, (i32, String)> {
    let repo = repo_path.canonicalize().map_err(|error| (1, format!("channel setup: cloned repo missing: {error}")))?;
    if repo == root || !repo.starts_with(root) {
        return Err((1, "channel setup: cloned repo escaped ghq root".to_owned()));
    }
    Ok(repo)
}

fn channel_run_ghq(args: &[&str], timeout: std::time::Duration) -> Result<String, (i32, String)> {
    use std::io::Read as _;

    let mut child = std::process::Command::new("ghq")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| (1, format!("channel setup: ghq unavailable: {error}")))?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| (1, format!("channel setup: ghq wait failed: {error}")))? {
            let mut stdout = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_string(&mut stdout).map_err(|error| (1, format!("channel setup: ghq stdout failed: {error}")))?;
            }
            if !status.success() {
                return Err((1, "channel setup: ghq command failed".to_owned()));
            }
            return Ok(stdout);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err((1, "channel setup: ghq command timed out".to_owned()));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn channel_setup_official(args: &ChannelSetupArgs) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let provider = args.provider.name();
    let plugin_id = args.provider.plugin_id();
    let total = if matches!(args.provider, ChannelSetupProvider::Imessage) { 4 } else { 7 };
    let mut stdout = String::new();
    let _ = writeln!(stdout, "\n  \x1b[36;1m🔧 {provider} Channel Setup for {}\x1b[0m", args.oracle);
    let _ = writeln!(stdout, "  {}", "─".repeat(45));

    channel_push_setup_step(&mut stdout, 1, total, "Plugin check");
    if channel_is_plugin_installed(provider) {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {plugin_id} installed");
    } else {
        let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m {plugin_id} not installed");
        let _ = writeln!(stdout, "  \x1b[90mrun: /plugin install {provider}@claude-plugins-official\x1b[0m");
        return Ok(stdout);
    }

    if matches!(args.provider, ChannelSetupProvider::Imessage) {
        return channel_setup_imessage(args, stdout, total, &plugin_id);
    }

    let token = channel_setup_token(args, &mut stdout, total)?;
    let state_dir = channel_state_dir(&args.oracle);
    channel_push_setup_step(&mut stdout, 3, total, "State directory");
    channel_create_private_dir(&state_dir)?;
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {}/", channel_tilde_path(&state_dir));
    if args.pass_key.is_none() { channel_rewrite_existing_env(provider, &state_dir, &token, &mut stdout)?; }

    if matches!(args.provider, ChannelSetupProvider::Discord) {
        channel_setup_discord_guild(args, &token, &mut stdout, total);
    } else {
        channel_push_setup_step(&mut stdout, 4, total, "Config");
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m ready");
    }

    channel_push_setup_step(&mut stdout, 5, total, "Access config + seed");
    channel_seed_access_json(&state_dir, &mut stdout)?;
    channel_setup_register(args, &plugin_id, &mut stdout, total)?;
    channel_push_setup_done(&mut stdout, &args.oracle, provider);
    Ok(stdout)
}

fn channel_setup_imessage(args: &ChannelSetupArgs, mut stdout: String, total: usize, plugin_id: &str) -> Result<String, (i32, String)> {
    channel_push_setup_step(&mut stdout, 2, total, "macOS check");
    if !channel_platform_is_macos() {
        stdout.push_str("  \x1b[31m✗\x1b[0m iMessage requires macOS\n");
        return Ok(stdout);
    }
    stdout.push_str("  \x1b[32m✓\x1b[0m macOS detected\n");
    stdout.push_str("  \x1b[90mℹ Full Disk Access required for Messages.app — grant when prompted\x1b[0m\n");
    channel_push_setup_step(&mut stdout, 3, total, "Register channel");
    let path = channel_oracle_config_path(&args.oracle);
    let mut config = channel_load_config_at(&path).unwrap_or_default();
    if !config.plugins.iter().any(|plugin| plugin.id == plugin_id) {
        config.plugins.push(ChannelPlugin { id: plugin_id.to_owned(), ..ChannelPlugin::default() });
        channel_archive_existing_config(&path)?;
        channel_save_config_at(&path, &config)?;
    }
    stdout.push_str("  \x1b[32m✓\x1b[0m registered\n");
    channel_push_setup_step(&mut stdout, 4, total, "Done!");
    channel_push_setup_done(&mut stdout, &args.oracle, "imessage");
    Ok(stdout)
}

