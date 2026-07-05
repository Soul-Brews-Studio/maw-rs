impl ChannelGithubRunner for ChannelSystemGithubRunner {
    fn ghq_root(&self) -> Result<std::path::PathBuf, (i32, String)> {
        if let Some(root) = std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_ROOT") {
            return Ok(std::path::PathBuf::from(root));
        }
        let stdout = channel_run_ghq(&["root"], std::time::Duration::from_secs(10))?;
        let root = stdout.trim();
        if root.is_empty() {
            return Err((1, "channel setup: ghq root returned empty path".to_owned()));
        }
        Ok(std::path::PathBuf::from(root))
    }

    fn repo_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn ghq_get(&self, repo: &str, url: &str, root: &std::path::Path) -> Result<(), (i32, String)> {
        if std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_GET_FAIL").is_some() {
            return Err((1, "channel setup: ghq get failed".to_owned()));
        }
        if let Some(log_path) = std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_GET_LOG") {
            use std::io::Write as _;

            let path = std::path::PathBuf::from(log_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel setup: fake ghq log dir failed: {error}")))?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| (1, format!("channel setup: fake ghq log failed: {error}")))?;
            writeln!(file, "ghq get {url}").map_err(|error| (1, format!("channel setup: fake ghq log failed: {error}")))?;
            std::fs::create_dir_all(channel_github_repo_path(root, repo))
                .map_err(|error| (1, format!("channel setup: fake ghq create repo failed: {error}")))?;
            return Ok(());
        }
        let _ = repo;
        channel_run_ghq(&["get", url], std::time::Duration::from_secs(30)).map(|_| ())
    }

    fn file_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn read_to_string(&self, path: &std::path::Path) -> Result<Option<String>, (i32, String)> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Ok(Some(raw)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err((1, format!("channel setup: read {} failed: {error}", path.display()))),
        }
    }

    fn bun_install_stub(&self, repo: &std::path::Path) -> Result<(), (i32, String)> {
        if let Some(log_path) = std::env::var_os("MAW_RS_CHANNEL_FAKE_BUN_INSTALL_LOG") {
            use std::io::Write as _;

            let path = std::path::PathBuf::from(log_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel setup: fake bun log dir failed: {error}")))?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| (1, format!("channel setup: fake bun log failed: {error}")))?;
            writeln!(file, "bun install --cwd {}", repo.display()).map_err(|error| (1, format!("channel setup: fake bun log failed: {error}")))?;
        }
        if std::env::var_os("MAW_RS_CHANNEL_FAKE_BUN_INSTALL_FAIL").is_some() {
            return Err((1, "channel setup: bun install failed".to_owned()));
        }
        Ok(())
    }
}

fn channel_setup_github(args: &ChannelSetupArgs) -> Result<String, (i32, String)> {
    channel_setup_github_with_runner(args, &ChannelSystemGithubRunner)
}

