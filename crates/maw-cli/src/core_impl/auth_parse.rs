struct AuthCommonArgs {
    plan_json: bool,
    method: String,
    path: String,
    timestamp: i64,
    body: Option<String>,
}

fn parse_auth_plan_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err(
            "auth: expected sign-v1, sign-headers, verify-v1, verify-legacy-from, verify-v3-from, from-sign-payload, hmac-sign, hmac-verify, constants, sign-v3, verify-request, loopback, from-address, or hash-body"
                .to_owned(),
        );
    };
    match kind {
        "sign-v1" => parse_auth_sign_v1_args(&argv[1..]),
        "sign-headers" => parse_auth_sign_headers_args(&argv[1..]),
        "verify-v1" => parse_auth_verify_v1_args(&argv[1..]),
        "verify-legacy-from" => parse_auth_verify_legacy_from_args(&argv[1..]),
        "verify-v3-from" => parse_auth_verify_v3_from_args(&argv[1..]),
        "from-sign-payload" => parse_auth_from_sign_payload_args(&argv[1..]),
        "hmac-sign" => parse_auth_hmac_sign_args(&argv[1..]),
        "hmac-verify" => parse_auth_hmac_verify_args(&argv[1..]),
        "constants" => parse_auth_constants_args(&argv[1..]),
        "sign-v3" => parse_auth_sign_v3_args(&argv[1..]),
        "verify-request" => parse_auth_verify_args(&argv[1..]),
        "loopback" => parse_auth_loopback_args(&argv[1..]),
        "from-address" => parse_auth_from_address_args(&argv[1..]),
        "hash-body" => parse_auth_hash_body_args(&argv[1..]),
        other => Err(format!("auth: unknown subcommand {other}")),
    }
}

fn parse_auth_sign_v1_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut token = None;
    let mut method = "GET".to_owned();
    let mut path = "/".to_owned();
    let mut timestamp = None;
    let mut body_hash = String::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--token" => {
                token = Some(take_auth_value(argv, index, "--token")?);
                index += 1;
            }
            "--method" => {
                method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                timestamp = Some(parse_i64_arg(&raw, "auth sign-v1: --now")?);
                index += 1;
            }
            "--body-hash" => {
                body_hash = take_auth_value(argv, index, "--body-hash")?;
                index += 1;
            }
            other => return Err(format!("auth sign-v1: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::SignV1 {
        plan_json,
        token: token.ok_or_else(|| "auth sign-v1: --token is required".to_owned())?,
        method,
        path,
        timestamp: timestamp.ok_or_else(|| "auth sign-v1: --now is required".to_owned())?,
        body_hash,
    })
}

fn parse_auth_sign_headers_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut token = None;
    let mut method = "GET".to_owned();
    let mut path = "/".to_owned();
    let mut timestamp = None;
    let mut body = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--token" => {
                token = Some(take_auth_value(argv, index, "--token")?);
                index += 1;
            }
            "--method" => {
                method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                timestamp = Some(parse_i64_arg(&raw, "auth sign-headers: --now")?);
                index += 1;
            }
            "--body" => {
                body = Some(take_auth_value(argv, index, "--body")?);
                index += 1;
            }
            other => return Err(format!("auth sign-headers: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::SignHeaders {
        plan_json,
        token: token.ok_or_else(|| "auth sign-headers: --token is required".to_owned())?,
        method,
        path,
        timestamp: timestamp.ok_or_else(|| "auth sign-headers: --now is required".to_owned())?,
        body,
    })
}

fn parse_auth_verify_v1_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut token = None;
    let mut method = "GET".to_owned();
    let mut path = "/".to_owned();
    let mut signed_at = None;
    let mut now = None;
    let mut signature = None;
    let mut body_hash = String::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--token" => {
                token = Some(take_auth_value(argv, index, "--token")?);
                index += 1;
            }
            "--method" => {
                method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--signed-at" => {
                let raw = take_auth_value(argv, index, "--signed-at")?;
                signed_at = Some(parse_i64_arg(&raw, "auth verify-v1: --signed-at")?);
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                now = Some(parse_i64_arg(&raw, "auth verify-v1: --now")?);
                index += 1;
            }
            "--signature" => {
                signature = Some(take_auth_value(argv, index, "--signature")?);
                index += 1;
            }
            "--body-hash" => {
                body_hash = take_auth_value(argv, index, "--body-hash")?;
                index += 1;
            }
            other => return Err(format!("auth verify-v1: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::VerifyV1 {
        plan_json,
        token: token.ok_or_else(|| "auth verify-v1: --token is required".to_owned())?,
        method,
        path,
        signature: signature.ok_or_else(|| "auth verify-v1: --signature is required".to_owned())?,
        signed_at: signed_at.ok_or_else(|| "auth verify-v1: --signed-at is required".to_owned())?,
        now: now.ok_or_else(|| "auth verify-v1: --now is required".to_owned())?,
        body_hash,
    })
}

