const DISPATCH_83: &[DispatcherEntry] = &[DispatcherEntry { command: "notify", handler: Handler::Async(run_notify_async) }];

const NOTIFY_USAGE: &str = "usage: maw notify [--from <node:oracle>] <target> <message> [--approve] [--trust]";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct NotifyArgs { target: String, text: String, from: Option<String>, approve: bool, trust: bool, force: bool }

fn run_notify_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { notify_run_async_impl(&args).await })
}

async fn notify_run_async_impl(raw_args: &[String]) -> CliOutput {
    let args = match notify_parse_args(raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return notify_usage_error(&message),
    };
    let config = load_hey_config();
    let sender_oracle = resolve_hey_sender_oracle(&config);
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(&args.target, &config.route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => {
            notify_local(&args, &target, &config, &sender_oracle)
        }
        RouteResult::Peer { peer_url, target, node: _ } => {
            notify_peer(&peer_url, &target, &args, &config, &sender_oracle).await
        }
        RouteResult::Error { detail, hint, .. } => notify_route_error(&detail, hint.as_deref()),
    }
}

fn notify_parse_args(argv: &[String]) -> Result<NotifyArgs, String> {
    if argv.first().is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h" | "-help")) { return Err(String::new()); }
    let mut parsed = NotifyArgs::default();
    let mut positional = Vec::<String>::new();
    let mut index = 0usize;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--" => return Err("notify: -- separator is not supported".to_owned()),
            "--approve" => parsed.approve = true,
            "--trust" => parsed.trust = true,
            "--force" => parsed.force = true,
            "--inbox" => {},
            "--from" => { index += 1; parsed.from = Some(notify_take_from(argv, index)?); },
            value if value.starts_with("--from=") => parsed.from = Some(notify_validate_from(value.trim_start_matches("--from="))?),
            value if value.starts_with('-') => return Err(format!("notify: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    notify_finish_args(parsed, &positional)
}

fn notify_finish_args(mut parsed: NotifyArgs, positional: &[String]) -> Result<NotifyArgs, String> {
    if positional.is_empty() { return Err("missing target and message".to_owned()); }
    if positional.len() < 2 { return Err(format!("missing message for '{}'", positional[0])); }
    parsed.target = notify_validate_target(&positional[0])?;
    parsed.text = notify_validate_message(&positional[1..].join(" "))?;
    Ok(parsed)
}

fn notify_take_from(argv: &[String], index: usize) -> Result<String, String> {
    let Some(value) = argv.get(index) else { return Err("missing value for --from".to_owned()); };
    if value.starts_with('-') { return Err("missing value for --from".to_owned()); }
    notify_validate_from(value)
}

fn notify_validate_target(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') || value.contains("..") || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("notify: invalid target {value:?}"));
    }
    Ok(value.to_owned())
}

fn notify_validate_message(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.bytes().any(|byte| byte == 0) { return Err("notify: message cannot be empty".to_owned()); }
    if value.bytes().any(|byte| matches!(byte, 0x01..=0x08 | 0x0b | 0x0c | 0x0e..=0x1f | 0x7f)) { return Err("notify: message contains control characters".to_owned()); }
    Ok(value.to_owned())
}

fn notify_validate_from(value: &str) -> Result<String, String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.contains('/') || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err("missing value for --from".to_owned());
    }
    Ok(value.to_owned())
}

fn notify_local(
    args: &NotifyArgs,
    resolved_target: &str,
    config: &HeyConfig,
    sender_oracle: &str,
) -> CliOutput {
    notify_local_with(args, resolved_target, config, sender_oracle, &locate_find_oracle_repo_path)
}

/// See [`notify_local`]. The `resolve_repo` seam (receiver oracle name → its repo
/// path) is split out so a test can drive the destination deterministically.
///
/// Delivery must land in the RECEIVER's `ψ/inbox`, resolved from the registry — not
/// the sender's own `inbox_dir`. The old code wrote to `inbox_real_env().inbox_dir`
/// (the sender), so a message addressed to `to` was filed with the sender and reached
/// nobody while reporting success (#672 defect 1). This mirrors the cross-node receive
/// path in `serve.rs` (`repo_path.join("ψ").join("inbox")`).
fn notify_local_with(
    args: &NotifyArgs,
    resolved_target: &str,
    config: &HeyConfig,
    sender_oracle: &str,
    resolve_repo: &dyn Fn(&str) -> Option<String>,
) -> CliOutput {
    let from = notify_display_from(args.from.as_deref(), config, sender_oracle);
    let to = notify_inbox_to(&args.target, resolved_target);
    let Some(repo_path) = resolve_repo(&to) else {
        return notify_route_error(
            &format!("cannot resolve a local inbox for '{to}'"),
            Some("not a known local oracle — check `maw locate <name> --path`"),
        );
    };
    let inbox_dir = std::path::Path::new(&repo_path).join("ψ").join("inbox");
    match inbox_write_file(&inbox_dir, &from, &to, &args.text, inbox_now_ms()) {
        Ok(filename) => CliOutput { code: 0, stdout: notify_success(&to, &filename, args.force), stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("notify: {message}\n") },
    }
}

async fn notify_peer(
    peer_url: &str,
    target: &str,
    args: &NotifyArgs,
    config: &HeyConfig,
    sender_oracle: &str,
) -> CliOutput {
    let send_args = SendArgs {
        target: target.to_owned(),
        text: args.text.clone(),
        inbox: Some(true),
        from: args.from.clone(),
        approve: args.approve,
        trust: args.trust,
        dry_run: false,
    };
    let mut output =
        gated_send_peer_message("notify", peer_url, target, &send_args, config, sender_oracle, false)
            .await;
    if output.code == 0 && args.force { output.stderr.push_str("\x1b[90mnote: --force is not meaningful for notify (delivery is always inbox-only).\x1b[0m\n"); }
    output
}

