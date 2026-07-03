#[must_use]
pub fn sign_hmac_sig(secret: &str, payload: &str) -> String {
    hmac_sha256_hex(secret, payload)
}

#[must_use]
pub fn verify_hmac_sig(secret: &str, payload: &str, signature_hex: &str) -> bool {
    if signature_hex.is_empty() || !signature_hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return false;
    }
    let expected = hmac_sha256_hex(secret, payload);
    constant_time_eq(expected.as_bytes(), signature_hex.as_bytes())
}

#[must_use]
pub fn sign_auto_pair_proof(identity: &AutoPairIdentity, federation_token: &str) -> String {
    hmac_sha256_hex(federation_token, &canonical_auto_pair_identity(identity))
}

#[must_use]
pub fn verify_auto_pair_proof(
    identity: &AutoPairIdentity,
    federation_token: &str,
    proof: &str,
) -> bool {
    if proof.len() != 64 || !proof.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return false;
    }
    let expected = sign_auto_pair_proof(identity, federation_token);
    constant_time_eq(expected.as_bytes(), proof.as_bytes())
}

fn canonical_auto_pair_identity(identity: &AutoPairIdentity) -> String {
    [
        identity.oracle.as_str(),
        identity.node.as_str(),
        identity.url.as_str(),
        identity.pubkey.as_str(),
    ]
    .join("\n")
}

struct SignedInput {
    from: String,
    v3_sig: String,
    v3_timestamp: String,
    legacy_sig: String,
    legacy_signed_at: String,
    has_v3_sig: bool,
    signed: bool,
}

fn signed_input(headers: &Headers) -> SignedInput {
    let from = headers
        .get("x-maw-from")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let v3_sig = headers
        .get("x-maw-signature-v3")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let v3_timestamp = headers
        .get("x-maw-timestamp")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let legacy_sig = headers
        .get("x-maw-signature")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let legacy_signed_at = headers
        .get("x-maw-signed-at")
        .unwrap_or_default()
        .trim()
        .to_owned();
    let has_v3_sig = !from.is_empty() && !v3_sig.is_empty() && !v3_timestamp.is_empty();
    let has_legacy_sig = !from.is_empty() && !legacy_sig.is_empty() && !legacy_signed_at.is_empty();
    SignedInput {
        from,
        v3_sig,
        v3_timestamp,
        legacy_sig,
        legacy_signed_at,
        has_v3_sig,
        signed: has_v3_sig || has_legacy_sig,
    }
}

fn signed_at_seconds(signed: &SignedInput) -> Option<i64> {
    if signed.has_v3_sig {
        parse_unix_seconds(&signed.v3_timestamp)
    } else {
        parse_iso_seconds(&signed.legacy_signed_at)
    }
}


fn header_trimmed<'a>(headers: &'a Headers, name: &str) -> &'a str {
    headers.get(name).map(str::trim).unwrap_or_default()
}