fn parse_auth_verify_legacy_from_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut cached_pubkey = None;
    let mut from = None;
    let mut signed_at = None;
    let mut signature = None;
    let mut method = "GET".to_owned();
    let mut path = "/".to_owned();
    let mut now = None;
    let mut body = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--cached-pubkey" => {
                cached_pubkey = Some(take_auth_value(argv, index, "--cached-pubkey")?);
                index += 1;
            }
            "--from" => {
                from = Some(take_auth_value(argv, index, "--from")?);
                index += 1;
            }
            "--signed-at" => {
                signed_at = Some(take_auth_value(argv, index, "--signed-at")?);
                index += 1;
            }
            "--signature" => {
                signature = Some(take_auth_value(argv, index, "--signature")?);
                index += 1;
            }
            "--method" => {
                method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                now = Some(parse_i64_arg(&raw, "auth verify-legacy-from: --now")?);
                index += 1;
            }
            "--body" => {
                body = Some(take_auth_value(argv, index, "--body")?);
                index += 1;
            }
            other => return Err(format!("auth verify-legacy-from: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::VerifyLegacyFrom {
        plan_json,
        cached_pubkey,
        from: from.ok_or_else(|| "auth verify-legacy-from: --from is required".to_owned())?,
        signed_at: signed_at
            .ok_or_else(|| "auth verify-legacy-from: --signed-at is required".to_owned())?,
        signature: signature
            .ok_or_else(|| "auth verify-legacy-from: --signature is required".to_owned())?,
        method,
        path,
        now: now.ok_or_else(|| "auth verify-legacy-from: --now is required".to_owned())?,
        body,
    })
}

fn parse_auth_verify_v3_from_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut cached_pubkey = None;
    let mut from = None;
    let mut timestamp = None;
    let mut signature_v3 = None;
    let mut method = "GET".to_owned();
    let mut path = "/".to_owned();
    let mut now = None;
    let mut body = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--cached-pubkey" => {
                cached_pubkey = Some(take_auth_value(argv, index, "--cached-pubkey")?);
                index += 1;
            }
            "--from" => {
                from = Some(take_auth_value(argv, index, "--from")?);
                index += 1;
            }
            "--timestamp" => {
                let raw = take_auth_value(argv, index, "--timestamp")?;
                timestamp = Some(parse_i64_arg(&raw, "auth verify-v3-from: --timestamp")?);
                index += 1;
            }
            "--signature-v3" => {
                signature_v3 = Some(take_auth_value(argv, index, "--signature-v3")?);
                index += 1;
            }
            "--method" => {
                method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                now = Some(parse_i64_arg(&raw, "auth verify-v3-from: --now")?);
                index += 1;
            }
            "--body" => {
                body = Some(take_auth_value(argv, index, "--body")?);
                index += 1;
            }
            other => return Err(format!("auth verify-v3-from: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::VerifyV3From {
        plan_json,
        cached_pubkey,
        from: from.ok_or_else(|| "auth verify-v3-from: --from is required".to_owned())?,
        timestamp: timestamp
            .ok_or_else(|| "auth verify-v3-from: --timestamp is required".to_owned())?,
        signature_v3: signature_v3
            .ok_or_else(|| "auth verify-v3-from: --signature-v3 is required".to_owned())?,
        method,
        path,
        now: now.ok_or_else(|| "auth verify-v3-from: --now is required".to_owned())?,
        body,
    })
}

fn parse_auth_from_sign_payload_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut legacy = false;
    let mut from = None;
    let mut timestamp = None;
    let mut signed_at = None;
    let mut method = "GET".to_owned();
    let mut path = "/".to_owned();
    let mut body_hash = String::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--legacy" => legacy = true,
            "--from" => {
                from = Some(take_auth_value(argv, index, "--from")?);
                index += 1;
            }
            "--timestamp" => {
                let raw = take_auth_value(argv, index, "--timestamp")?;
                timestamp = Some(parse_i64_arg(&raw, "auth from-sign-payload: --timestamp")?);
                index += 1;
            }
            "--signed-at" => {
                signed_at = Some(take_auth_value(argv, index, "--signed-at")?);
                index += 1;
            }
            "--method" => {
                method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--body-hash" => {
                body_hash = take_auth_value(argv, index, "--body-hash")?;
                index += 1;
            }
            other => return Err(format!("auth from-sign-payload: unknown argument {other}")),
        }
        index += 1;
    }
    let from = from.ok_or_else(|| "auth from-sign-payload: --from is required".to_owned())?;
    if legacy {
        if signed_at.is_none() {
            return Err("auth from-sign-payload: --signed-at is required with --legacy".to_owned());
        }
    } else if timestamp.is_none() {
        return Err("auth from-sign-payload: --timestamp is required".to_owned());
    }
    Ok(AuthPlanAction::FromSignPayload {
        plan_json,
        legacy,
        from,
        timestamp,
        signed_at,
        method,
        path,
        body_hash,
    })
}

fn parse_auth_hmac_verify_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut secret = None;
    let mut payload = None;
    let mut signature = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--secret" => {
                secret = Some(take_auth_value(argv, index, "--secret")?);
                index += 1;
            }
            "--payload" => {
                payload = Some(take_auth_value(argv, index, "--payload")?);
                index += 1;
            }
            "--signature" => {
                signature = Some(take_auth_value(argv, index, "--signature")?);
                index += 1;
            }
            other => return Err(format!("auth hmac-verify: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::HmacVerify {
        plan_json,
        secret: secret.ok_or_else(|| "auth hmac-verify: --secret is required".to_owned())?,
        payload: payload.ok_or_else(|| "auth hmac-verify: --payload is required".to_owned())?,
        signature: signature
            .ok_or_else(|| "auth hmac-verify: --signature is required".to_owned())?,
    })
}

