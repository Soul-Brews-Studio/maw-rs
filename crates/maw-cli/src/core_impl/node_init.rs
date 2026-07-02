const DISPATCH_105: &[DispatcherEntry] = &[
    DispatcherEntry { command: "init", handler: Handler::Sync(init_run_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct InitNativeOptions {
    node: String,
    ghq_root: Option<String>,
    token: Option<String>,
    federate: bool,
    peers: Vec<(String, String)>,
    federation_token: Option<String>,
    force: bool,
    backup: bool,
}

fn init_run_command(argv: &[String]) -> CliOutput {
    if argv.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h")) {
        return CliOutput { code: 0, stdout: init_port_usage(), stderr: String::new() };
    }
    if argv.iter().any(|arg| arg == "--non-interactive") {
        return init_run_non_interactive(argv);
    }
    init_run_interactive(argv)
}

fn init_run_non_interactive(argv: &[String]) -> CliOutput {
    match init_parse_options(argv) {
        Ok(opts) => init_write_config(&opts, false),
        Err(error) => init_port_error(&error),
    }
}

fn init_run_interactive(argv: &[String]) -> CliOutput {
    if argv.iter().any(|arg| arg == "--non-interactive") {
        return init_run_non_interactive(argv);
    }
    match init_collect_answers_from_tty() {
        Ok(mut opts) => {
            opts.force = argv.iter().any(|arg| arg == "--force");
            opts.backup = argv.iter().any(|arg| arg == "--backup");
            init_write_config(&opts, true)
        }
        Err(error) => init_port_error(&error),
    }
}

fn init_parse_options(argv: &[String]) -> Result<InitNativeOptions, String> {
    let mut node = std::env::var("HOSTNAME").unwrap_or_else(|_| "local".to_owned());
    let mut ghq_root = None;
    let mut token = None;
    let mut federate = false;
    let mut peer_urls = Vec::new();
    let mut peer_names = Vec::new();
    let mut federation_token = None;
    let mut force = false;
    let mut backup = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--non-interactive" => {}
            "--node" => { node = init_required_value(argv, &mut index, "--node")?; }
            "--ghq-root" => { ghq_root = Some(init_expand_home(&init_required_value(argv, &mut index, "--ghq-root")?)); }
            "--token" => { token = Some(init_required_value(argv, &mut index, "--token")?); }
            "--federate" => federate = true,
            "--peer" => { peer_urls.push(init_required_value(argv, &mut index, "--peer")?); }
            "--peer-name" => { peer_names.push(init_required_value(argv, &mut index, "--peer-name")?); }
            "--federation-token" => { federation_token = Some(init_required_value(argv, &mut index, "--federation-token")?); }
            "--force" => force = true,
            "--backup" => backup = true,
            other if other.starts_with('-') => return Err(format!("maw init: unknown flag {other}")),
            other => return Err(format!("maw init: unexpected argument {other}")),
        }
        index += 1;
    }
    init_validate_node_name(&node)?;
    let mut peers = Vec::new();
    for (idx, url) in peer_urls.iter().enumerate() {
        init_validate_peer_url(url).map_err(|err| format!("--peer #{}: {err}", idx + 1))?;
        let name = peer_names.get(idx).cloned().unwrap_or_else(|| format!("peer-{}", idx + 1));
        init_validate_peer_name(&name).map_err(|err| format!("--peer-name #{}: {err}", idx + 1))?;
        peers.push((name, url.clone()));
    }
    if !peers.is_empty() { federate = true; }
    Ok(InitNativeOptions { node, ghq_root, token, federate, peers, federation_token, force, backup })
}

fn init_required_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    argv.get(*index).filter(|value| !value.starts_with('-')).cloned().ok_or_else(|| format!("{flag} requires a value"))
}

fn init_collect_answers_from_tty() -> Result<InitNativeOptions, String> {
    let default_node = std::env::var("HOSTNAME").unwrap_or_else(|_| "local".to_owned());
    let node = init_ask_tty("Node name (this machine's identity in the federation)", &default_node)?;
    init_validate_node_name(&node)?;
    let token = init_ask_tty("Claude token (blank = use $CLAUDE_CODE_OAUTH_TOKEN or ~/.claude/credentials)", "")?;
    let federate_raw = init_ask_tty("Federate with other machines? (y/N)", "N")?.to_lowercase();
    let federate = matches!(federate_raw.as_str(), "y" | "yes");
    let mut peers = Vec::new();
    if federate {
        for idx in 1..=32 {
            let url = init_ask_tty(&format!("Peer {idx} URL"), "done")?;
            if url.is_empty() || url == "done" { break; }
            init_validate_peer_url(&url)?;
            let name = init_ask_tty(&format!("Peer {idx} name (short label)"), &format!("peer-{idx}"))?;
            init_validate_peer_name(&name)?;
            peers.push((name, url));
        }
    }
    Ok(InitNativeOptions {
        node,
        ghq_root: None,
        token: (!token.is_empty()).then_some(token),
        federate,
        peers,
        federation_token: None,
        force: false,
        backup: false,
    })
}