fn verify_workspace_hmac(parts: &RequestAuthParts, body_hash: &str) -> Result<Option<String>, String> {
    let Some(workspace_key) = parts
        .workspace_key
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    let signature = header_trimmed(&parts.headers, "x-maw-signature");
    let timestamp_raw = header_trimmed(&parts.headers, "x-maw-timestamp");
    if signature.is_empty() || timestamp_raw.is_empty() {
        return Err("missing-signature".to_owned());
    }
    let Some(timestamp) = parse_unix_seconds(timestamp_raw) else {
        return Err("invalid-timestamp".to_owned());
    };
    if (parts.now - timestamp).abs() > WINDOW_SEC {
        return Err("timestamp-out-of-window".to_owned());
    }

    let auth_version = header_trimmed(&parts.headers, "x-maw-auth-version").to_ascii_lowercase();
    let method = parts.method.to_uppercase();
    let mut paths = vec![parts.path.as_str()];
    let api_prefixed;
    if !parts.path.starts_with("/api/") {
        api_prefixed = format!("/api{}", parts.path);
        paths.push(api_prefixed.as_str());
    }
    let mut v1_payloads = Vec::new();
    let mut v2_payloads = Vec::new();
    for path in paths {
        v1_payloads.push(("hmac-v1", format!("{method}:{path}:{timestamp}")));
        v2_payloads.push(("hmac-v2", format!("{method}:{path}:{timestamp}:{body_hash}")));
    }
    let payloads = match auth_version.as_str() {
        "" | "v1" => v1_payloads,
        "v2" => v2_payloads,
        // maw-js stacks current v1 fleet-token headers under auth-version=v3 when
        // from-signing is also present. Accept the current body-less fleet
        // signature first, then v2 for future body-bound fleet-token senders.
        "v3" => v1_payloads.into_iter().chain(v2_payloads).collect(),
        _ => return Err("unsupported-auth-version".to_owned()),
    };

    for (who, expected) in payloads {
        if verify_hmac_sig(workspace_key, &expected, signature) {
            let from = header_trimmed(&parts.headers, "x-maw-from");
            return Ok(Some(if from.is_empty() {
                who.to_owned()
            } else {
                format!("{who}:{from}")
            }));
        }
    }
    Err("signature-invalid".to_owned())
}

fn from_verify_candidate_paths(path: &str) -> Vec<String> {
    let primary = if path.is_empty() { "/" } else { path };
    let mut paths = vec![primary.to_owned()];
    if primary.starts_with('/') && primary != "/api" && !primary.starts_with("/api/") {
        let api_prefixed = format!("/api{primary}");
        if !paths.iter().any(|path| path == &api_prefixed) {
            paths.push(api_prefixed);
        }
    }
    paths
}

fn verify_cached_from_sign(
    parts: &RequestAuthParts,
    signed: &SignedInput,
    body_hash: &str,
) -> Result<Option<String>, String> {
    let Some(cached) = parts
        .cached_pubkey
        .as_deref()
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };

    if signed.has_v3_sig {
        let Some(signed_at_sec) = parse_unix_seconds(&signed.v3_timestamp) else {
            return Err("invalid-timestamp".to_owned());
        };
        if (parts.now - signed_at_sec).abs() > WINDOW_SEC {
            return Err("timestamp-out-of-window".to_owned());
        }
        for path in from_verify_candidate_paths(&parts.path) {
            let payload = build_from_sign_payload(
                &signed.from,
                signed_at_sec,
                &parts.method,
                &path,
                body_hash,
            );
            if verify_hmac_sig(cached, &payload, &signed.v3_sig) {
                return Ok(Some(format!("from-sign:{}", signed.from)));
            }
        }
        return Err("pin-mismatch".to_owned());
    }

    if signed.signed {
        let Some(signed_at_sec) = parse_iso_seconds(&signed.legacy_signed_at) else {
            return Err("invalid-signed-at".to_owned());
        };
        if (parts.now - signed_at_sec).abs() > WINDOW_SEC {
            return Err("timestamp-out-of-window".to_owned());
        }
        for path in from_verify_candidate_paths(&parts.path) {
            let payload = build_legacy_from_sign_payload(
                &signed.from,
                &signed.legacy_signed_at,
                &parts.method,
                &path,
                body_hash,
            );
            if verify_hmac_sig(cached, &payload, &signed.legacy_sig) {
                return Ok(Some(format!("from-sign:{}", signed.from)));
            }
        }
        return Err("pin-mismatch".to_owned());
    }

    if !signed.from.is_empty() {
        return Err("cache-no-sig".to_owned());
    }
    Ok(None)
}

fn ed25519_signature_header(headers: &Headers) -> Option<&str> {
    [
        "x-maw-ed25519-signature",
        "x-maw-signature-ed25519",
        "x-maw-from-signature-ed25519",
    ]
    .into_iter()
    .find_map(|name| headers.get(name).map(str::trim).filter(|value| !value.is_empty()))
}

