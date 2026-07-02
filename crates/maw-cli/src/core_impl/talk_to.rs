use crate::serve_core::ServecoreThreadStore;

const DISPATCH_85: &[DispatcherEntry] = &[
    DispatcherEntry { command: "talk-to", handler: Handler::Async(run_talkto_async) },
    DispatcherEntry { command: "talkto", handler: Handler::Async(run_talkto_async) },
    DispatcherEntry { command: "talk", handler: Handler::Async(run_talkto_async) },
];

const TALKTO_USAGE: &str = "usage: maw talk-to <agent> <message> [--force]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct TalktoArgs {
    recipient: String,
    message: String,
    force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TalktoThreadResult {
    id: u64,
    count: Option<usize>,
}

fn run_talkto_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { talkto_run_async_impl(&args).await })
}

async fn talkto_run_async_impl(raw_args: &[String]) -> CliOutput {
    let args = match talkto_parse_args(raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return talkto_usage_error(&message),
    };
    let config = load_hey_config();
    let thread = talkto_persist_thread(&args.recipient, &args.message);
    let notification = talkto_notification(&args, thread.as_ref());
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(&args.recipient, &config.route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => {
            talkto_local(&mut tmux, &target, &args, &notification, thread.as_ref())
        }
        RouteResult::Peer { peer_url, target, node } => {
            talkto_peer(&peer_url, &target, Some(node.as_str()), &args, &notification, &config, thread.as_ref()).await
        }
        RouteResult::Error { detail, hint, .. } => talkto_route_error(&detail, hint.as_deref(), thread.as_ref()),
    }
}

