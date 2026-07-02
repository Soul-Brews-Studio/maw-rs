use futures_util::{SinkExt as _, StreamExt as _};

const DISPATCH_108: &[DispatcherEntry] = &[
    DispatcherEntry { command: "follow", handler: Handler::Sync(follow_run_command) },
];

const FOLLOW_USAGE: &str = "usage: maw follow <pane> [--since=<dur>] [--json] [--grep <pattern>] [--quit-on-idle=<dur>]";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct FollowOptions {
    target: String,
    since: Option<String>,
    json: bool,
    grep: Option<String>,
    quit_on_idle: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FollowResult {
    pane: String,
    reason: String,
    chunks: usize,
}

fn follow_run_command(argv: &[String]) -> CliOutput {
    let options = match follow_parse_cli(argv) {
        Ok(options) => options,
        Err(message) => return follow_error(&message),
    };
    match follow_cmd(&options) {
        Ok(output) => CliOutput { code: 0, stdout: output.0, stderr: output.1 },
        Err(message) => follow_error(&message),
    }
}


fn follow_error(message: &str) -> CliOutput {
    let code = if message == FOLLOW_USAGE { 2 } else { 1 };
    CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") }
}

fn follow_parse_cli(argv: &[String]) -> Result<FollowOptions, String> {
    let mut options = FollowOptions::default();
    let mut index = 0;
    while index < argv.len() {
        let token = &argv[index];
        if token == "--" {
            index += 1;
            while index < argv.len() {
                follow_set_target(&mut options, &argv[index])?;
                index += 1;
            }
            break;
        }
        match token.as_str() {
            "--help" | "-h" => return Err(FOLLOW_USAGE.to_owned()),
            "--json" => index += 1,
            "--since" => {
                options.since = Some(follow_take_value(argv, &mut index, "--since")?);
            }
            "--grep" => {
                options.grep = Some(follow_take_value(argv, &mut index, "--grep")?);
            }
            "--quit-on-idle" => {
                options.quit_on_idle = Some(follow_take_value(argv, &mut index, "--quit-on-idle")?);
            }
            _ if token.starts_with("--since=") => {
                options.since = Some(follow_inline_value(token, "--since=")?);
                index += 1;
            }
            _ if token.starts_with("--grep=") => {
                options.grep = Some(follow_inline_value(token, "--grep=")?);
                index += 1;
            }
            _ if token.starts_with("--quit-on-idle=") => {
                options.quit_on_idle = Some(follow_inline_value(token, "--quit-on-idle=")?);
                index += 1;
            }
            _ if token.starts_with('-') => return Err(FOLLOW_USAGE.to_owned()),
            _ => {
                follow_set_target(&mut options, token)?;
                index += 1;
            }
        }
        if token == "--json" {
            options.json = true;
        }
    }
    if options.target.is_empty() {
        return Err(FOLLOW_USAGE.to_owned());
    }
    follow_validate_target(&options.target)?;
    follow_validate_options(&options)?;
    Ok(options)
}

fn follow_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    let Some(value) = argv.get(*index) else { return Err(FOLLOW_USAGE.to_owned()); };
    let value = follow_validate_value(flag, value)?;
    *index += 1;
    Ok(value)
}

fn follow_inline_value(token: &str, prefix: &str) -> Result<String, String> {
    follow_validate_value(prefix.trim_end_matches('='), &token[prefix.len()..])
}

fn follow_set_target(options: &mut FollowOptions, target: &str) -> Result<(), String> {
    if !options.target.is_empty() {
        return Err(FOLLOW_USAGE.to_owned());
    }
    options.target = follow_validate_value("target", target)?;
    Ok(())
}

fn follow_validate_value(label: &str, value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("follow: invalid {label} value"));
    }
    Ok(value.to_owned())
}

fn follow_validate_target(target: &str) -> Result<(), String> {
    if !target.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '%' | '-')) {
        return Err("follow: tmux target contains unsupported characters".to_owned());
    }
    if target.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("follow: bare numeric tmux targets are refused; use session:window or %pane_id".to_owned());
    }
    Ok(())
}

fn follow_validate_options(options: &FollowOptions) -> Result<(), String> {
    if let Some(since) = &options.since {
        if follow_parse_duration_ms(since).is_none() {
            return Err(format!("follow: invalid --since duration: {since}"));
        }
    }
    if let Some(idle) = &options.quit_on_idle {
        if follow_parse_duration_ms(idle).as_ref().is_none_or(|ms| *ms == 0) {
            return Err(format!("follow: invalid --quit-on-idle duration: {idle}"));
        }
    }
    if let Some(pattern) = &options.grep {
        follow_compile_grep(pattern)?;
    }
    Ok(())
}