fn ed25519_pubkey_header(headers: &Headers) -> Option<&str> {
    [
        "x-maw-ed25519-pubkey",
        "x-maw-pubkey",
        "x-maw-peer-pubkey",
    ]
    .into_iter()
    .find_map(|name| headers.get(name).map(str::trim).filter(|value| !value.is_empty()))
}

fn verify_from_request(args: &VerifyRequestArgs) -> FromVerifyDecision {
    let signed = signed_input(&args.headers);
    let cached = args
        .cached_pubkey
        .as_deref()
        .filter(|value| !value.is_empty());

    let Some(cached) = cached else {
        return if signed.signed {
            FromVerifyDecision::AcceptTofuRecord {
                reason: "no-cache-signed".to_owned(),
                from: signed.from.clone(),
            }
        } else {
            FromVerifyDecision::AcceptLegacy {
                reason: "no-cache-no-sig".to_owned(),
            }
        };
    };

    if !signed.signed {
        return FromVerifyDecision::RefuseUnsigned {
            reason: "cache-no-sig".to_owned(),
            from: (!signed.from.is_empty()).then_some(signed.from.clone()),
        };
    }

    let Some(signed_at_sec) = signed_at_seconds(&signed) else {
        return malformed(if signed.has_v3_sig {
            "invalid-timestamp"
        } else {
            "invalid-signed-at"
        });
    };
    let delta = (args.now - signed_at_sec).abs();
    if delta > WINDOW_SEC {
        return FromVerifyDecision::RefuseSkew {
            reason: "timestamp-out-of-window".to_owned(),
            from: signed.from,
            delta,
        };
    }

    let body_hash = hash_body(args.body.as_deref());
    let signature = if signed.has_v3_sig {
        &signed.v3_sig
    } else {
        &signed.legacy_sig
    };
    let mut verified = false;
    for path in from_verify_candidate_paths(&args.path) {
        let payload = if signed.has_v3_sig {
            build_from_sign_payload(
                &signed.from,
                signed_at_sec,
                &args.method,
                &path,
                &body_hash,
            )
        } else {
            build_legacy_from_sign_payload(
                &signed.from,
                &signed.legacy_signed_at,
                &args.method,
                &path,
                &body_hash,
            )
        };
        if verify_hmac_sig(cached, &payload, signature) {
            verified = true;
            break;
        }
    }
    if !verified {
        return FromVerifyDecision::RefuseMismatch {
            reason: "signature-invalid".to_owned(),
            from: signed.from,
        };
    }
    FromVerifyDecision::AcceptVerified {
        reason: "cache-sig-valid".to_owned(),
        from: signed.from,
    }
}


impl VerifyRequestInput for VerifyRequestArgs {
    type Decision = FromVerifyDecision;

    fn verify_request_input(&self) -> Self::Decision {
        verify_from_request(self)
    }
}

impl VerifyRequestInput for RequestAuthParts {
    type Decision = RequestAuthDecision;

    fn verify_request_input(&self) -> Self::Decision {
        verify_serve_request(self)
    }
}

#[must_use]
pub fn verify_request<T: VerifyRequestInput + ?Sized>(input: &T) -> T::Decision {
    input.verify_request_input()
}

