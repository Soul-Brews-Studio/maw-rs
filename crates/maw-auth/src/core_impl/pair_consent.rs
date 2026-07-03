#[must_use]
pub fn is_loopback(address: Option<&str>) -> bool {
    let Some(address) = address else {
        return false;
    };
    address == "127.0.0.1"
        || address == "::1"
        || address == "::ffff:127.0.0.1"
        || address == "localhost"
        || address.starts_with("127.")
}

#[must_use]
pub fn sign_headers_at(
    token: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timestamp: i64,
) -> Headers {
    let body_hash = body.map_or_else(String::new, |body| hash_body(Some(body)));
    let mut headers = vec![
        ("X-Maw-Timestamp".to_owned(), timestamp.to_string()),
        (
            "X-Maw-Signature".to_owned(),
            sign(token, method, path, timestamp, &body_hash),
        ),
    ];
    if !body_hash.is_empty() {
        headers.push(("X-Maw-Auth-Version".to_owned(), "v2".to_owned()));
    }
    Headers::new(headers)
}

/// Sign the v3 `from:` request payload.
///
/// # Errors
///
/// Returns an error when `peer_key` or `from_address` is empty, matching maw-js's
/// loud validation branches.
pub fn sign_request_v3(
    peer_key: &str,
    from_address: &str,
    method: &str,
    path: &str,
    timestamp: i64,
    body: Option<&[u8]>,
) -> Result<V3Signature, String> {
    if peer_key.is_empty() {
        return Err("signRequestV3: peerKey is required".to_owned());
    }
    if from_address.is_empty() {
        return Err("signRequestV3: fromAddress is required (<oracle>:<node>)".to_owned());
    }
    let method = if method.is_empty() { "GET" } else { method }.to_uppercase();
    let body_hash = body.map_or_else(String::new, |body| hash_body(Some(body)));
    let payload = build_from_sign_payload(from_address, timestamp, &method, path, &body_hash);
    Ok(V3Signature {
        signature: hmac_sha256_hex(peer_key, &payload),
        body_hash,
    })
}

/// Produce v3 outbound auth headers.
///
/// # Errors
///
/// Returns an error when v3 signing inputs are invalid.
pub fn sign_headers_v3_at(
    peer_key: &str,
    from_address: &str,
    method: &str,
    path: &str,
    body: Option<&[u8]>,
    timestamp: i64,
) -> Result<Headers, String> {
    let signature = sign_request_v3(peer_key, from_address, method, path, timestamp, body)?;
    Ok(Headers::new([
        ("X-Maw-From", from_address.to_owned()),
        ("X-Maw-Signature-V3", signature.signature),
        ("X-Maw-Timestamp", timestamp.to_string()),
        ("X-Maw-Auth-Version", "v3".to_owned()),
    ]))
}

#[must_use]
pub fn resolve_from_address(config: &FromAddressConfig) -> Option<String> {
    let node = config.node.as_deref()?;
    let oracle = config.oracle.as_deref().unwrap_or(DEFAULT_ORACLE);
    Some(format!("{oracle}:{node}"))
}