fn follow_cmd(options: &FollowOptions) -> Result<(String, String), String> {
    let pane = follow_resolve_target(&options.target)?;
    if let Ok(fake) = std::env::var("MAW_RS_FOLLOW_FAKE_STREAM") {
        return follow_render_fake_stream(&pane, options, &fake);
    }
    let url = follow_url_from_config()?;
    let result = follow_runtime_connect(&pane, options, &url)?;
    Ok((String::new(), follow_result_stderr(&result)))
}

fn follow_resolve_target(target: &str) -> Result<String, String> {
    follow_validate_target(target)?;
    if target.contains(':') || target.starts_with('%') {
        return Ok(target.to_owned());
    }
    let sessions = TmuxClient::local().list_all();
    let matches = sessions.iter().filter(|session| session.name == target).collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Err(format!("follow: session '{target}' not found")),
        [session] => Ok(session.windows.first().map_or_else(|| session.name.clone(), |window| format!("{}:{}", session.name, window.name))),
        _ => Err(format!("follow: '{target}' is ambiguous")),
    }
}

fn follow_render_fake_stream(pane: &str, options: &FollowOptions, fake: &str) -> Result<(String, String), String> {
    let grep = options.grep.as_deref().map(follow_compile_grep).transpose()?;
    let mut stdout = String::new();
    let mut chunks = 0usize;
    for chunk in follow_fake_chunks(fake) {
        if !follow_chunk_matches(grep.as_ref(), &chunk) {
            continue;
        }
        chunks = chunks.saturating_add(1);
        follow_push_chunk(&mut stdout, pane, options.json, &chunk);
    }
    let result = FollowResult { pane: pane.to_owned(), reason: "closed".to_owned(), chunks };
    Ok((stdout, follow_result_stderr(&result)))
}

fn follow_fake_chunks(fake: &str) -> Vec<String> {
    fake.split("\n---chunk---\n").map(ToOwned::to_owned).collect()
}

fn follow_chunk_matches(grep: Option<&String>, chunk: &str) -> bool {
    grep.is_none_or(|needle| chunk.contains(needle))
}

fn follow_push_chunk(stdout: &mut String, pane: &str, json: bool, chunk: &str) {
    if json {
        let ts = std::env::var("MAW_RS_FOLLOW_FAKE_NOW").unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned());
        let _ = writeln!(stdout, "{{\"ts\":{},\"pane\":{},\"chunk\":{}}}", json_string(&ts), json_string(pane), json_string(chunk));
    } else {
        stdout.push_str(chunk);
    }
}

fn follow_result_stderr(result: &FollowResult) -> String {
    if std::env::var_os("MAW_RS_FOLLOW_SUMMARY").is_some() {
        format!("follow: {} ({}, {} chunks)\n", result.pane, result.reason, result.chunks)
    } else {
        String::new()
    }
}

fn follow_parse_duration_ms(raw: &str) -> Option<u64> {
    let input = raw.trim();
    if input.is_empty() || input != raw {
        return None;
    }
    if input.chars().all(|ch| ch.is_ascii_digit() || ch == '.') {
        return follow_float_ms(input, 1_000.0);
    }
    follow_parse_compound_duration(input)
}

fn follow_parse_compound_duration(input: &str) -> Option<u64> {
    let mut total = 0u64;
    let mut cursor = 0usize;
    while cursor < input.len() {
        let start = cursor;
        while cursor < input.len() && (input.as_bytes()[cursor].is_ascii_digit() || input.as_bytes()[cursor] == b'.') {
            cursor += 1;
        }
        if cursor == start {
            return None;
        }
        let unit_start = cursor;
        while cursor < input.len() && input.as_bytes()[cursor].is_ascii_alphabetic() {
            cursor += 1;
        }
        total = total.checked_add(follow_duration_piece(&input[start..unit_start], &input[unit_start..cursor])?)?;
    }
    Some(total)
}

fn follow_duration_piece(number: &str, unit: &str) -> Option<u64> {
    let multiplier = match unit {
        "ms" => 1.0,
        "s" => 1_000.0,
        "m" => 60_000.0,
        "h" => 3_600_000.0,
        "d" => 86_400_000.0,
        _ => return None,
    };
    follow_float_ms(number, multiplier)
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn follow_float_ms(number: &str, multiplier: f64) -> Option<u64> {
    let value = number.parse::<f64>().ok()?;
    if !value.is_finite() || value < 0.0 {
        return None;
    }
    let millis = (value * multiplier).round();
    if millis > u64::MAX as f64 {
        return None;
    }
    Some(millis as u64)
}

fn follow_replay_lines_for_duration(ms: u64) -> u64 {
    ms.div_ceil(1_000).clamp(1, 10_000)
}

fn follow_url_from_config() -> Result<String, String> {
    if let Ok(explicit) = std::env::var("MAW_ENGINE_URL") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return follow_ws_url_from_engine(trimmed);
        }
    }
    let port = std::env::var("MAW_PORT").ok().filter(|value| !value.trim().is_empty()).unwrap_or_else(|| "3456".to_owned());
    Ok(format!("ws://127.0.0.1:{port}/ws/pty"))
}