fn verify_serve_request(parts: &RequestAuthParts) -> RequestAuthDecision {
    if parts.peer_ip.is_some_and(|ip| ip.is_loopback()) {
        return RequestAuthDecision::Accept {
            who: "loopback".to_owned(),
        };
    }

    let signed = signed_input(&parts.headers);
    if signed.from.is_empty() && (signed.has_v3_sig || ed25519_signature_header(&parts.headers).is_some()) {
        return RequestAuthDecision::Reject {
            reason: "missing-from".to_owned(),
        };
    }
    if !signed.has_v3_sig && !signed.from.is_empty() && ed25519_signature_header(&parts.headers).is_some() {
        return verify_ed25519_serve_request(parts, &signed);
    }

    let body_hash = hash_body(parts.body.as_deref());
    let fleet_accept = match verify_workspace_hmac(parts, &body_hash) {
        Ok(accept) => accept,
        Err(reason) => return auth_reject(&reason),
    };
    let from_accept = match verify_cached_from_sign(parts, &signed, &body_hash) {
        Ok(accept) => accept,
        Err(reason) => return auth_reject(&reason),
    };

    if let Some(who) = from_accept {
        return RequestAuthDecision::Accept { who };
    }
    if let Some(who) = fleet_accept {
        return RequestAuthDecision::Accept { who };
    }

    RequestAuthDecision::Reject {
        reason: if signed.signed {
            "pin-missing".to_owned()
        } else {
            "missing-credentials".to_owned()
        },
    }
}


fn verify_ed25519_serve_request(
    parts: &RequestAuthParts,
    signed: &SignedInput,
) -> RequestAuthDecision {
    let Some(signed_at_sec) = parse_unix_seconds(&signed.v3_timestamp) else {
        return auth_reject("invalid-timestamp");
    };
    if (parts.now - signed_at_sec).abs() > WINDOW_SEC {
        return auth_reject("timestamp-out-of-window");
    }
    let Some(signature_hex) = ed25519_signature_header(&parts.headers) else {
        return auth_reject("missing-credentials");
    };
    let key_hex = match ed25519_select_pubkey(parts, &signed.from) {
        Ok(key) => key,
        Err(reason) => return auth_reject(reason),
    };
    let body_hash = hash_body(parts.body.as_deref());
    let mut verified = false;
    for path in from_verify_candidate_paths(&parts.path) {
        let payload = build_from_sign_payload(
            &signed.from,
            signed_at_sec,
            &parts.method,
            &path,
            &body_hash,
        );
        if verify_ed25519_signature(&key_hex, payload.as_bytes(), signature_hex) {
            verified = true;
            break;
        }
    }
    if !verified {
        return auth_reject("ed25519-signature-invalid");
    }
    if let Err(reason) = ed25519_pin_verified_key(parts, &signed.from, &key_hex) {
        return auth_reject(reason);
    }
    RequestAuthDecision::Accept {
        who: format!("ed25519:{}", signed.from),
    }
}

fn ed25519_select_pubkey(parts: &RequestAuthParts, from: &str) -> Result<String, &'static str> {
    let observed = ed25519_pubkey_header(&parts.headers).map(str::to_owned);
    if let Some(pins) = &parts.ed25519_pins {
        let guard = pins.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if guard.is_poisoned() {
            return Err("tofu-store-corrupt");
        }
        if let Some(pinned) = guard.pinned(from) {
            if observed.as_deref().is_some_and(|key| key != pinned) {
                return Err("ed25519-pin-mismatch");
            }
            return Ok(pinned.to_owned());
        }
        return observed.ok_or("ed25519-pin-missing");
    }
    parts
        .cached_pubkey
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or("ed25519-pin-missing")
}

fn ed25519_pin_verified_key(
    parts: &RequestAuthParts,
    from: &str,
    key_hex: &str,
) -> Result<(), &'static str> {
    let Some(pins) = &parts.ed25519_pins else {
        return Ok(());
    };
    let mut guard = pins.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if guard.is_poisoned() {
        return Err("tofu-store-corrupt");
    }
    if let Some(pinned) = guard.pinned(from) {
        if pinned == key_hex {
            return Ok(());
        }
        return Err("ed25519-pin-mismatch");
    }
    if guard.pin_first_contact(from, key_hex) {
        Ok(())
    } else {
        Err("ed25519-pin-mismatch")
    }
}

