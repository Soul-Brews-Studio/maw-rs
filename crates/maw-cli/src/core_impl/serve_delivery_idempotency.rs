// Delivering the same message twice must not land it twice.
//
// A sender that retries after a timeout re-sends an identical payload, so every
// inbound delivery claims a key first (sender + target + logical timestamp +
// body digest). The claim is held while the write is in flight and settled after,
// so a concurrent retry either waits for the original's outcome or is answered
// from it -- never re-injected into the pane. Claims age out on a TTL sweep.

enum ServeInboxIdempotencyClaim {
    Claimed(Option<DeliveryIdempotencyKey>),
    Duplicate(Box<axum::response::Response>),
}

fn serve_claim_inbox_idempotency(
    state: &ServeState,
    headers: &HeaderMap,
    parsed: &SendBody,
    resolved: &str,
    context: &ServeInboxContext<'_>,
) -> ServeInboxIdempotencyClaim {
    let idempotency_key =
        serve_delivery_idempotency_key(headers, context.log_from, resolved, context.message);
    let Some(key) = idempotency_key.clone() else {
        return ServeInboxIdempotencyClaim::Claimed(None);
    };
    match serve_delivery_idempotency_claim(state, key.clone(), serve_delivery_idempotency_now(state)) {
        DeliveryIdempotencyClaim::Claimed => ServeInboxIdempotencyClaim::Claimed(idempotency_key),
        DeliveryIdempotencyClaim::Duplicate(record) => {
            serve_log_delivery_deduped(
                state,
                &key,
                resolved,
                context.message,
                context.log_from,
                context.log_to,
                "inbox",
            );
            ServeInboxIdempotencyClaim::Duplicate(Box::new(serve_delivery_idempotency_response(
                &record,
                resolved,
                &parsed.text.clone().unwrap_or_default(),
                "inbox",
            )))
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DeliveryIdempotencyKey {
    source: String,
    target: String,
    payload_hash: String,
    logical_ts: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeliveryIdempotencyRecord {
    InFlight { seen_at: i64 },
    Complete {
        target: String,
        state: String,
        seen_at: i64,
    },
}

impl DeliveryIdempotencyRecord {
    fn response_state(&self) -> &str {
        match self {
            Self::InFlight { .. } => "queued",
            Self::Complete { state, .. } => state,
        }
    }

    fn response_target<'a>(&'a self, fallback: &'a str) -> &'a str {
        match self {
            Self::InFlight { .. } => fallback,
            Self::Complete { target, .. } => target,
        }
    }

    const fn seen_at(&self) -> i64 {
        match self {
            Self::InFlight { seen_at } | Self::Complete { seen_at, .. } => *seen_at,
        }
    }
}

#[derive(Default)]
struct DeliveryIdempotencyStore {
    records: HashMap<DeliveryIdempotencyKey, DeliveryIdempotencyRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum DeliveryIdempotencyClaim {
    Claimed,
    Duplicate(DeliveryIdempotencyRecord),
}

fn serve_delivery_idempotency_key(
    headers: &HeaderMap,
    fallback_source: &str,
    target: &str,
    payload: &str,
) -> Option<DeliveryIdempotencyKey> {
    let logical_ts = serve_delivery_logical_ts(headers)?;
    let raw_source = header_to_string(headers, "x-maw-from");
    let source = raw_source.trim();
    let source = if source.is_empty() { fallback_source.trim() } else { source };
    let target = target.trim();
    if source.is_empty() || target.is_empty() {
        return None;
    }
    let payload_hash = maw_auth::hash_body(Some(payload.as_bytes()));
    if payload_hash.is_empty() {
        return None;
    }
    Some(DeliveryIdempotencyKey {
        source: source.to_owned(),
        target: target.to_owned(),
        payload_hash,
        logical_ts,
    })
}

fn serve_delivery_logical_ts(headers: &HeaderMap) -> Option<String> {
    ["x-maw-timestamp", "x-maw-signed-at"]
        .into_iter()
        .map(|name| header_to_string(headers, name))
        .map(|value| value.trim().to_owned())
        .find(|value| !value.is_empty())
}

fn serve_delivery_idempotency_claim(
    state: &ServeState,
    key: DeliveryIdempotencyKey,
    now: i64,
) -> DeliveryIdempotencyClaim {
    let mut store = state
        .delivery_idempotency
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    serve_delivery_idempotency_prune(&mut store, now);
    if let Some(record) = store.records.get(&key).cloned() {
        return DeliveryIdempotencyClaim::Duplicate(record);
    }
    store
        .records
        .insert(key, DeliveryIdempotencyRecord::InFlight { seen_at: now });
    DeliveryIdempotencyClaim::Claimed
}

fn serve_delivery_idempotency_complete(
    state: &ServeState,
    key: DeliveryIdempotencyKey,
    target: &str,
    state_name: &str,
    now: i64,
) {
    let mut store = state
        .delivery_idempotency
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    store.records.insert(
        key,
        DeliveryIdempotencyRecord::Complete {
            target: target.to_owned(),
            state: state_name.to_owned(),
            seen_at: now,
        },
    );
}

fn serve_delivery_idempotency_cancel(state: &ServeState, key: &DeliveryIdempotencyKey) {
    let mut store = state
        .delivery_idempotency
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if matches!(store.records.get(key), Some(DeliveryIdempotencyRecord::InFlight { .. })) {
        store.records.remove(key);
    }
}

fn serve_delivery_idempotency_prune(store: &mut DeliveryIdempotencyStore, now: i64) {
    store.records.retain(|_, record| {
        let age = now.saturating_sub(record.seen_at());
        age <= DELIVERY_IDEMPOTENCY_TTL_SECONDS
    });
}

fn serve_delivery_idempotency_now(state: &ServeState) -> i64 {
    #[cfg(test)]
    {
        state
            .now_override
            .unwrap_or_else(|| i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX))
    }
    #[cfg(not(test))]
    {
        let _ = state;
        i64::try_from(current_epoch_seconds()).unwrap_or(i64::MAX)
    }
}

fn serve_delivery_idempotency_response(
    record: &DeliveryIdempotencyRecord,
    fallback_target: &str,
    text: &str,
    source: &str,
) -> axum::response::Response {
    let target = record.response_target(fallback_target);
    let state_name = record.response_state();
    Json(json!({
        "ok": true,
        "target": target,
        "text": text,
        "source": source,
        "state": state_name,
        "deduped": true,
        "idempotent": true,
        "reason": "duplicate delivery dropped by idempotency key",
        "receipt": ["duplicate_dropped"],
        "lastLine": "duplicate delivery dropped by idempotency key",
    }))
    .into_response()
}

fn serve_log_delivery_deduped(
    state: &ServeState,
    key: &DeliveryIdempotencyKey,
    target: &str,
    message: &str,
    from: &str,
    to: &str,
    route: &str,
) {
    serve_log_lifecycle(
        state,
        json!({
            "kind": "context.message",
            "direction": "inbound",
            "state": "deduped",
            "route": route,
            "from": serve_truncate(from, SERVE_LOG_TEXT_MAX),
            "to": serve_truncate(to, SERVE_LOG_TEXT_MAX),
            "target": target,
            "text": serve_truncate(message, SERVE_LOG_TEXT_MAX),
            "oracle": serve_oracle_from_target(target),
            "source": "maw-rs-native",
            "idempotency": {
                "source": &key.source,
                "target": &key.target,
                "payloadHash": &key.payload_hash,
                "logicalTs": &key.logical_ts,
            },
        }),
    );
}
