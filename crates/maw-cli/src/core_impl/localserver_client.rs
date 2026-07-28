// Talking to this node's own maw serve over HTTP.
//
// `maw health`, `maw messages` and `maw reply` are thin clients against the
// local server rather than direct state readers, so they see exactly what a peer
// would see. Includes the percent-encoding, because a target with a colon or a
// slash in it must survive the round trip intact.

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

async fn run_reply_async_impl(raw_args: &[String]) -> CliOutput {
    if raw_args.first().is_some_and(|arg| arg == "--list" || arg == "-l") {
        let mut path = "/api/requests?status=delivered".to_owned();
        if let Some(oracle) = raw_args.get(1) {
            path.push_str("&oracle=");
            path.push_str(&percent_encode_query(oracle));
        }
        return match localserver_request(LocalserverCliRequest { method: "GET".to_owned(), path, body: None }).await {
            Ok(resp) if resp.status < 400 => CliOutput { code: 0, stdout: format_reply_list(&resp.body), stderr: String::new() },
            Ok(resp) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: local maw server returned HTTP {}: {}\n", resp.status, resp.body) },
            Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: {message}\n") },
        };
    }
    if raw_args.len() < 2 {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: "usage: maw-rs reply <correlationId> <message>\n       maw-rs reply --list [oracle]\n".to_owned(),
        };
    }
    let correlation_id = &raw_args[0];
    let reply = raw_args[1..].join(" ");
    let body = serde_json::json!({ "reply": reply }).to_string();
    let path = format!("/api/reply/{}", percent_encode_path(correlation_id));
    match localserver_request(LocalserverCliRequest { method: "POST".to_owned(), path, body: Some(body) }).await {
        Ok(resp) if resp.status < 400 => CliOutput { code: 0, stdout: format!("\u{1b}[32mreplied\u{1b}[0m → {correlation_id}\n"), stderr: String::new() },
        Ok(resp) if resp.body.contains("already replied") => CliOutput { code: 0, stdout: String::new(), stderr: format!("\u{1b}[33mwarn\u{1b}[0m: request '{correlation_id}' already replied\n") },
        Ok(resp) if resp.body.contains("request not found") => CliOutput { code: 1, stdout: String::new(), stderr: format!("\u{1b}[31merror\u{1b}[0m: request '{correlation_id}' not found\n") },
        Ok(resp) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: local maw server returned HTTP {}: {}\n", resp.status, resp.body) },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("reply: {message}\n") },
    }
}

async fn localserver_request(request: LocalserverCliRequest) -> Result<maw_transport::HttpResponse, String> {
    let base = resolve_localserver_base_url();
    let url = format!("{}{}", base.trim_end_matches('/'), request.path);
    let client = ReqwestHttpTransportIo::new(5_000)?;
    client.request(&TransportHttpRequest {
        method: request.method,
        url,
        headers: BTreeMap::new(),
        body: request.body,
        timeout_ms: Some(5_000),
        follow_redirects: false,
        pinned_addr: None,
        max_response_bytes: None,
    }).await
}

fn resolve_localserver_base_url() -> String {
    if let Ok(url) = std::env::var("MAW_LOCALSERVER_URL").or_else(|_| std::env::var("MAW_ENGINE_URL")) {
        return url.trim_end_matches('/').to_owned();
    }
    let port = load_hey_config_port().unwrap_or_else(|| std::env::var("MAW_PORT").ok().and_then(|value| value.parse::<u16>().ok()).unwrap_or(31_745));
    format!("http://127.0.0.1:{port}")
}

fn localserver_port_label() -> String {
    resolve_localserver_base_url().rsplit(':').next().unwrap_or("?").to_owned()
}

fn load_hey_config_port() -> Option<u16> {
    let env = real_xdg_env();
    let value = merged_config_value_for_env(&env);
    value.get("port").and_then(|port| port.as_u64().and_then(|n| u16::try_from(n).ok()).or_else(|| port.as_str()?.parse::<u16>().ok()))
}

fn ensure_trailing_newline(mut value: String) -> String {
    if !value.ends_with('\n') {
        value.push('\n');
    }
    value
}

fn percent_encode_query(value: &str) -> String {
    percent_encode(value, false)
}

fn percent_encode_path(value: &str) -> String {
    percent_encode(value, true)
}

fn percent_encode(value: &str, slash: bool) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        let ok = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') || (slash && byte == b'/');
        if ok {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

fn format_reply_list(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
        return ensure_trailing_newline(body.to_owned());
    };
    let Some(requests) = value.get("requests").and_then(serde_json::Value::as_array) else {
        return ensure_trailing_newline(body.to_owned());
    };
    if requests.is_empty() {
        return "no pending requests\n".to_owned();
    }
    let mut lines = Vec::new();
    for request in requests {
        let id = request.get("correlationId").and_then(serde_json::Value::as_str).unwrap_or("?");
        let from = request.get("from").and_then(serde_json::Value::as_str).unwrap_or("?");
        let message = request.get("message").and_then(serde_json::Value::as_str).unwrap_or("");
        lines.push(format!("  \u{1b}[36m{id}\u{1b}[0m from \u{1b}[33m{from}\u{1b}[0m → {message}"));
    }
    let total = value.get("total").and_then(serde_json::Value::as_u64).unwrap_or(requests.len() as u64);
    lines.push(String::new());
    lines.push(format!("{total} pending request(s)"));
    ensure_trailing_newline(lines.join("\n"))
}
