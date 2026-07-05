fn channel_migrate_one(stem: &str, args: &ChannelMigrateArgs, counts: &mut ChannelMigrateCounts, stdout: &mut String) -> Result<(), (i32, String)> {
    use std::fmt::Write as _;

    let Some(global) = channel_load_oracle_config(stem) else {
        let _ = writeln!(stdout, "  \x1b[90m·\x1b[0m {stem}: no global config — skip");
        counts.skipped += 1;
        return Ok(());
    };
    let Some(repo_path) = channel_resolve_repo_for_stem(stem) else {
        let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m {stem}: no local repo (tried ghq for '{stem}' and '-oracle' variants) — skip");
        counts.failed += 1;
        return Ok(());
    };
    let repo_config = channel_repo_config_path(&repo_path);
    if channel_load_config_at(&repo_config).is_some() {
        let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m {stem}: {}/.claude/channel.json already exists — skip (delete it first)", repo_path.display());
        counts.skipped += 1;
        return Ok(());
    }
    if args.dry_run {
        let _ = writeln!(stdout, "  \x1b[36m·\x1b[0m DRY-RUN {stem}: would write {}/.claude/channel.json ({} plugin(s))", repo_path.display(), global.plugins.len());
        counts.migrated += 1;
        return Ok(());
    }

    channel_save_config_at(&repo_config, &global)?;
    channel_save_repo_gitignore(&repo_path)?;
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {stem}: → {}/.claude/channel.json", repo_path.display());
    if args.remove_global { channel_remove_global_after_copy(stem, stdout)?; }
    counts.migrated += 1;
    Ok(())
}

fn channel_resolve_repo_for_stem(stem: &str) -> Option<std::path::PathBuf> {
    let candidates = channel_repo_candidates(stem);
    if let Some(root) = std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_ROOT") {
        let root = std::path::PathBuf::from(root);
        return candidates.into_iter().map(|candidate| root.join(candidate)).find(|path| path.exists());
    }
    let output = std::process::Command::new("ghq").arg("list").arg("--full-path").output().ok()?;
    if !output.status.success() { return None; }
    let listing = String::from_utf8_lossy(&output.stdout);
    candidates.into_iter().find_map(|candidate| channel_match_repo_listing(&listing, &candidate))
}

fn channel_repo_candidates(stem: &str) -> Vec<String> {
    let alternate = stem.strip_suffix("-oracle").map_or_else(|| format!("{stem}-oracle"), str::to_owned);
    vec![stem.to_owned(), alternate]
}

fn channel_match_repo_listing(listing: &str, candidate: &str) -> Option<std::path::PathBuf> {
    let suffix = format!("/{candidate}");
    listing.lines().map(str::trim).filter(|line| !line.is_empty()).find_map(|line| {
        if line.ends_with(&suffix) || line.ends_with(candidate) { Some(std::path::PathBuf::from(line)) } else { None }
    })
}

fn channel_remove_global_after_copy(stem: &str, stdout: &mut String) -> Result<(), (i32, String)> {
    use std::fmt::Write as _;

    let config_path = channel_oracle_config_path(stem);
    channel_archive_existing_config(&config_path)?;
    match std::fs::remove_file(&config_path) {
        Ok(()) => {
            let dir = channel_state_dir(stem);
            let _ = std::fs::remove_dir(&dir);
            stdout.push_str("    \x1b[90m✓ removed global config\x1b[0m\n");
        }
        Err(error) => {
            let _ = writeln!(stdout, "    \x1b[33m⚠ failed to remove global: {error}\x1b[0m");
        }
    }
    Ok(())
}

fn channel_setup(input: &[String]) -> Result<String, (i32, String)> {
    let setup_args = channel_parse_setup(input)?;
    match &setup_args.provider {
        ChannelSetupProvider::Github(_) => channel_setup_github(&setup_args),
        _ => channel_setup_official(&setup_args),
    }
}

fn channel_parse_setup(argv: &[String]) -> Result<ChannelSetupArgs, (i32, String)> {
    if argv.len() < 2 {
        return Err((1, "usage: maw channel setup <oracle> <provider>".to_owned()));
    }
    let oracle = channel_validate_name("oracle", &argv[0])?;
    let provider = channel_validate_setup_provider(&argv[1])?;
    let mut pass_key = None;
    let mut guild_id = None;
    let mut env = std::collections::BTreeMap::new();
    let mut index = 2;
    while index < argv.len() {
        match argv[index].as_str() {
            "--pass" => {
                let value = channel_take_setup_flag_value(argv, index, "--pass")?;
                pass_key = Some(channel_validate_pass_key(value)?);
                index += 2;
            }
            "--guild" => {
                let value = channel_take_setup_flag_value(argv, index, "--guild")?;
                guild_id = Some(channel_validate_snowflake(value)?);
                index += 2;
            }
            "--env" => {
                let value = channel_take_setup_flag_value(argv, index, "--env")?;
                let (key, env_value) = channel_validate_env_assignment(value)?;
                env.insert(key, env_value);
                index += 2;
            }
            "--no-interactive" => index += 1,
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            other if other.starts_with('-') => return Err((2, format!("channel setup: unknown flag {other}"))),
            other => return Err((2, format!("channel setup: unexpected argument {other}"))),
        }
    }
    Ok(ChannelSetupArgs { oracle, provider, pass_key, guild_id, env })
}

fn channel_validate_setup_provider(value: &str) -> Result<ChannelSetupProvider, (i32, String)> {
    match value {
        "discord" => Ok(ChannelSetupProvider::Discord),
        "telegram" => Ok(ChannelSetupProvider::Telegram),
        "imessage" => Ok(ChannelSetupProvider::Imessage),
        provider if provider.starts_with("github:") => {
            let repo = provider.trim_start_matches("github:");
            channel_validate_github_repo(repo)?;
            Ok(ChannelSetupProvider::Github(repo.to_owned()))
        }
        _ => Err((2, format!("channel setup: unknown provider {value}"))),
    }
}

fn channel_validate_github_repo(value: &str) -> Result<(), (i32, String)> {
    let Some((org, repo)) = value.split_once('/') else {
        return Err((2, "channel setup: invalid github provider".to_owned()));
    };
    if org.is_empty() || repo.is_empty() || value.contains("..") || value.contains('\\') {
        return Err((2, "channel setup: invalid github provider".to_owned()));
    }
    for part in [org, repo] {
        if part.starts_with('-') || part.chars().any(char::is_control) {
            return Err((2, "channel setup: invalid github provider".to_owned()));
        }
        if !part.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
            return Err((2, "channel setup: invalid github provider".to_owned()));
        }
    }
    Ok(())
}

fn channel_take_setup_flag_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, (i32, String)> {
    argv.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (2, format!("channel setup: missing {flag} value")))
}

fn channel_validate_snowflake(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.len() > 32
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || !value.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err((2, "channel setup: invalid guild snowflake".to_owned()));
    }
    Ok(value.to_owned())
}

trait ChannelGithubRunner {
    fn ghq_root(&self) -> Result<std::path::PathBuf, (i32, String)>;
    fn repo_exists(&self, path: &std::path::Path) -> bool;
    fn ghq_get(&self, repo: &str, url: &str, root: &std::path::Path) -> Result<(), (i32, String)>;
    fn file_exists(&self, path: &std::path::Path) -> bool;
    fn read_to_string(&self, path: &std::path::Path) -> Result<Option<String>, (i32, String)>;
    fn bun_install_stub(&self, repo: &std::path::Path) -> Result<(), (i32, String)>;
}

struct ChannelSystemGithubRunner;