fn follow_ws_url_from_engine(raw: &str) -> Result<String, String> {
    let mut url = raw.parse::<axum::http::Uri>().map_err(|error| format!("follow: invalid MAW_ENGINE_URL: {error}"))?.to_string();
    if url.starts_with("https://") {
        url.replace_range(0..8, "wss://");
    } else if url.starts_with("http://") {
        url.replace_range(0..7, "ws://");
    }
    let Some((base, _)) = url.split_once('?') else {
        return Ok(format!("{}/ws/pty", url.trim_end_matches('/')));
    };
    Ok(format!("{}/ws/pty", base.trim_end_matches('/')))
}

fn follow_runtime_connect(pane: &str, options: &FollowOptions, url: &str) -> Result<FollowResult, String> {
    let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|error| format!("follow: runtime: {error}"))?;
    runtime.block_on(follow_connect_async(pane, options, url))
}

async fn follow_connect_async(pane: &str, options: &FollowOptions, url: &str) -> Result<FollowResult, String> {
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.map_err(|error| format!("follow: websocket error: {url}: {error}"))?;
    let replay_lines = options.since.as_deref().and_then(follow_parse_duration_ms).map_or(0, follow_replay_lines_for_duration);
    let attach = serde_json::json!({"type":"attach","target":pane,"cols":120,"rows":40,"replayLines":replay_lines}).to_string();
    ws.send(tokio_tungstenite::tungstenite::Message::Text(attach)).await.map_err(|error| format!("follow: attach send failed: {error}"))?;
    follow_read_ws(pane, options, ws).await
}

async fn follow_read_ws(
    pane: &str,
    options: &FollowOptions,
    mut ws: tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> Result<FollowResult, String> {
    let grep = options.grep.as_deref().map(follow_compile_grep).transpose()?;
    let mut chunks = 0usize;
    while let Some(message) = ws.next().await {
        let message = message.map_err(|error| format!("follow: websocket read failed: {error}"))?;
        let text = follow_message_text(message)?;
        if let Some(reason) = follow_control_reason(&text)? {
            return Ok(FollowResult { pane: pane.to_owned(), reason, chunks });
        }
        if follow_chunk_matches(grep.as_ref(), &text) {
            chunks = chunks.saturating_add(1);
            if options.json {
                println!("{{\"ts\":{},\"pane\":{},\"chunk\":{}}}", json_string(&follow_event_timestamp()), json_string(pane), json_string(&text));
            } else {
                print!("{text}");
            }
        }
    }
    Ok(FollowResult { pane: pane.to_owned(), reason: "closed".to_owned(), chunks })
}

fn follow_message_text(message: tokio_tungstenite::tungstenite::Message) -> Result<String, String> {
    match message {
        tokio_tungstenite::tungstenite::Message::Text(text) => Ok(text),
        tokio_tungstenite::tungstenite::Message::Binary(bytes) => String::from_utf8(bytes).map_err(|error| format!("follow: binary frame was not utf-8: {error}")),
        tokio_tungstenite::tungstenite::Message::Close(_) | tokio_tungstenite::tungstenite::Message::Ping(_) | tokio_tungstenite::tungstenite::Message::Pong(_) | tokio_tungstenite::tungstenite::Message::Frame(_) => Ok(String::new()),
    }
}

fn follow_control_reason(text: &str) -> Result<Option<String>, String> {
    if !text.starts_with('{') {
        return Ok(None);
    }
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else { return Ok(None); };
    let kind = value.get("type").and_then(serde_json::Value::as_str).unwrap_or("");
    match kind {
        "attached" => Ok(Some("attached".to_owned()).filter(|_| false)),
        "detached" => Ok(Some("detached".to_owned())),
        "error" => Err(value.get("message").and_then(serde_json::Value::as_str).unwrap_or("PTY follow error").to_owned()),
        _ => Ok(None),
    }
}

fn follow_compile_grep(pattern: &str) -> Result<String, String> {
    if pattern.is_empty() || pattern.starts_with('-') || pattern.chars().any(char::is_control) {
        return Err("follow: invalid --grep pattern".to_owned());
    }
    Ok(pattern.to_owned())
}

fn follow_event_timestamp() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("{seconds}")
}