fn verify_ed25519_signature(key_hex: &str, payload: &[u8], signature_hex: &str) -> bool {
    let Some(key_bytes) = hex_to_array::<32>(key_hex) else {
        return false;
    };
    let Some(signature_bytes) = hex_to_array::<64>(signature_hex) else {
        return false;
    };
    let Ok(key) = ed25519_dalek::VerifyingKey::from_bytes(&key_bytes) else {
        return false;
    };
    let Ok(signature) = ed25519_dalek::Signature::try_from(signature_bytes.as_slice()) else {
        return false;
    };
    key.verify_strict(payload, &signature).is_ok()
}

fn hex_to_array<const N: usize>(raw: &str) -> Option<[u8; N]> {
    let value = raw.trim();
    if value.len() != N * 2 || !value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0_u8; N];
    for (index, byte) in out.iter_mut().enumerate() {
        let start = index * 2;
        *byte = u8::from_str_radix(&value[start..start + 2], 16).ok()?;
    }
    Some(out)
}

fn auth_reject(reason: &str) -> RequestAuthDecision {
    RequestAuthDecision::Reject {
        reason: reason.to_owned(),
    }
}

#[must_use]
pub fn is_protected(path: &str, method: &str) -> bool {
    let method = method.to_ascii_uppercase();
    let normalized = auth_normalize_protected_path(path);
    matches!(
        (method.as_str(), normalized.as_str()),
        ("POST", "/triggers/fire" | "/worktrees/cleanup" | "/orchestration/workon" | "/trust" | "/trust/revoke")
            | ("GET", "/trust")
    ) || (method == "POST" && normalized.starts_with("/plugins/"))
}

fn auth_normalize_protected_path(path: &str) -> String {
    let path_only = path.split('?').next().unwrap_or(path);
    let normalized = path_only.strip_prefix("/api").unwrap_or(path_only);
    if normalized.is_empty() {
        "/".to_owned()
    } else {
        normalized.to_owned()
    }
}

#[must_use]
pub fn is_refuse_decision(decision: &FromVerifyDecision) -> bool {
    decision.kind().starts_with("refuse-")
}

fn malformed(reason: &str) -> FromVerifyDecision {
    FromVerifyDecision::RefuseMalformed {
        reason: reason.to_owned(),
    }
}

fn parse_unix_seconds(raw: &str) -> Option<i64> {
    if raw.is_empty() || !raw.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    raw.parse().ok()
}

fn consent_error(error: impl Into<String>) -> ConsentApprovalResult {
    ConsentApprovalResult {
        ok: false,
        error: Some(error.into()),
        entry: None,
    }
}

fn pair_api_probe_error(status: u16, error: &str) -> PairApiProbeResult {
    PairApiProbeResult {
        status,
        ok: false,
        error: Some(error.to_owned()),
        node: None,
    }
}

fn pair_api_accept_error(status: u16, error: &str) -> PairApiAcceptResult {
    PairApiAcceptResult {
        status,
        ok: false,
        error: Some(error.to_owned()),
        node: None,
        url: None,
        federation_token: None,
    }
}

fn pair_api_status_error(status: u16, error: &str) -> PairApiStatusResult {
    PairApiStatusResult {
        status,
        ok: false,
        error: Some(error.to_owned()),
        consumed: None,
        remote_node: None,
        remote_url: None,
    }
}

fn pair_api_auto_error(status: u16, error: &str) -> PairApiAutoResult {
    PairApiAutoResult {
        status,
        ok: false,
        error: Some(error.to_owned()),
        node: None,
        oracle: None,
        url: None,
        pubkey: None,
        proof: None,
        one_way: None,
        add_alias: None,
        add_url: None,
        add_node: None,
        add_pubkey: None,
        add_identity_oracle: None,
        add_identity_node: None,
        mark_symmetric_check: false,
    }
}

fn consent_status_str(status: ConsentStatus) -> &'static str {
    match status {
        ConsentStatus::Pending => "pending",
        ConsentStatus::Approved => "approved",
        ConsentStatus::Rejected => "rejected",
        ConsentStatus::Expired => "expired",
    }
}

