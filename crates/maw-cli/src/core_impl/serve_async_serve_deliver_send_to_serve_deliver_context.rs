fn serve_deliver_send(
    state: &ServeState,
    headers: &HeaderMap,
    body: &Bytes,
) -> axum::response::Response {
    let parsed = serde_json::from_slice::<SendBody>(body).unwrap_or_default();
    let target = parsed.target.clone().unwrap_or_default();
    let message = serve_send_message(&parsed);
    let raw_from = header_to_string(headers, "x-maw-from");
    let from = (!raw_from.trim().is_empty()).then_some(raw_from);
    let config = load_hey_config();
    let log_from = from.clone().unwrap_or_else(|| serve_local_identity(&config));
    let log_to = serve_local_identity(&config);

    if target.trim().is_empty() {
        serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, "empty-target", "validate");
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"ok": false, "error": "empty-target", "state": "failed"})),
        )
            .into_response();
    }

    if parsed.inbox.unwrap_or(false) {
        let context = ServeInboxContext {
            config: &config,
            log_from: &log_from,
            log_to: &log_to,
            target: &target,
            message: &message,
        };
        return serve_deliver_inbox(state, headers, &parsed, &context);
    }

    let sessions = match state.delivery.route_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, &error, "route-list");
            return serve_delivery_error(StatusCode::SERVICE_UNAVAILABLE, "route-list-failed", &target, &error);
        }
    };

    match resolve_route_target(&target, &config.route, &sessions) {
        RouteResult::Local { target: resolved } | RouteResult::SelfNode { target: resolved } => {
            let context = ServeDeliverContext {
                config: &config,
                from: from.as_deref(),
                log_from: &log_from,
                log_to: &log_to,
                requested: &target,
                resolved: &resolved,
                message: &message,
            };
            serve_deliver_local(state, &context)
        }
        RouteResult::Peer { node, .. } => {
            let error = format!("peer-forward-unavailable:{node}");
            serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, &error, "peer-forward");
            serve_delivery_error(StatusCode::BAD_GATEWAY, "peer-forward-unavailable", &target, &error)
        }
        RouteResult::Error { reason, detail, .. } => {
            let error = format!("{reason}: {detail}");
            serve_log_delivery_failed(state, &target, &message, &log_from, &log_to, &error, "resolve");
            serve_delivery_error(StatusCode::NOT_FOUND, &reason, &target, &detail)
        }
    }
}


struct ServeInboxContext<'a> {
    config: &'a HeyConfig,
    log_from: &'a str,
    log_to: &'a str,
    target: &'a str,
    message: &'a str,
}

fn serve_deliver_inbox(
    state: &ServeState,
    headers: &HeaderMap,
    parsed: &SendBody,
    context: &ServeInboxContext<'_>,
) -> axum::response::Response {
    let target = context.target;
    let message = context.message;
    let config = context.config;
    let log_from = context.log_from;
    let log_to = context.log_to;
    let sessions = match state.delivery.route_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "route-list");
            return serve_delivery_error(StatusCode::SERVICE_UNAVAILABLE, "route-list-failed", target, &error);
        }
    };
    let resolved = match resolve_route_target(target, &config.route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => target,
        RouteResult::Peer { node, .. } => {
            let error = format!("peer-forward-unavailable:{node}");
            serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "peer-forward");
            return serve_delivery_error(StatusCode::BAD_GATEWAY, "peer-forward-unavailable", target, &error);
        }
        RouteResult::Error { reason, detail, .. } => {
            let error = format!("{reason}: {detail}");
            serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "resolve");
            return serve_delivery_error(StatusCode::NOT_FOUND, &reason, target, &detail);
        }
    };
    if !serve_resolved_target_exists(&sessions, &resolved) {
        let error = format!("target not live in tmux: {resolved}");
        serve_log_delivery_failed(state, target, message, log_from, log_to, &error, "inbox");
        return serve_delivery_error(StatusCode::NOT_FOUND, "target-not-live", target, &error);
    }
    let from = serve_display_from(headers, config);
    match state.receiver_inbox.write_receiver_inbox(ReceiverInboxInput {
        query: target,
        target: Some(&resolved),
        to: Some(target),
        from: &from,
        message,
        config,
    }) {
        ReceiverInboxResult::Ok(inbox) => {
            let reason = "--inbox requested; pane injection skipped";
            serve_log_lifecycle(
                state,
                json!({
                    "kind": "context.message",
                    "direction": "inbound",
                    "state": "queued",
                    "route": "inbox",
                    "from": serve_truncate(&from, SERVE_LOG_TEXT_MAX),
                    "to": serve_truncate(log_to, SERVE_LOG_TEXT_MAX),
                    "target": resolved,
                    "requestedTarget": target,
                    "text": serve_truncate(message, SERVE_LOG_TEXT_MAX),
                    "oracle": inbox.oracle,
                    "lastLine": reason,
                    "signed": !header_to_string(headers, "x-maw-from").trim().is_empty(),
                    "source": "maw-rs-native",
                }),
            );
            Json(json!({
                "ok": true,
                "target": resolved,
                "text": parsed.text.clone().unwrap_or_default(),
                "source": "inbox",
                "state": "queued",
                "inbox": inbox.path.display().to_string(),
                "reason": reason,
                "receipt": ["fallback_queued"],
            }))
            .into_response()
        }
        ReceiverInboxResult::Err { oracle: _, reason } => {
            serve_log_delivery_failed(state, target, message, log_from, log_to, &reason, "inbox");
            serve_delivery_error(StatusCode::BAD_GATEWAY, "receiver-inbox-unavailable", target, &reason)
        }
    }
}

#[derive(Clone, Copy)]
struct ReceiverInboxInput<'a> {
    query: &'a str,
    target: Option<&'a str>,
    to: Option<&'a str>,
    from: &'a str,
    message: &'a str,
    config: &'a HeyConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReceiverInboxOk {
    oracle: String,
    inbox_dir: std::path::PathBuf,
    path: std::path::PathBuf,
    filename: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ReceiverInboxResult {
    Ok(ReceiverInboxOk),
    Err { oracle: Option<String>, reason: String },
}

struct ServeDeliverContext<'a> {
    config: &'a HeyConfig,
    from: Option<&'a str>,
    log_from: &'a str,
    log_to: &'a str,
    requested: &'a str,
    resolved: &'a str,
    message: &'a str,
}

