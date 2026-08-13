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

// #795: `from`, when present, is the WIRE-shaped `oracle:node` string
// (matching validate_wire_from / the signed `expected` identity and the raw
// `x-maw-from` header serve.rs hands to this same function) -- never the
// human-order display string. Both branches must render human `node:oracle`
// so a local and a federated delivery of the same sender produce the
// identical [..] tag; passing the wire value straight through here was the
// bug (#795).
fn format_local_hey_message(
    text: &str,
    config: &HeyConfig,
    sender_oracle: &str,
    from: Option<&str>,
) -> String {
    if text.starts_with('/') {
        return text.to_owned();
    }
    let display = from.and_then(wire_sender_to_human).unwrap_or_else(|| {
        let node = config.node.as_deref().unwrap_or("local");
        format!("{node}:{sender_oracle}")
    });
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
    let (sender, warnings) =
        resolve_hey_sender_oracle_with(config, tmux_pane.as_deref(), in_tmux, &mut runner);
    // #786: a background job whose cwd has no oracle marker used to fall
    // through to MAW_SESSION_WINDOW silently -- the launching pane's window
    // name baked into the job's env at spawn time -- signing every outgoing
    // message as that pane's oracle for the job's whole lifetime, visible
    // only in RECIPIENTS' logs. Surface it here, to stderr so piped stdout
    // (e.g. `delivered ...`) stays clean, on the very first message.
    for warning in &warnings {
        eprintln!("⚠ {warning}");
    }
    sender
}

/// Resolve the sender oracle (pane name, then cwd/env/config), plus any
/// warnings (#786) about how it was resolved. Callers that print to the user
/// (`resolve_hey_sender_oracle`) surface the warnings; callers that only need
/// the resolved value (e.g. re-deriving a display `from` for an audit
/// record, where the primary resolution already warned once) may ignore
/// them.
fn resolve_hey_sender_oracle_with<R: maw_tmux::TmuxRunner>(
    config: &HeyConfig,
    tmux_pane: Option<&str>,
    in_tmux: bool,
    runner: &mut R,
) -> (String, Vec<String>) {
    if let Some(pane_oracle) = tmux_pane
        .filter(|pane| !pane.trim().is_empty())
        .and_then(|pane| tmux_window_name_with(runner, Some(pane)))
    {
        return (pane_oracle, Vec::new());
    }
    let (canonical, warnings) = resolve_hey_canonical_sender_oracle(config);
    let sender = canonical.unwrap_or_else(|| {
        if in_tmux {
            let focused = tmux_window_name_with(runner, None);
            return format!("pane/{}", resolve_sender_oracle(None, focused.as_deref(), None));
        }
        // Headless (no TMUX/TMUX_PANE): the focused-window query would name
        // whatever window the attached client happens to show — another
        // oracle's identity (#519). Emit a truthful marker instead.
        send_headless_sender_marker()
    });
    (sender, warnings)
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

/// Canonical (non-pane) sender resolution: cwd oracle marker (walked upward
/// by `footer_claude_oracle263`), then `MAW_SESSION_WINDOW`, then the
/// configured oracle. Also returns warnings (#786):
///
/// - cwd resolves to nothing and `MAW_SESSION_WINDOW` is used instead -- the
///   exact silent misattribution from the issue (a background job's cwd and
///   its launching pane can legitimately be different repos).
/// - cwd AND `MAW_SESSION_WINDOW` both resolve but disagree -- cwd still wins
///   (unchanged precedence), but only the operator can say whether that is
///   right for a background job, so it is surfaced rather than silently
///   preferred.
fn resolve_hey_canonical_sender_oracle(config: &HeyConfig) -> (Option<String>, Vec<String>) {
    let cwd_oracle = std::env::current_dir()
        .ok()
        .and_then(|cwd| footer_claude_oracle263(&cwd));
    let session_window = std::env::var("MAW_SESSION_WINDOW").ok();
    let session_oracle = session_window
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(|value| resolve_sender_oracle(Some(value), None, None));

    let mut warnings = Vec::new();
    if let (Some(cwd_value), Some(session_value)) = (&cwd_oracle, &session_oracle) {
        if cwd_value != session_value {
            warnings.push(format!(
                "sender identity conflict: cwd oracle marker resolves to '{cwd_value}' but MAW_SESSION_WINDOW resolves to '{session_value}'; signing as '{cwd_value}' (cwd wins)"
            ));
        }
    }
    if let Some(oracle) = cwd_oracle {
        return (Some(oracle), warnings);
    }
    if let Some(oracle) = session_oracle {
        warnings.push(format!(
            "signing as '{oracle}' (from MAW_SESSION_WINDOW); cwd has no oracle marker"
        ));
        return (Some(oracle), warnings);
    }
    let configured = config
        .oracle
        .as_deref()
        .filter(|oracle| !oracle.trim().is_empty())
        .map(|oracle| oracle.trim().to_owned());
    (configured, warnings)
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
    // Warnings ignored here: this re-derives the display `from` for an audit
    // record after the primary resolve_hey_sender_oracle call already
    // surfaced them once for this invocation (#786).
    let (canonical, _warnings) = resolve_hey_canonical_sender_oracle(config);
    let handle = canonical.unwrap_or_else(|| sender_oracle.to_owned());
    Some(format!("{node}:{handle}"))
}