fn iso_from_unix_millis(ms: i64) -> String {
    let seconds = ms.div_euclid(1_000);
    let millis = ms.rem_euclid(1_000);
    let days = seconds.div_euclid(86_400);
    let seconds_of_day = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).expect("civil month fits u32"),
        u32::try_from(day).expect("civil day fits u32"),
    )
}

fn parse_iso_seconds(iso: &str) -> Option<i64> {
    parse_iso_millis(iso).map(|millis| millis / 1_000)
}

fn parse_iso_millis(iso: &str) -> Option<i64> {
    let (date, time) = iso.split_once('T')?;
    let time = time.strip_suffix('Z').unwrap_or(time);
    let mut date_parts = date.split('-');
    let year = date_parts.next().unwrap_or_default().parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next().unwrap_or_default().parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let sec_part = time_parts.next()?;
    let (second, millis) = parse_second_millis(sec_part)?;
    if date_parts.next().is_some() || time_parts.next().is_some() {
        return None;
    }
    timestamp_seconds(year, month, day, hour, minute, second).map(|seconds| {
        seconds
            .saturating_mul(1_000)
            .saturating_add(i64::from(millis))
    })
}

fn parse_second_millis(sec_part: &str) -> Option<(u32, u16)> {
    let (second, fraction) = sec_part.split_once('.').unwrap_or((sec_part, ""));
    let second = second.parse::<u32>().ok()?;
    let mut value = 0_u16;
    let mut count = 0_u8;
    for ch in fraction.chars().take(3) {
        let digit = u16::try_from(ch.to_digit(10)?).expect("decimal digit fits u16");
        value = (value * 10) + digit;
        count += 1;
    }
    let millis = match count {
        0 => 0,
        1 => value * 100,
        2 => value * 10,
        _ => value,
    };
    Some((second, millis))
}