#[must_use]
pub fn normalize_pair_code(raw: &str) -> String {
    raw.chars()
        .filter(|ch| *ch != '-' && !ch.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect()
}

#[must_use]
pub fn is_valid_pair_code_shape(code: &str) -> bool {
    let code = normalize_pair_code(code);
    code.len() == 6 && code.chars().all(|ch| PAIR_CODE_ALPHABET.contains(ch))
}

#[must_use]
pub fn pretty_pair_code(code: &str) -> String {
    let code = normalize_pair_code(code);
    if code.len() == 6 {
        format!("{}-{}", &code[..3], &code[3..])
    } else {
        code
    }
}

#[must_use]
pub fn redact_pair_code(code: &str) -> String {
    let code = normalize_pair_code(code);
    if code.len() >= 3 {
        format!("{}-***", &code[..3])
    } else {
        "***".to_owned()
    }
}

#[must_use]
pub fn generate_pair_code_from_bytes(bytes: &[u8]) -> String {
    let alphabet = PAIR_CODE_ALPHABET.as_bytes();
    bytes
        .iter()
        .take(6)
        .map(|byte| char::from(alphabet[usize::from(byte % 32)]))
        .collect()
}

#[must_use]
pub fn hash_consent_pin(pin: &str) -> String {
    hex_lower(&Sha256::digest(normalize_pair_code(pin).as_bytes()))
}

#[must_use]
pub fn verify_consent_pin(pin: &str, expected_hash: &str) -> bool {
    is_valid_pair_code_shape(pin) && hash_consent_pin(pin) == expected_hash
}

#[must_use]
pub fn consent_request_id_from_bytes(bytes: &[u8]) -> String {
    hex_lower(&bytes.iter().copied().take(12).collect::<Vec<_>>())
}

pub fn pair_api_generate_plan(
    store: &mut PairCodeStore,
    config: &PairApiConfig,
    code: &str,
    expires_sec: Option<u64>,
    ttl_ms: Option<u64>,
    now_ms: u64,
) -> PairApiGenerateResult {
    let ttl_ms =
        ttl_ms.unwrap_or_else(|| expires_sec.map_or(120_000, |sec| sec.saturating_mul(1_000)));
    let entry = store.register_at(code, ttl_ms, now_ms);
    PairApiGenerateResult {
        status: 201,
        ok: true,
        code: pretty_pair_code(&entry.code),
        expires_at: entry.expires_at,
        ttl_ms,
        node: config.node.clone(),
        port: config.port,
    }
}

#[must_use]
pub fn pair_api_probe_plan(
    store: &PairCodeStore,
    config: &PairApiConfig,
    code: &str,
    now_ms: u64,
) -> PairApiProbeResult {
    if !is_valid_pair_code_shape(code) {
        return pair_api_probe_error(400, "invalid_shape");
    }
    match store.lookup_at(code, now_ms) {
        LookupResult::Live(_) => PairApiProbeResult {
            status: 200,
            ok: true,
            error: None,
            node: Some(config.node.clone()),
        },
        LookupResult::NotFound => pair_api_probe_error(404, "not_found"),
        LookupResult::Expired => pair_api_probe_error(410, "expired"),
        LookupResult::Consumed => pair_api_probe_error(410, "consumed"),
    }
}

pub fn pair_api_accept_plan(
    store: &mut PairCodeStore,
    config: &PairApiConfig,
    code: &str,
    input: Option<PairAcceptInput>,
    now_ms: u64,
) -> PairApiAcceptResult {
    if !is_valid_pair_code_shape(code) {
        return pair_api_accept_error(400, "invalid_shape");
    }
    let Some(input) = input.filter(|input| !input.node.is_empty() && input.url.is_some()) else {
        return pair_api_accept_error(400, "bad_request");
    };
    match store.consume_at(code, now_ms) {
        LookupResult::Live(_) => {
            store.accepted.insert(normalize_pair_code(code), input);
            PairApiAcceptResult {
                status: 200,
                ok: true,
                error: None,
                node: Some(config.node.clone()),
                url: Some(config.base_url.clone()),
                federation_token: Some(config.federation_token.clone()),
            }
        }
        LookupResult::NotFound => pair_api_accept_error(404, "not_found"),
        LookupResult::Expired => pair_api_accept_error(410, "expired"),
        LookupResult::Consumed => pair_api_accept_error(410, "consumed"),
    }
}

#[must_use]
pub fn pair_api_status_plan(store: &PairCodeStore, code: &str, now_ms: u64) -> PairApiStatusResult {
    if !is_valid_pair_code_shape(code) {
        return pair_api_status_error(400, "invalid_shape");
    }
    let normalized = normalize_pair_code(code);
    match store.lookup_at(&normalized, now_ms) {
        LookupResult::Live(_) => PairApiStatusResult {
            status: 200,
            ok: true,
            error: None,
            consumed: Some(false),
            remote_node: None,
            remote_url: None,
        },
        LookupResult::Consumed => {
            let accepted = store.accepted.get(&normalized);
            PairApiStatusResult {
                status: 200,
                ok: true,
                error: None,
                consumed: Some(true),
                remote_node: accepted.map(|input| input.node.clone()),
                remote_url: accepted.and_then(|input| input.url.clone()),
            }
        }
        LookupResult::NotFound => pair_api_status_error(404, "not_found"),
        LookupResult::Expired => pair_api_status_error(410, "expired"),
    }
}

#[must_use]
pub fn pair_api_auto_plan(
    config: &PairApiConfig,
    hellos: &RecentHelloStore,
    input: Option<AutoPairInput>,
    add_outcome: AutoPairAddOutcome,
    now_ms: u64,
) -> PairApiAutoResult {
    let Some(input) = input
        .filter(|input| !input.node.is_empty() && !input.url.is_empty() && !input.zid.is_empty())
    else {
        return pair_api_auto_error(400, "missing_fields");
    };
    if !hellos.is_recent(&input.zid, now_ms) {
        return pair_api_auto_error(403, "no_recent_hello");
    }
    match add_outcome {
        AutoPairAddOutcome::PubkeyMismatch(message) => pair_api_auto_error(409, &message),
        AutoPairAddOutcome::Error(message) => pair_api_auto_error(400, &message),
        AutoPairAddOutcome::Ok { one_way } => {
            let identity = AutoPairIdentity {
                node: config.node.clone(),
                oracle: config.oracle.clone(),
                url: config.base_url.clone(),
                pubkey: config.pubkey.clone(),
            };
            PairApiAutoResult {
                status: 200,
                ok: true,
                error: None,
                node: Some(config.node.clone()),
                oracle: Some(config.oracle.clone()),
                url: Some(config.base_url.clone()),
                pubkey: Some(config.pubkey.clone()),
                proof: Some(sign_auto_pair_proof(&identity, &config.federation_token)),
                one_way: Some(one_way),
                add_alias: Some(input.node.clone()),
                add_url: Some(input.url.clone()),
                add_node: Some(input.node.clone()),
                add_pubkey: input.pubkey.clone(),
                add_identity_oracle: input.oracle,
                add_identity_node: Some(input.node),
                mark_symmetric_check: true,
            }
        }
    }
}

#[must_use]
pub fn trust_key(from: &str, to: &str, action: ConsentAction) -> String {
    format!("{from}→{to}:{}", action.as_str())
}

#[must_use]
pub fn apply_consent_expiry(request: &PendingRequest, now_ms: i64) -> PendingRequest {
    if request.status == ConsentStatus::Pending
        && parse_iso_millis(&request.expires_at).is_some_and(|expires_at| now_ms > expires_at)
    {
        return PendingRequest {
            status: ConsentStatus::Expired,
            ..request.clone()
        };
    }
    request.clone()
}

pub fn request_consent_plan(
    store: &mut ConsentStore,
    args: ConsentRequestArgs,
) -> ConsentRequestResult {
    const TTL_MS: i64 = 10 * 60 * 1_000;
    let created_at = iso_from_unix_millis(args.now_ms);
    let expires_at = iso_from_unix_millis(args.now_ms.saturating_add(TTL_MS));
    let pending = PendingRequest {
        id: args.request_id.clone(),
        from: args.from,
        to: args.to,
        action: args.action,
        summary: args.summary,
        pin_hash: hash_consent_pin(&args.pin),
        created_at,
        expires_at: expires_at.clone(),
        status: ConsentStatus::Pending,
    };
    store.write_pending(pending.clone());

    let peer_url = args
        .peer_url
        .as_deref()
        .map(|url| format!("{}/api/consent/request", url.trim_end_matches('/')));
    let peer_body = peer_url.as_ref().map(|_| PeerPendingRequest {
        id: pending.id.clone(),
        from: pending.from.clone(),
        to: pending.to.clone(),
        action: pending.action,
        summary: pending.summary.clone(),
        pin_hash: pending.pin_hash.clone(),
        created_at: pending.created_at.clone(),
        expires_at: pending.expires_at.clone(),
        status: pending.status,
        pin: None,
    });

    match args.peer_post {
        PeerPostResult::HttpStatus(status) if status >= 400 => ConsentRequestResult {
            ok: false,
            request_id: Some(args.request_id),
            pin: None,
            expires_at: Some(expires_at),
            error: Some(format!("peer rejected request: HTTP {status}")),
            already_trusted: false,
            peer_url,
            peer_method: Some("POST".to_owned()),
            peer_body,
        },
        PeerPostResult::NetworkError(message) => ConsentRequestResult {
            ok: false,
            request_id: Some(args.request_id),
            pin: None,
            expires_at: Some(expires_at),
            error: Some(format!("network error contacting peer: {message}")),
            already_trusted: false,
            peer_url,
            peer_method: Some("POST".to_owned()),
            peer_body,
        },
        PeerPostResult::Skipped | PeerPostResult::Ok | PeerPostResult::HttpStatus(_) => {
            ConsentRequestResult {
                ok: true,
                request_id: Some(args.request_id),
                pin: Some(args.pin),
                expires_at: Some(expires_at),
                error: None,
                already_trusted: false,
                peer_method: peer_url.as_ref().map(|_| "POST".to_owned()),
                peer_url,
                peer_body,
            }
        }
    }
}

pub fn approve_consent_plan(
    store: &mut ConsentStore,
    request_id: &str,
    pin: &str,
    now_ms: i64,
) -> ConsentApprovalResult {
    let Some(request) = store.read_pending(request_id) else {
        return consent_error(format!("request not found: {request_id}"));
    };
    let request = apply_consent_expiry(&request, now_ms);
    if request.status != ConsentStatus::Pending {
        return consent_error(format!(
            "request is {}, cannot approve",
            consent_status_str(request.status)
        ));
    }
    if !verify_consent_pin(pin, &request.pin_hash) {
        return consent_error("PIN mismatch");
    }
    store.update_status(request_id, ConsentStatus::Approved);
    let entry = TrustEntry {
        from: request.from,
        to: request.to,
        action: request.action,
        approved_at: iso_from_unix_millis(now_ms),
        approved_by: ApprovedBy::Human,
        request_id: Some(request_id.to_owned()),
    };
    store.record_trust(entry.clone());
    ConsentApprovalResult {
        ok: true,
        error: None,
        entry: Some(entry),
    }
}

pub fn reject_consent_plan(store: &mut ConsentStore, request_id: &str) -> ConsentApprovalResult {
    let Some(request) = store.read_pending(request_id) else {
        return consent_error(format!("request not found: {request_id}"));
    };
    if request.status != ConsentStatus::Pending {
        return consent_error(format!(
            "request is {}, cannot reject",
            consent_status_str(request.status)
        ));
    }
    store.update_status(request_id, ConsentStatus::Rejected);
    ConsentApprovalResult {
        ok: true,
        error: None,
        entry: None,
    }
}

#[must_use]
pub fn build_from_sign_payload(
    from: &str,
    timestamp: i64,
    method: &str,
    path: &str,
    body_hash: &str,
) -> String {
    format!(
        "{}:{path}:{timestamp}:{body_hash}:{from}",
        method.to_uppercase()
    )
}

#[must_use]
pub fn build_legacy_from_sign_payload(
    from: &str,
    signed_at: &str,
    method: &str,
    path: &str,
    body_hash: &str,
) -> String {
    format!(
        "{from}\n{signed_at}\n{}\n{path}\n{body_hash}",
        method.to_uppercase()
    )
}