fn talkto_parse_args(argv: &[String]) -> Result<TalktoArgs, String> {
    if argv.first().is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h" | "-help")) {
        return Err(String::new());
    }
    let mut parsed = TalktoArgs::default();
    let mut positional = Vec::<String>::new();
    for arg in argv {
        match arg.as_str() {
            "--" => return Err("talk-to: -- separator is not supported".to_owned()),
            "--force" => parsed.force = true,
            value if value.starts_with('-') => return Err(format!("talk-to: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
    }
    talkto_finish_args(parsed, &positional)
}

fn talkto_finish_args(mut parsed: TalktoArgs, positional: &[String]) -> Result<TalktoArgs, String> {
    if positional.is_empty() { return Err("talk-to: target and message are required".to_owned()); }
    if positional.len() < 2 { return Err(format!("talk-to: message is required for '{}'", positional[0])); }
    parsed.recipient = talkto_validate_recipient(&positional[0])?;
    parsed.message = talkto_validate_message(&positional[1..].join(" "))?;
    Ok(parsed)
}

fn talkto_validate_recipient(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') || value.contains("..") || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("talk-to: invalid recipient {value:?}"));
    }
    Ok(value.to_owned())
}

fn talkto_validate_message(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.bytes().any(|byte| byte == 0) { return Err("talk-to: message cannot be empty".to_owned()); }
    if value.bytes().any(|byte| matches!(byte, 0x01..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)) { return Err("talk-to: message contains control characters".to_owned()); }
    Ok(value.to_owned())
}

fn talkto_persist_thread(recipient: &str, message: &str) -> Option<TalktoThreadResult> {
    if std::env::var_os("MAW_RS_TALKTO_NO_THREAD").is_some() { return None; }
    let store = ServecoreThreadStore::servecore_default();
    talkto_persist_thread_with_store(&store, recipient, message)
}

fn talkto_persist_thread_with_store(
    store: &ServecoreThreadStore,
    recipient: &str,
    message: &str,
) -> Option<TalktoThreadResult> {
    let title = format!("channel:{recipient}");
    let Ok((result, record)) = store.servecore_post_channel(&title, "claude", message) else {
        return None;
    };
    Some(TalktoThreadResult {
        id: result.thread_id,
        count: Some(record.messages.len()),
    })
}

fn talkto_notification(args: &TalktoArgs, thread: Option<&TalktoThreadResult>) -> String {
    let from = std::env::var("CLAUDE_AGENT_NAME").unwrap_or_else(|_| "cli".to_owned());
    if let Some(thread) = thread {
        return format!(
            "💬 channel:{} (#{}) — {} msgs\nFrom: {from}\nMessage:\n{}\n→ Full copy saved in thread #{}",
            args.recipient,
            thread.id,
            thread.count.map_or_else(|| "?".to_owned(), |count| count.to_string()),
            args.message,
            thread.id
        );
    }
    format!("💬 from {from}\nMessage:\n{}", args.message)
}

fn talkto_local(
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
    target: &str,
    args: &TalktoArgs,
    notification: &str,
    thread: Option<&TalktoThreadResult>,
) -> CliOutput {
    if let Err(message) = talkto_validate_tmux_target(target) { return talkto_saved_or_error(&message, thread); }
    let pane = tmux.first_pane_id(target).unwrap_or_else(|| target.to_owned());
    if let Err(message) = talkto_validate_tmux_target(&pane) { return talkto_saved_or_error(&message, thread); }
    if !args.force {
        let command = match tmux.get_pane_command(&pane) {
            Ok(command) => command,
            Err(error) => return talkto_saved_or_error(&format!("tmux inspect failed: {error}"), thread),
        };
        if !talkto_is_agent_command(&command) {
            return talkto_saved_or_error(&format!("no active Claude session in {pane} (use --force)"), thread);
        }
    }
    if let Err(error) = tmux.send_keys_literal(&pane, notification) { return talkto_send_error(&format!("tmux send-keys failed: {error}"), thread); }
    if let Err(error) = tmux.send_enter(&pane) { return talkto_send_error(&format!("tmux send-enter failed: {error}"), thread); }
    let _ = talkto_append_log(&args.recipient, &pane, &args.message, thread);
    CliOutput { code: 0, stdout: format!("✓ thread #{} + sent → {pane}\n", thread.map_or("?".to_owned(), |item| item.id.to_string())), stderr: talkto_thread_stub_warning(thread) }
}

async fn talkto_peer(
    peer_url: &str,
    target: &str,
    node: Option<&str>,
    args: &TalktoArgs,
    notification: &str,
    config: &HeyConfig,
    thread: Option<&TalktoThreadResult>,
) -> CliOutput {
    if let Err(message) = talkto_validate_transport_target(target) { return talkto_saved_or_error(&message, thread); }
    let send_args = SendArgs { target: target.to_owned(), text: notification.to_owned(), inbox: None, from: None, approve: false, trust: false };
    let mut output = match send_acl_gate_peer("talk-to", target, &send_args, config, false) {
        SendAclGateResult::Proceed { stderr_prefix } => {
            if let Some(output) = talkto_fake_peer(peer_url, target, node, args, notification, thread) {
                send_acl_apply_proceed_stderr(output, &stderr_prefix)
            } else {
                send_acl_deliver_peer_message("talk-to", peer_url, target, &send_args, config, stderr_prefix).await
            }
        }
        SendAclGateResult::Queued(output) | SendAclGateResult::Reject(output) => return output,
    };
    if output.code == 0 {
        output.stdout = format!("✓ thread #{} + sent → {}:{}\n", thread.map_or("?".to_owned(), |item| item.id.to_string()), node.unwrap_or("peer"), target);
        output.stderr.push_str(&talkto_thread_stub_warning(thread));
    } else if thread.is_some() {
        output.code = 0;
        output.stdout = format!("✓ thread #{} updated\n", thread.expect("checked").id);
        output.stderr.push_str("warn: remote send failed — message saved to thread only\n");
    }
    output
}

fn talkto_fake_peer(
    peer_url: &str,
    target: &str,
    node: Option<&str>,
    args: &TalktoArgs,
    notification: &str,
    thread: Option<&TalktoThreadResult>,
) -> Option<CliOutput> {
    let path = std::env::var_os("MAW_RS_TALKTO_FAKE_PEER_LOG")?;
    let row = serde_json::json!({
        "peerUrl": peer_url,
        "target": target,
        "node": node,
        "force": args.force,
        "text": notification,
    });
    let result = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut file| { use std::io::Write as _; writeln!(file, "{row}") });
    if let Err(error) = result {
        return Some(CliOutput { code: 1, stdout: String::new(), stderr: format!("talk-to: fake peer transport failed: {error}\n") });
    }
    Some(CliOutput {
        code: 0,
        stdout: format!("✓ thread #{} + sent → {}:{}\n", thread.map_or("?".to_owned(), |item| item.id.to_string()), node.unwrap_or("peer"), target),
        stderr: talkto_thread_stub_warning(thread),
    })
}

fn talkto_validate_transport_target(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("invalid transport target {value:?}"));
    }
    Ok(())
}

fn talkto_route_error(detail: &str, hint: Option<&str>, thread: Option<&TalktoThreadResult>) -> CliOutput {
    let reason = hint.map_or_else(|| detail.to_owned(), |hint| format!("{detail}; {hint}"));
    talkto_saved_or_error(&reason, thread)
}