fn timestamp_seconds(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<i64> {
    if !(1..=12).contains(&month) || hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let leap_feb = if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 {
        29
    } else {
        28
    };
    let month_lengths = [31, leap_feb, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let max_day = month_lengths[usize::try_from(month - 1).expect("validated month fits usize")];
    if day == 0 || day > max_day {
        return None;
    }
    let year = i64::from(year) - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(
        (era * 146_097 + doe - 719_468) * 86_400
            + i64::from(hour) * 3_600
            + i64::from(minute) * 60
            + i64::from(second),
    )
}

fn hmac_sha256_hex(secret: &str, payload: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    hex_lower(&mac.finalize().into_bytes())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        out.push(char::from(HEX[usize::from(byte >> 4)]));
        out.push(char::from(HEX[usize::from(byte & 0x0f)]));
    }
    out
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0_u8;
    for (&left, &right) in a.iter().zip(b) {
        diff |= left ^ right;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::{
        consent_status_str, constant_time_eq, from_verify_candidate_paths, parse_iso_millis,
        parse_second_millis, sign_hmac_sig, timestamp_seconds, verify_auto_pair_proof,
        verify_hmac_sig, AutoPairIdentity, ConsentStatus,
    };

    #[test]
    fn auth_is_protected_matches_serve_daemon_surface() {
        assert!(super::is_protected("/triggers/fire", "POST"));
        assert!(super::is_protected("/api/triggers/fire", "post"));
        assert!(super::is_protected("/api/worktrees/cleanup?dry=1", "POST"));
        assert!(super::is_protected("/api/orchestration/workon", "POST"));
        assert!(super::is_protected("/api/plugins/reload", "POST"));
        assert!(!super::is_protected("/api/plugins", "GET"));
        assert!(!super::is_protected("/api/identity", "GET"));
        assert!(!super::is_protected("/api/triggers", "GET"));
    }

    #[test]
    fn from_verify_candidate_paths_add_api_variant_without_double_prefix() {
        assert_eq!(
            from_verify_candidate_paths("/send"),
            vec!["/send".to_owned(), "/api/send".to_owned()]
        );
        assert_eq!(
            from_verify_candidate_paths("/api/send"),
            vec!["/api/send".to_owned()]
        );
        assert_eq!(
            from_verify_candidate_paths("/"),
            vec!["/".to_owned(), "/api/".to_owned()]
        );
        assert_eq!(
            from_verify_candidate_paths("relative"),
            vec!["relative".to_owned()]
        );
    }

    #[test]
    fn private_helpers_cover_unreachable_public_edges() {
        assert_eq!(consent_status_str(ConsentStatus::Pending), "pending");
        assert_eq!(consent_status_str(ConsentStatus::Approved), "approved");
        assert_eq!(consent_status_str(ConsentStatus::Rejected), "rejected");
        assert_eq!(consent_status_str(ConsentStatus::Expired), "expired");
        assert!(!constant_time_eq(b"short", b"longer"));
        assert_eq!(
            super::iso_from_unix_millis(-62_167_219_200_001),
            "-001-12-31T23:59:59.999Z"
        );
    }

    #[test]
    fn private_timestamp_and_hmac_edges_are_reachable() {
        assert!(!verify_hmac_sig("s", "p", ""));
        assert!(!verify_hmac_sig("s", "p", "not-hex"));
        assert!(!verify_hmac_sig("s", "p", "0"));
        assert!(!verify_auto_pair_proof(
            &AutoPairIdentity {
                oracle: "o".to_owned(),
                node: "n".to_owned(),
                url: "u".to_owned(),
                pubkey: "p".to_owned(),
            },
            "token",
            "",
        ));
        let hmac = sign_hmac_sig("s", "p");
        assert!(verify_hmac_sig("s", "p", &hmac));
        let identity = AutoPairIdentity {
            oracle: "o".to_owned(),
            node: "n".to_owned(),
            url: "u".to_owned(),
            pubkey: "p".to_owned(),
        };
        let proof = super::sign_auto_pair_proof(&identity, "token");
        assert!(verify_auto_pair_proof(&identity, "token", &proof));
        assert!(!verify_auto_pair_proof(
            &AutoPairIdentity {
                oracle: "o".to_owned(),
                node: "n".to_owned(),
                url: "u".to_owned(),
                pubkey: "p".to_owned(),
            },
            "token",
            &"z".repeat(64),
        ));

        assert_eq!(parse_iso_millis("2024-01-01T00:00:00"), Some(1_704_067_200_000));
        assert_eq!(parse_second_millis("07.1"), Some((7, 100)));
        assert_eq!(parse_second_millis("07.12"), Some((7, 120)));
        assert_eq!(parse_second_millis("07.1239"), Some((7, 123)));
        assert_eq!(parse_second_millis("07.x"), None);

        for invalid in [
            "",
            "2024-01-01",
            "bad-01-01T00:00:00Z",
            "2024-bad-01T00:00:00Z",
            "2024-01-badT00:00:00Z",
            "2024-01-01Tbad:00:00Z",
            "2024-01-01T00:bad:00Z",
            "2024-01-01T00:00",
            "2024-01-01T00:00:badZ",
            "2024-01-01-extraT00:00:00Z",
            "2024-01-01T00:00:00:extraZ",
            "2024-01-01T24:00:00Z",
            "2024-01-01T00:60:00Z",
            "2024-01-01T00:00:60Z",
            "2024-02-30T00:00:00Z",
        ] {
            assert_eq!(parse_iso_millis(invalid), None, "{invalid}");
        }

        assert_eq!(timestamp_seconds(2000, 2, 29, 0, 0, 0), Some(951_782_400));
        assert_eq!(timestamp_seconds(1900, 2, 29, 0, 0, 0), None);
        assert_eq!(timestamp_seconds(2024, 13, 0, 0, 0, 0), None);
    }
}
