// Who a message says it is from, and proving it.
//
// A sender has three spellings that must not drift: the human tag in the message
// body, the wire `from` on the request, and the signed identity. They are
// derived here from the pane, the repo, the config and any explicit --from, then
// signed. An explicit --from is validated rather than trusted -- forging another
// oracle's name is exactly what the signature exists to stop.

fn send_message_signature(
    config: &HeyConfig,
    sender_oracle: &str,
    from: Option<&str>,
    text: &str,
) -> Result<Option<MessageSignature>, String> {
    if text.starts_with('[') {
        return Err("bracket-prefixed hey text is reserved for signed transport prefixes".to_owned());
    }
    let node = config.node.as_deref().filter(|value| !value.is_empty()).unwrap_or("local");
    let expected = format!("{sender_oracle}:{node}");
    if let Some(explicit) = from {
        if validate_wire_from(explicit)? != expected {
            return Err(format!("--from {explicit} does not match signing identity {expected}"));
        }
    }
    let Ok(peer_key) = load_peer_key() else { return Ok(None); };
    let headers = maw_auth::sign_ed25519_headers_at(&peer_key, &expected, "POST", "/api/send", Some(text.as_bytes()), i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX))?;
    if headers.get("X-Maw-Ed25519-Signature").unwrap_or_default().is_empty()
        || headers.get("X-Maw-Ed25519-Pubkey").unwrap_or_default().is_empty()
    {
        return Ok(None);
    }
    Ok(Some(MessageSignature))
}

fn resolve_hey_wire_from(
    explicit: Option<&str>,
    config: &HeyConfig,
    sender_oracle: &str,
) -> Result<String, String> {
    if let Some(value) = explicit {
        return validate_wire_from(value);
    }
    if let Ok(value) = std::env::var("MAW_SENDER") {
        return human_sender_to_wire_from(&value);
    }
    let node = config
        .node
        .as_deref()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "cannot resolve sender identity; set MAW_SENDER or config node".to_owned())?;
    Ok(format!("{sender_oracle}:{node}"))
}

fn validate_wire_from(value: &str) -> Result<String, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err("wire sender identity must be oracle:node".to_owned());
    }
    Ok(value.to_owned())
}

fn human_sender_to_wire_from(value: &str) -> Result<String, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 2 || parts.iter().any(|part| part.is_empty()) {
        return Err("MAW_SENDER must be node:oracle".to_owned());
    }
    Ok(format!("{}:{}", parts[1], parts[0]))
}

fn wire_sender_to_human(from: &str) -> Option<String> {
    let (oracle, node) = from.split_once(':')?;
    (!oracle.is_empty() && !node.is_empty()).then(|| format!("{node}:{oracle}"))
}

fn format_local_hey_message(
    text: &str,
    config: &HeyConfig,
    sender_oracle: &str,
    from: Option<&str>,
) -> String {
    if text.starts_with('/') {
        return text.to_owned();
    }
    let display = from.map_or_else(
        || {
            let node = config.node.as_deref().unwrap_or("local");
            format!("{node}:{sender_oracle}")
        },
        ToOwned::to_owned,
    );
    format!("[{display}] {text}")
}

fn resolve_hey_sender_oracle_for_from(config: &HeyConfig, from: Option<&str>) -> String {
    from.and_then(explicit_wire_sender_oracle)
        .unwrap_or_else(|| resolve_hey_sender_oracle(config))
}

fn explicit_wire_sender_oracle(from: &str) -> Option<String> {
    let (oracle, node) = from.split_once(':')?;
    (!oracle.is_empty() && !node.is_empty()).then(|| oracle.to_owned())
}

fn resolve_hey_sender_oracle(config: &HeyConfig) -> String {
    let mut runner = CommandTmuxRunner::new();
    let tmux_pane = std::env::var("TMUX_PANE").ok();
    let in_tmux = std::env::var("TMUX").is_ok_and(|value| !value.trim().is_empty());
    resolve_hey_sender_oracle_with(config, tmux_pane.as_deref(), in_tmux, &mut runner)
}

fn resolve_hey_sender_oracle_with<R: maw_tmux::TmuxRunner>(
    config: &HeyConfig,
    tmux_pane: Option<&str>,
    in_tmux: bool,
    runner: &mut R,
) -> String {
    tmux_pane
        .filter(|pane| !pane.trim().is_empty())
        .and_then(|pane| tmux_window_name_with(runner, Some(pane)))
        .or_else(|| resolve_hey_canonical_sender_oracle(config))
        .unwrap_or_else(|| {
            if in_tmux {
                let focused = tmux_window_name_with(runner, None);
                return format!("pane/{}", resolve_sender_oracle(None, focused.as_deref(), None));
            }
            // Headless (no TMUX/TMUX_PANE): the focused-window query would name
            // whatever window the attached client happens to show — another
            // oracle's identity (#519). Emit a truthful marker instead.
            send_headless_sender_marker()
        })
}

fn send_headless_sender_marker() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| send_cwd_repo_stem(&cwd))
        .map_or_else(|| "pane/unknown".to_owned(), |stem| format!("job/{stem}"))
}

fn send_cwd_repo_stem(cwd: &std::path::Path) -> Option<String> {
    let mut dir = cwd.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned());
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn resolve_hey_canonical_sender_oracle(config: &HeyConfig) -> Option<String> {
    if let Ok(cwd) = std::env::current_dir() {
        if let Some(oracle) = footer_claude_oracle263(&cwd) { return Some(oracle); }
    }
    let session_window = std::env::var("MAW_SESSION_WINDOW").ok();
    if session_window
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        return Some(resolve_sender_oracle(session_window.as_deref(), None, None));
    }
    config.oracle.as_deref().filter(|oracle| !oracle.trim().is_empty()).map(|oracle| oracle.trim().to_owned())
}

fn current_tmux_window_name() -> Option<String> {
    let mut runner = CommandTmuxRunner::new();
    tmux_window_name_with(&mut runner, None)
}

fn tmux_window_name_with<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    target: Option<&str>,
) -> Option<String> {
    let mut args = Vec::with_capacity(if target.is_some() { 4 } else { 2 });
    if let Some(target) = target {
        args.extend(["-t".to_owned(), target.to_owned()]);
    }
    args.extend(["-p".to_owned(), "#{window_name}".to_owned()]);
    let raw = runner.run("display-message", &args).ok()?;
    let window = raw.trim();
    (!window.is_empty()).then(|| window.to_owned())
}

fn send_normalized_from(config: &HeyConfig, sender_oracle: &str, from: Option<&str>) -> Option<String> {
    if let Some(from) = from {
        return wire_sender_to_human(from);
    }
    if let Ok(sender) = std::env::var("MAW_SENDER") {
        return human_sender_to_wire_from(&sender).ok().and_then(|wire| wire_sender_to_human(&wire));
    }
    let node = config.node.as_deref().filter(|node| !node.is_empty())?;
    let handle = resolve_hey_canonical_sender_oracle(config)
        .unwrap_or_else(|| sender_oracle.to_owned());
    Some(format!("{node}:{handle}"))
}

// Display prefix for locally formatted messages: explicit `--from oracle:node`
// renders in the same node:oracle order as resolved senders (#519); values that
// are not wire-shaped pass through verbatim.
fn send_display_from(from: Option<&str>) -> Option<String> {
    from.map(|value| wire_sender_to_human(value).unwrap_or_else(|| value.to_owned()))
}