fn channel_setup_github_with_runner(args: &ChannelSetupArgs, runner: &dyn ChannelGithubRunner) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let ChannelSetupProvider::Github(repo) = &args.provider else {
        return Err((2, "channel setup: github runner used for non-github provider".to_owned()));
    };
    let root_raw = runner.ghq_root()?;
    let root = channel_canonicalize_ghq_root(&root_raw)?;
    let repo_path = channel_github_repo_path(&root, repo);
    let url = format!("https://github.com/{repo}");
    let mut cloned = false;

    if !runner.repo_exists(&repo_path) {
        runner.ghq_get(repo, &url, &root)?;
        cloned = true;
    }
    let repo_canon = channel_canonicalize_github_repo(&root, &repo_path)?;

    let mcp = channel_github_mcp_config(runner, &repo_canon)?;
    let bun_result = if runner.file_exists(&repo_canon.join("package.json")) {
        Some(runner.bun_install_stub(&repo_canon))
    } else {
        None
    };
    let config_path = channel_oracle_config_path(&args.oracle);
    let mut config = channel_load_config_at(&config_path).unwrap_or_default();
    let mut wrote_config = false;
    let plugin = channel_github_plugin(args, repo, &repo_canon, &mcp);
    if let Some(pass_key) = &args.pass_key {
        config.token_source = Some(format!("pass:{pass_key}"));
    }
    if config.plugins.iter().any(|existing| existing.id == plugin.id) {
        if args.pass_key.is_some() {
            channel_archive_existing_config(&config_path)?;
            channel_save_config_private_at(&config_path, &config)?;
            wrote_config = true;
        }
    } else {
        config.plugins.push(plugin.clone());
        channel_archive_existing_config(&config_path)?;
        channel_save_config_private_at(&config_path, &config)?;
        wrote_config = true;
    }

    let mut stdout = String::new();
    let _ = writeln!(stdout, "\n  \x1b[36;1m🔧 Git Channel Setup for {}\x1b[0m", args.oracle);
    let _ = writeln!(stdout, "  {}", "─".repeat(45));
    let _ = writeln!(stdout, "\n  \x1b[36mStep 1/4: Locate repo\x1b[0m");
    let _ = writeln!(stdout, "  source: github:{repo}");
    let _ = writeln!(stdout, "  ghq root: {}", root.display());
    let _ = writeln!(stdout, "  repo: {}", repo_canon.display());
    if cloned {
        stdout.push_str("  \x1b[32m✓\x1b[0m cloned with ghq get\n");
    } else {
        stdout.push_str("  \x1b[32m✓\x1b[0m repo already present — clone skipped\n");
    }
    let _ = writeln!(stdout, "\n  \x1b[36mStep 2/4: Token + dependencies\x1b[0m");
    if let Some(pass_key) = &args.pass_key {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m token source: pass:{pass_key} (reference only)");
    } else {
        stdout.push_str("  \x1b[90m· token source not configured\x1b[0m\n");
    }
    match bun_result {
        Some(Ok(())) => stdout.push_str("  \x1b[33m⚠\x1b[0m package.json found — bun install stubbed/best-effort; continuing\n"),
        Some(Err((_, message))) => {
            let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m {message}; continuing");
        }
        None => stdout.push_str("  \x1b[90m· no package.json — bun install skipped\x1b[0m\n"),
    }
    let _ = writeln!(stdout, "\n  \x1b[36mStep 3/4: MCP record\x1b[0m");
    let untrusted_label = if mcp.config.untrusted == Some(true) { " (untrusted .mcp.json, validated)" } else { " (native default)" };
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {} → {}{}{}", mcp.plugin_id, mcp.config.command, channel_display_args(&mcp.config.args), untrusted_label);
    stdout.push_str("  \x1b[90m· setup records MCP only; it does not spawn the MCP command\x1b[0m\n");
    let _ = writeln!(stdout, "\n  \x1b[36mStep 4/4: Register config\x1b[0m");
    if wrote_config {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m wrote {} (0600)", channel_tilde_path(&config_path));
    } else {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m already registered: {} → {}", args.oracle, plugin.id);
    }
    stdout.push_str("  \x1b[90m· dev-server spawn is out of scope for setup\x1b[0m\n");
    stdout.push_str("\n  \x1b[32m✅ Git channel setup complete\x1b[0m\n");
    Ok(stdout)
}

#[derive(Debug)]
struct ChannelGithubMcp {
    plugin_id: String,
    config: ChannelMcpConfig,
}

fn channel_github_plugin(args: &ChannelSetupArgs, repo: &str, repo_path: &std::path::Path, mcp: &ChannelGithubMcp) -> ChannelPlugin {
    ChannelPlugin {
        id: mcp.plugin_id.clone(),
        env: (!args.env.is_empty()).then_some(args.env.clone()),
        source: Some(format!("github:{repo}")),
        path: Some(repo_path.display().to_string()),
        mcp: Some(mcp.config.clone()),
        dev: Some(true),
    }
}

fn channel_github_mcp_config(runner: &dyn ChannelGithubRunner, repo_path: &std::path::Path) -> Result<ChannelGithubMcp, (i32, String)> {
    let path = repo_path.join(".mcp.json");
    let Some(raw) = runner.read_to_string(&path)? else {
        return Ok(ChannelGithubMcp {
            plugin_id: "server:relay".to_owned(),
            config: ChannelMcpConfig {
                command: "bun".to_owned(),
                args: vec!["run".to_owned(), "--cwd".to_owned(), repo_path.display().to_string(), "start".to_owned()],
                untrusted: None,
            },
        });
    };
    channel_parse_untrusted_mcp(&raw, repo_path)
}

