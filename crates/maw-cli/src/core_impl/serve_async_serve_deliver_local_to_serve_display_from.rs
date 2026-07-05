fn serve_deliver_local(
    state: &ServeState,
    context: &ServeDeliverContext<'_>,
) -> axum::response::Response {
    let fresh_sessions = match state.delivery.route_sessions() {
        Ok(sessions) => sessions,
        Err(error) => {
            serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "toctou-list");
            return serve_delivery_error(StatusCode::SERVICE_UNAVAILABLE, "route-list-failed", context.requested, &error);
        }
    };
    if !serve_resolved_target_exists(&fresh_sessions, context.resolved) {
        let error = format!("target disappeared before delivery: {}", context.resolved);
        serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "toctou");
        return serve_delivery_error(StatusCode::NOT_FOUND, "target-disappeared", context.requested, &error);
    }

    let outbound = format_local_hey_message(context.message, context.config, context.from);
    if let Err(error) = state.delivery.send_literal_enter(context.resolved, &outbound) {
        serve_log_delivery_failed(state, context.requested, context.message, context.log_from, context.log_to, &error, "tmux-send");
        return serve_delivery_error(StatusCode::BAD_GATEWAY, "tmux-send-failed", context.resolved, &error);
    }

    let capture = state.delivery.capture_tail(context.resolved, 8).unwrap_or_default();
    let state_name = if capture.contains("Press up to edit queued messages") {
        "queued"
    } else {
        "delivered"
    };
    let last_line = serve_last_nonempty_line(&capture);
    serve_log_lifecycle(
        state,
        json!({
            "kind": "context.message",
            "direction": "inbound",
            "state": state_name,
            "route": "local",
            "context.from": serve_truncate(context.log_from, SERVE_LOG_TEXT_MAX),
            "to": serve_truncate(context.log_to, SERVE_LOG_TEXT_MAX),
            "target": context.resolved,
            "requestedTarget": context.requested,
            "text": serve_truncate(context.message, SERVE_LOG_TEXT_MAX),
            "oracle": serve_oracle_from_target(context.requested),
            "lastLine": serve_truncate(&last_line, SERVE_LOG_TEXT_MAX),
            "source": "maw-rs-native",
        }),
    );
    Json(json!({
        "ok": true,
        "target": context.resolved,
        "text": context.message,
        "source": "maw-rs",
        "state": state_name,
        "lastLine": last_line,
    }))
    .into_response()
}

fn serve_delivery_error(
    status: StatusCode,
    error: &str,
    target: &str,
    detail: &str,
) -> axum::response::Response {
    (
        status,
        Json(json!({
            "ok": false,
            "error": error,
            "target": target,
            "detail": serve_truncate(detail, SERVE_LOG_ERROR_MAX),
            "state": "failed"
        })),
    )
        .into_response()
}

fn serve_log_delivery_failed(
    state: &ServeState,
    target: &str,
    message: &str,
    from: &str,
    to: &str,
    error: &str,
    route: &str,
) {
    serve_log_lifecycle(
        state,
        json!({
            "kind": "message",
            "direction": "inbound",
            "state": "failed",
            "route": route,
            "from": serve_truncate(from, SERVE_LOG_TEXT_MAX),
            "to": serve_truncate(to, SERVE_LOG_TEXT_MAX),
            "target": target,
            "text": serve_truncate(message, SERVE_LOG_TEXT_MAX),
            "oracle": serve_oracle_from_target(target),
            "error": serve_truncate(error, SERVE_LOG_ERROR_MAX),
            "source": "maw-rs-native",
        }),
    );
}

fn serve_log_lifecycle(state: &ServeState, event: Value) {
    match state.feed.lock() {
        Ok(mut feed) => serve_push_feed_event(&mut feed, event),
        Err(poisoned) => {
            let mut feed = poisoned.into_inner();
            serve_push_feed_event(&mut feed, event);
        }
    }
}

fn serve_push_feed_event(feed: &mut Vec<Value>, mut event: Value) {
    if let Value::Object(map) = &mut event {
        map.insert("timestamp".to_owned(), json!(unix_seconds()));
    }
    feed.push(event);
    if feed.len() > SERVE_FEED_MAX {
        let drain = feed.len() - SERVE_FEED_MAX;
        feed.drain(0..drain);
    }
}

fn serve_feed_snapshot(state: &ServeState, limit: Option<usize>) -> Vec<Value> {
    let events = match state.feed.lock() {
        Ok(feed) => feed.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Some(limit) = limit {
        let start = events.len().saturating_sub(limit);
        events[start..].to_vec()
    } else {
        events
    }
}

fn serve_send_message(body: &SendBody) -> String {
    let text = body.text.clone().unwrap_or_default();
    match &body.attachments {
        Some(attachments) if !attachments.is_empty() => {
            let mut parts = attachments.clone();
            parts.push(text);
            parts.join("\n")
        }
        _ => text,
    }
}

fn serve_resolved_target_exists(sessions: &[RouteSession], target: &str) -> bool {
    if target.starts_with('%') {
        return false;
    }
    let (session_name, window_part) = target.split_once(':').unwrap_or((target, ""));
    let Some(session) = sessions.iter().find(|session| session.name == session_name) else {
        return false;
    };
    if window_part.is_empty() {
        return true;
    }
    let window_part = window_part.split('.').next().unwrap_or(window_part);
    session.windows.iter().any(|window| {
        window.index.to_string() == window_part || window.name.eq_ignore_ascii_case(window_part)
    })
}

fn serve_last_nonempty_line(text: &str) -> String {
    text.lines()
        .rev()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim_end()
        .to_owned()
}

fn serve_truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_owned();
    }
    let mut out = value.chars().take(max.saturating_sub(1)).collect::<String>();
    out.push('…');
    out
}

fn serve_local_identity(config: &HeyConfig) -> String {
    let node = config.node.as_deref().unwrap_or("local");
    let oracle = config.oracle.as_deref().unwrap_or(DEFAULT_ORACLE);
    format!("{node}:{oracle}")
}

fn serve_oracle_from_target(target: &str) -> String {
    target
        .split([':', '.'])
        .next()
        .unwrap_or(target)
        .to_owned()
}

fn serve_display_from(headers: &HeaderMap, config: &HeyConfig) -> String {
    let raw = header_to_string(headers, "x-maw-from");
    let raw = raw.trim();
    if raw.is_empty() {
        return serve_local_identity(config);
    }
    if let Some((oracle, node)) = raw.split_once(':') {
        let oracle = oracle.trim();
        let node = node.trim();
        if !oracle.is_empty() && !node.is_empty() {
            return format!("{node}:{oracle}");
        }
    }
    raw.to_owned()
}