fn notify_route_error(detail: &str, hint: Option<&str>) -> CliOutput {
    CliOutput { code: 2, stdout: String::new(), stderr: if let Some(hint) = hint { format!("notify: {detail}; {hint}\n") } else { format!("notify: {detail}\n") } }
}

fn notify_display_from(explicit: Option<&str>, config: &HeyConfig, sender_oracle: &str) -> String {
    explicit.map_or_else(
        || {
            if let Ok(sender) = std::env::var("MAW_SENDER") {
                return sender;
            }
            let node = config.node.as_deref().unwrap_or("local");
            format!("{node}:{sender_oracle}")
        },
        ToOwned::to_owned,
    )
}

fn notify_inbox_to(requested: &str, resolved: &str) -> String {
    if requested.starts_with("local:") { requested.trim_start_matches("local:").to_owned() } else { requested.to_owned() }.chars().filter(|ch| !ch.is_control()).collect::<String>().trim_matches('/').trim().to_owned().notify_if_empty_then(resolved)
}

trait NotifyEmptyFallback { fn notify_if_empty_then(self, fallback: &str) -> String; }

impl NotifyEmptyFallback for String {
    fn notify_if_empty_then(self, fallback: &str) -> String { if self.is_empty() { fallback.to_owned() } else { self } }
}

fn notify_success(to: &str, filename: &str, force: bool) -> String {
    let mut out = format!("queued inbox {to} {filename}\n");
    if force { out.push_str("\x1b[90mnote: --force is not meaningful for notify (delivery is always inbox-only).\x1b[0m\n"); }
    out
}

fn notify_usage_error(message: &str) -> CliOutput {
    let detail = if message.is_empty() { String::new() } else { format!("{message}\n") };
    CliOutput { code: 2, stdout: String::new(), stderr: format!("{detail}{NOTIFY_USAGE}\n  Routine push — persists to recipient's ψ/inbox/ for them to pull via `maw inbox --unread`.\n  Does NOT inject into the target pane (unlike `maw hey`). #1882\n") }
}

#[cfg(test)]
mod notify_tests {
    use super::*;

    fn notify_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn notify_parser_accepts_flags_and_joins_message() {
        let args = notify_parse_args(&notify_strings(&["--from", "node:oracle", "--approve", "--trust", "local:nova", "hello", "there"])).unwrap();
        assert_eq!(args.from.as_deref(), Some("node:oracle"));
        assert_eq!(args.target, "local:nova");
        assert_eq!(args.text, "hello there");
        assert!(args.approve);
        assert!(args.trust);
    }

    #[test]
    fn notify_parser_rejects_option_injection_and_missing_message() {
        assert!(notify_parse_args(&notify_strings(&["--", "local:nova", "msg"])).unwrap_err().contains("-- separator"));
        assert!(notify_parse_args(&notify_strings(&["-target", "msg"])).unwrap_err().contains("unknown argument"));
        assert!(notify_parse_args(&notify_strings(&["local:nova"])).unwrap_err().contains("missing message"));
        assert!(notify_parse_args(&notify_strings(&["--from", "--bad", "local:nova", "msg"])).unwrap_err().contains("missing value"));
    }

    fn notify_args(target: &str, text: &str) -> NotifyArgs {
        NotifyArgs {
            target: target.to_owned(),
            text: text.to_owned(),
            from: Some("m5:ui".to_owned()),
            approve: false,
            trust: false,
            force: false,
        }
    }

    #[test]
    fn notify_local_writes_to_the_receiver_inbox_not_the_sender() {
        // #672 defect 1: a message addressed to `to` must land in the RECEIVER's
        // ψ/inbox (resolved from the registry), not the sender's. The fake resolver
        // stands in for `locate_find_oracle_repo_path`; reverting the fix to write to
        // `inbox_real_env().inbox_dir` leaves this receiver dir empty → RED.
        let receiver = std::env::temp_dir().join(format!("maw-rs-notify-recv-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&receiver);
        std::fs::create_dir_all(&receiver).expect("receiver repo");
        let receiver_str = receiver.to_string_lossy().into_owned();
        let resolve = |oracle: &str| {
            assert_eq!(oracle, "arra-oracle-v3", "resolver is asked for the receiver, not the sender");
            Some(receiver_str.clone())
        };

        let out = notify_local_with(
            &notify_args("local:arra-oracle-v3", "the nudge"),
            "arra-oracle-v3",
            &HeyConfig::default(),
            "ui",
            &resolve,
        );
        assert_eq!(out.code, 0, "stderr={}", out.stderr);

        let inbox = receiver.join("ψ").join("inbox");
        let files: Vec<_> = std::fs::read_dir(&inbox).expect("receiver inbox exists").filter_map(Result::ok).collect();
        assert_eq!(files.len(), 1, "exactly one message filed in the RECEIVER inbox");
        let body = std::fs::read_to_string(files[0].path()).expect("read msg");
        assert!(body.contains("to: arra-oracle-v3"), "addressed to receiver");
        assert!(body.contains("from: m5:ui"));
        assert!(body.contains("the nudge"));
        std::fs::remove_dir_all(&receiver).ok();
    }

    #[test]
    fn notify_local_errors_clearly_when_receiver_is_unknown() {
        // An unresolvable receiver must fail loudly (route error), never silently
        // succeed by writing somewhere the receiver will not read (#672).
        let out = notify_local_with(
            &notify_args("local:ghost-oracle", "hi"),
            "ghost-oracle",
            &HeyConfig::default(),
            "ui",
            &|_oracle: &str| None,
        );
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("cannot resolve a local inbox for 'ghost-oracle'"), "stderr={}", out.stderr);
    }
}