fn init_ask_tty(question: &str, default_value: &str) -> Result<String, String> {
    let prompt = if default_value.is_empty() { format!("{question}: ") } else { format!("{question} [{default_value}]: ") };
    let mut tty = std::fs::OpenOptions::new().read(true).write(true).open("/dev/tty")
        .map_err(|_| "/dev/tty unavailable — use --non-interactive".to_owned())?;
    std::io::Write::write_all(&mut tty, prompt.as_bytes()).map_err(|error| format!("write /dev/tty: {error}"))?;
    std::io::Write::flush(&mut tty).map_err(|error| format!("flush /dev/tty: {error}"))?;
    let mut input = String::new();
    let mut reader = std::io::BufReader::new(tty);
    std::io::BufRead::read_line(&mut reader, &mut input).map_err(|error| format!("read /dev/tty: {error}"))?;
    let trimmed = input.trim();
    Ok(if trimmed.is_empty() { default_value.to_owned() } else { trimmed.to_owned() })
}

fn init_write_config(opts: &InitNativeOptions, interactive: bool) -> CliOutput {
    let path = active_config_dir().join("maw.config.json");
    if path.exists() && !opts.force && !opts.backup {
        return init_port_error(&format!("Config exists at {}. Use --force to overwrite or --backup to preserve + overwrite.", path.display()));
    }
    let mut stdout = String::new();
    if path.exists() && opts.backup {
        let backup = match init_backup_config(&path) {
            Ok(backup) => backup,
            Err(error) => return init_port_error(&error),
        };
        let _ = writeln!(stdout, "\x1b[32m✓\x1b[0m backed up to {}", backup.display());
    }
    let federation_token = opts.federate.then(|| opts.federation_token.clone().unwrap_or_else(init_generate_federation_token));
    let mut config = serde_json::json!({
        "host": "local",
        "node": opts.node,
        "port": 3456,
        "oracleUrl": "http://localhost:47779",
        "env": {},
        "commands": { "default": "claude --dangerously-skip-permissions --continue" },
        "sessions": {},
    });
    if let Some(token) = &opts.token { config["env"]["CLAUDE_CODE_OAUTH_TOKEN"] = serde_json::json!(token); }
    if let Some(root) = &opts.ghq_root { config["ghqRoot"] = serde_json::json!(root); }
    if opts.federate {
        config["federationToken"] = serde_json::json!(federation_token.clone().unwrap_or_default());
        config["namedPeers"] = serde_json::Value::Array(opts.peers.iter().map(|(name, url)| serde_json::json!({"name": name, "url": url})).collect());
    }
    if let Err(error) = init_write_json_atomic(&path, &config) { return init_port_error(&error); }
    if interactive { stdout.push_str("\x1b[1mmaw init\x1b[0m — first-run setup\n\n"); }
    let _ = writeln!(stdout, "\x1b[32m✓\x1b[0m Wrote {}", path.display());
    if opts.federate {
        let _ = writeln!(stdout, "\x1b[36mfederation token\x1b[0m: {}", federation_token.unwrap_or_default());
    }
    CliOutput { code: 0, stdout, stderr: init_port_token_warning(opts) }
}

fn init_port_token_warning(opts: &InitNativeOptions) -> String {
    if opts.token.is_none() && std::env::var_os("CLAUDE_CODE_OAUTH_TOKEN").is_none() {
        "\x1b[90mwarning\x1b[0m: no --token and no CLAUDE_CODE_OAUTH_TOKEN env — Claude agents will need credentials before wake\n".to_owned()
    } else { String::new() }
}

fn init_write_json_atomic(path: &std::path::Path, value: &serde_json::Value) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| format!("config path has no parent: {}", path.display()))?;
    std::fs::create_dir_all(parent).map_err(|error| format!("init: create {}: {error}", parent.display()))?;
    let tmp = path.with_extension(format!("json.tmp.{}", std::process::id()));
    let body = serde_json::to_string_pretty(value).map_err(|error| format!("init: render config: {error}"))? + "\n";
    std::fs::write(&tmp, body).map_err(|error| format!("init: write {}: {error}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|error| format!("init: rename {}: {error}", path.display()))
}

fn init_backup_config(path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let backup = path.with_extension(format!("json.bak.{}", now_iso_utc()));
    std::fs::copy(path, &backup).map_err(|error| format!("init: backup {}: {error}", path.display()))?;
    Ok(backup)
}

fn init_validate_node_name(name: &str) -> Result<(), String> {
    init_validate_name_re(name, 63, "Node name must be 1-63 chars, letters/digits/hyphens only")
}
fn init_validate_peer_name(name: &str) -> Result<(), String> {
    init_validate_name_re(name, 31, "Name must be 1-31 chars, letters/digits/hyphens only")
}
fn init_validate_name_re(name: &str, max: usize, message: &str) -> Result<(), String> {
    let valid = !name.is_empty() && name.len() <= max && name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-');
    if valid { Ok(()) } else { Err(message.to_owned()) }
}
fn init_validate_peer_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://")) { return Err("URL must start with http:// or https://".to_owned()); }
    Ok(())
}
fn init_expand_home(value: &str) -> String {
    value.strip_prefix("~/").and_then(|tail| std::env::var_os("HOME").map(|home| std::path::PathBuf::from(home).join(tail).display().to_string())).unwrap_or_else(|| value.to_owned())
}
fn init_generate_federation_token() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().fold(String::with_capacity(64), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}
fn init_port_usage() -> String {
    "maw init [--non-interactive --node <name> --token <t> --federate --peer <url> --peer-name <name> --federation-token <hex> --force]\n\nInteractive 3-question wizard. Writes ~/.config/maw/maw.config.json.\n".to_owned()
}
fn init_port_error(message: &str) -> CliOutput { CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") } }

