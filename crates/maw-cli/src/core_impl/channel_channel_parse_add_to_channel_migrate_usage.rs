fn channel_parse_add(argv: &[String]) -> Result<ChannelAddArgs, (i32, String)> {
    if argv.len() < 2 {
        return Err((1, "usage: maw channel add <oracle> <plugin-id>".to_owned()));
    }
    let oracle = channel_validate_name("oracle", &argv[0])?;
    let plugin_id = channel_expand_plugin_id(&argv[1])?;
    let mut repo_path = None;
    let mut env = std::collections::BTreeMap::new();
    let mut pass_key = None;
    let mut index = 2;
    while index < argv.len() {
        match argv[index].as_str() {
            "--repo" => {
                let value = channel_take_flag_value(argv, index, "--repo")?;
                repo_path = Some(channel_validate_repo_path(value)?);
                index += 2;
            }
            "--env" => {
                let value = channel_take_flag_value(argv, index, "--env")?;
                let (key, env_value) = channel_validate_env_assignment(value)?;
                env.insert(key, env_value);
                index += 2;
            }
            "--pass" => {
                let value = channel_take_flag_value(argv, index, "--pass")?;
                pass_key = Some(channel_validate_pass_key(value)?);
                index += 2;
            }
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            other if other.starts_with('-') => return Err((2, format!("channel add: unknown flag {other}"))),
            other => return Err((2, format!("channel add: unexpected argument {other}"))),
        }
    }
    Ok(ChannelAddArgs { oracle, plugin_id, repo_path, env, pass_key })
}

fn channel_parse_rm(argv: &[String]) -> Result<(String, Option<String>), (i32, String)> {
    match argv {
        [] => Err((1, "usage: maw channel rm <oracle> [plugin-id]".to_owned())),
        [oracle] => Ok((channel_validate_name("oracle", oracle)?, None)),
        [oracle, plugin] => Ok((channel_validate_name("oracle", oracle)?, Some(channel_expand_plugin_id(plugin)?))),
        _ => Err((2, "channel rm accepts oracle and optional plugin only".to_owned())),
    }
}

fn channel_new_plugin(args: &ChannelAddArgs) -> ChannelPlugin {
    let mut env = args.env.clone();
    if args.plugin_id.contains("discord") && !env.contains_key("DISCORD_STATE_DIR") {
        let state_dir = if args.repo_path.is_some() { ".claude/channel-state".to_owned() } else { format!("~/.claude/channels/{}", args.oracle) };
        env.insert("DISCORD_STATE_DIR".to_owned(), state_dir);
    }
    ChannelPlugin { id: args.plugin_id.clone(), env: (!env.is_empty()).then_some(env), ..ChannelPlugin::default() }
}

fn channel_push_added_env(stdout: &mut String, plugin: &ChannelPlugin) {
    use std::fmt::Write as _;

    if let Some(env) = &plugin.env {
        for (key, value) in env {
            let value = channel_display_env_value(key, value);
            let _ = writeln!(stdout, "     env: {key}={value}");
        }
    }
}

fn channel_push_added_token(stdout: &mut String, config: &ChannelConfig) {
    use std::fmt::Write as _;

    if let Some(token_source) = &config.token_source {
        let token_source = channel_display_token_source(token_source);
        let _ = writeln!(stdout, "     token: {token_source}");
    }
}

fn channel_expand_plugin_id(value: &str) -> Result<String, (i32, String)> {
    if value.starts_with("github:") {
        return Err((1, "channel add: github providers are handled by the setup slice".to_owned()));
    }
    channel_validate_plugin_id(value)?;
    if value.contains(':') || value.contains('@') { Ok(value.to_owned()) } else { Ok(format!("plugin:{value}@claude-plugins-official")) }
}

fn channel_validate_plugin_id(value: &str) -> Result<(), (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains("..")
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err((2, "channel: invalid plugin".to_owned()));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '@')) {
        return Err((2, "channel: invalid plugin".to_owned()));
    }
    Ok(())
}

fn channel_take_flag_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, (i32, String)> {
    argv.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (2, format!("channel add: missing {flag} value")))
}

fn channel_validate_env_assignment(value: &str) -> Result<(String, String), (i32, String)> {
    let Some((key, env_value)) = value.split_once('=') else {
        return Err((2, "channel add: --env must be KEY=VALUE".to_owned()));
    };
    if key.is_empty()
        || key.starts_with('-')
        || key.chars().any(char::is_control)
        || !key.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err((2, "channel: invalid env key".to_owned()));
    }
    if env_value.chars().any(char::is_control) { return Err((2, "channel: invalid env value".to_owned())); }
    Ok((key.to_owned(), env_value.to_owned()))
}

fn channel_validate_pass_key(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains("..")
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err((2, "channel: invalid pass key".to_owned()));
    }
    Ok(value.to_owned())
}

fn channel_validate_repo_path(value: &str) -> Result<std::path::PathBuf, (i32, String)> {
    let path = std::path::Path::new(value);
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err((2, "channel: invalid repo path".to_owned()));
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => return Err((2, "channel: invalid repo path".to_owned())),
            std::path::Component::Normal(name) if name.to_string_lossy().starts_with('-') => {
                return Err((2, "channel: invalid repo path".to_owned()));
            }
            _ => {}
        }
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path)).map_err(|error| (1, format!("channel: cannot resolve repo path: {error}")))
    }
}



fn channel_migrate(input: &[String]) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let migrate_args = channel_parse_migrate(input)?;
    let stems = if migrate_args.targets.is_empty() {
        channel_list_all_configs().into_iter().map(|(oracle, _)| oracle).collect::<Vec<_>>()
    } else {
        migrate_args.targets.clone()
    };
    if stems.is_empty() { return Ok("  no oracles with global channel config to migrate\n".to_owned()); }

    let mut counts = ChannelMigrateCounts::default();
    let mut stdout = String::new();
    for stem in stems {
        channel_migrate_one(&stem, &migrate_args, &mut counts, &mut stdout)?;
    }
    let _ = writeln!(stdout, "\n  {} migrated, {} skipped, {} failed", counts.migrated, counts.skipped, counts.failed);
    if counts.migrated > 0 && !migrate_args.remove_global && !migrate_args.dry_run {
        stdout.push_str("  tip: re-run with --remove-global to delete the global config copies.\n");
    }
    Ok(stdout)
}

fn channel_parse_migrate(argv: &[String]) -> Result<ChannelMigrateArgs, (i32, String)> {
    let mut to_repo = false;
    let mut dry_run = false;
    let mut remove_global = false;
    let mut targets = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--to-repo" => to_repo = true,
            "--dry-run" => dry_run = true,
            "--remove-global" => remove_global = true,
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("channel migrate: unknown flag {value}"))),
            value => targets.push(channel_validate_name("oracle", value)?),
        }
    }
    if !to_repo { return Err((1, channel_migrate_usage())); }
    Ok(ChannelMigrateArgs { targets, dry_run, remove_global })
}

fn channel_migrate_usage() -> String {
    "usage: maw channel migrate --to-repo [oracle...] [--dry-run] [--remove-global]\n  copies global ~/.claude/channels/<oracle>/config.json into\n  <repo>/.claude/channel.json so config travels with the repo (#1195).\n\n  no [oracle...] args = migrate every oracle with global config.\n  --dry-run            = show what would happen, no writes.\n  --remove-global      = delete the global config after a successful copy.".to_owned()
}

