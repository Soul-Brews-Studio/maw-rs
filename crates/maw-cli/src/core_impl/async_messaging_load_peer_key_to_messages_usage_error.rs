fn load_peer_key() -> Result<String, String> {
    if let Ok(value) = std::env::var("MAW_PEER_KEY") {
        if !value.is_empty() {
            return Ok(value);
        }
    }
    let env = real_xdg_env();
    let path = maw_state_path(&env, &["peer-key"]);
    if let Ok(raw) = std::fs::read_to_string(&path) {
        let key = raw.trim().to_owned();
        if !key.is_empty() {
            return Ok(key);
        }
    }
    let key = generate_peer_key()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create peer-key directory: {error}"))?;
    }
    write_peer_key_file(&path, &key)?;
    Ok(key)
}

fn generate_peer_key() -> Result<String, String> {
    let mut file = std::fs::File::open("/dev/urandom")
        .map_err(|error| format!("failed to open random peer key source: {error}"))?;
    let mut bytes = [0_u8; 32];
    std::io::Read::read_exact(&mut file, &mut bytes)
        .map_err(|error| format!("failed to read random peer key bytes: {error}"))?;
    Ok(hex_bytes(&bytes))
}

fn write_peer_key_file(path: &std::path::Path, key: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .mode(0o600)
            .open(path)
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        std::io::Write::write_all(&mut file, key.as_bytes())
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        std::io::Write::write_all(&mut file, b"\n")
            .map_err(|error| format!("failed to write peer-key: {error}"))?;
        Ok(())
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, format!("{key}\n"))
            .map_err(|error| format!("failed to write peer-key: {error}"))
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn real_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME")
        .map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_DATA_DIR",
        "MAW_STATE_DIR",
        "MAW_CACHE_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
        "XDG_CACHE_HOME",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok().map(|value| (name.to_owned(), value)));
    MawXdgEnv::with_vars(home, vars)
}

#[derive(Debug, Clone, Default)]
struct LocalserverCliRequest {
    method: String,
    path: String,
    body: Option<String>,
}

fn run_health_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_health_async_impl(&args).await })
}

fn run_messages_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_messages_async_impl(&args).await })
}

fn run_reply_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { run_reply_async_impl(&args).await })
}

async fn run_health_async_impl(raw_args: &[String]) -> CliOutput {
    if !raw_args.is_empty() {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: "usage: maw-rs health\n".to_owned(),
        };
    }
    let mut lines = vec!["\nmaw health\n".to_owned()];
    let sessions = TmuxClient::local().list_all();
    lines.push(format!(
        "  \u{1b}[32m●\u{1b}[0m tmux server        running ({} sessions)",
        sessions.len()
    ));
    match localserver_request(LocalserverCliRequest {
        method: "POST".to_owned(),
        path: "/api/probe".to_owned(),
        body: Some("{}".to_owned()),
    })
    .await
    {
        Ok(resp) if resp.status < 400 => lines.push(format!(
            "  \u{1b}[32m●\u{1b}[0m maw server         online (:{}, probe ok)",
            localserver_port_label()
        )),
        Ok(resp) => lines.push(format!(
            "  \u{1b}[33m●\u{1b}[0m maw server         HTTP {} (probe)",
            resp.status
        )),
        Err(_) => lines.push("  \u{1b}[31m●\u{1b}[0m maw server         offline".to_owned()),
    }
    lines.push(String::new());
    CliOutput {
        code: 0,
        stdout: format!("{}\n", lines.join("\n")),
        stderr: String::new(),
    }
}

async fn run_messages_async_impl(raw_args: &[String]) -> CliOutput {
    if let Some(output) = messages_lifecycle_subcommand152(raw_args) { return output; }
    let mut path = "/api/message-ledger".to_owned();
    let mut passthrough = Vec::<String>::new();
    let mut index = 0;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--limit" | "--from" | "--to" | "--direction" | "--state" | "--q" => {
                let Some(value) = raw_args.get(index + 1) else {
                    return messages_usage_error(&format!("messages: missing {} value", raw_args[index]));
                };
                passthrough.push(format!("{}={}", raw_args[index].trim_start_matches("--"), percent_encode_query(value)));
                index += 1;
            }
            "--json" => passthrough.push("json=1".to_owned()),
            value if value.starts_with('-') => return messages_usage_error(&format!("messages: unknown argument {value}")),
            value => return messages_usage_error(&format!("messages: unexpected argument {value}")),
        }
        index += 1;
    }
    if !passthrough.is_empty() {
        path.push('?');
        path.push_str(&passthrough.join("&"));
    }
    match localserver_request(LocalserverCliRequest {
        method: "GET".to_owned(),
        path,
        body: None,
    })
    .await
    {
        Ok(resp) if resp.status < 400 => CliOutput { code: 0, stdout: ensure_trailing_newline(resp.body), stderr: String::new() },
        Ok(resp) => CliOutput { code: 1, stdout: String::new(), stderr: format!("messages: local maw server returned HTTP {}: {}\n", resp.status, resp.body) },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("messages: {message}\n") },
    }
}

fn messages_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs messages [serve [--detach] [--engine URL] [--port N] | status [--engine URL] | stop [--engine URL] | --limit N --from ID --to ID --direction outbound|inbound|forwarded --state queued|delivered|failed --q text --json]\n"),
    }
}

