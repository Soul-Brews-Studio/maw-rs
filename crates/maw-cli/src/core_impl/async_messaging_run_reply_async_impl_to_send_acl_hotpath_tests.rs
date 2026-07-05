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
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
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

#[cfg(test)]
mod send_acl_hotpath_tests {
    include!("async_messaging_send_acl_hotpath_tests/01_send_acl_env_guard_to_inbox_hey_send_args_k_6b8a79.rs");
    include!("async_messaging_send_acl_hotpath_tests/02_send_acl_notify_cross_ddc387_to_send_acl_vec.rs");
}