fn talkto_saved_or_error(reason: &str, thread: Option<&TalktoThreadResult>) -> CliOutput {
    if let Some(thread) = thread {
        return CliOutput { code: 0, stdout: format!("✓ thread #{} updated\n", thread.id), stderr: format!("warn: {reason} — message saved to thread only\n") };
    }
    CliOutput { code: 1, stdout: String::new(), stderr: format!("talk-to: {reason}\n") }
}

fn talkto_send_error(reason: &str, thread: Option<&TalktoThreadResult>) -> CliOutput {
    if let Some(thread) = thread {
        return CliOutput { code: 0, stdout: format!("✓ thread #{} updated\n", thread.id), stderr: format!("warn: {reason} — message saved to thread only\n") };
    }
    CliOutput { code: 1, stdout: String::new(), stderr: format!("talk-to: {reason}\n") }
}

fn talkto_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.contains('\0') || value.bytes().any(|byte| matches!(byte, 0x01..=0x1f | 0x7f)) {
        return Err(format!("invalid tmux target {value:?}"));
    }
    Ok(())
}

fn talkto_is_agent_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    lower.contains("claude") || lower.contains("codex") || lower.contains("node")
}

fn talkto_append_log(to: &str, target: &str, message: &str, thread: Option<&TalktoThreadResult>) -> Result<(), String> {
    let env = real_xdg_env();
    let path = maw_state_path(&env, &["maw-log.jsonl"]);
    if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|error| error.to_string())?; }
    let row = serde_json::json!({
        "ts": talkto_now_iso(),
        "from": std::env::var("CLAUDE_AGENT_NAME").unwrap_or_else(|_| "cli".to_owned()),
        "to": to,
        "target": target,
        "msg": message,
        "host": std::env::var("HOSTNAME").unwrap_or_else(|_| "unknown".to_owned()),
        "sid": std::env::var("CLAUDE_SESSION_ID").ok(),
        "ch": thread.map(|item| format!("thread:{}", item.id)),
    });
    std::fs::OpenOptions::new().create(true).append(true).open(path).and_then(|mut file| { use std::io::Write as _; writeln!(file, "{row}") }).map_err(|error| error.to_string())
}

fn talkto_now_iso() -> String {
    let ms = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_millis());
    format!("epoch-ms:{ms}")
}

fn talkto_thread_stub_warning(_thread: Option<&TalktoThreadResult>) -> String { String::new() }

fn talkto_usage_error(message: &str) -> CliOutput {
    let detail = if message.is_empty() { String::new() } else { format!("{message}\n") };
    CliOutput { code: 2, stdout: String::new(), stderr: format!("{detail}{TALKTO_USAGE}\n") }
}

#[cfg(test)]
mod talkto_tests {
    use super::*;

    fn talkto_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn talkto_parser_accepts_force_and_message_words() {
        let args = talkto_parse_args(&talkto_strings(&["alpha", "hello", "there", "--force"])).unwrap();
        assert_eq!(args.recipient, "alpha");
        assert_eq!(args.message, "hello there");
        assert!(args.force);
    }

    #[test]
    fn talkto_parser_rejects_option_injection() {
        assert!(talkto_parse_args(&talkto_strings(&["--", "alpha", "msg"])).unwrap_err().contains("separator"));
        assert!(talkto_parse_args(&talkto_strings(&["-alpha", "msg"])).unwrap_err().contains("unknown argument"));
        assert!(talkto_parse_args(&talkto_strings(&["alpha/../../x", "msg"])).unwrap_err().contains("invalid recipient"));
        assert!(talkto_parse_args(&talkto_strings(&["alpha"])).unwrap_err().contains("message is required"));
    }

    #[test]
    fn talkto_consumer_persists_real_thread_store() {
        let mut root = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        root.push(format!("maw-rs-talkto-thread-{}-{nanos}", std::process::id()));
        root.push("consumer");
        let store = ServecoreThreadStore::servecore_with_root(root);
        let first = talkto_persist_thread_with_store(&store, "alpha", "hello").expect("first");
        let second = talkto_persist_thread_with_store(&store, "alpha", "again").expect("second");
        assert_eq!(first.id, second.id);
        assert_eq!(first.count, Some(1));
        assert_eq!(second.count, Some(2));
        let record = store.read(first.id).expect("read");
        assert_eq!(record.thread.title, "channel:alpha");
        assert_eq!(record.messages[0].content, "hello");
        assert_eq!(record.messages[1].content, "again");
        assert_eq!(talkto_thread_stub_warning(Some(&second)), "");
    }
}
