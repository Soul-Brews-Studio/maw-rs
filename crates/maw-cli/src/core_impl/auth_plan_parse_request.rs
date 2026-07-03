fn parse_auth_hmac_sign_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut secret = None;
    let mut payload = None;
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
            other => return Err(format!("auth hmac-sign: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::HmacSign {
        plan_json,
        secret: secret.ok_or_else(|| "auth hmac-sign: --secret is required".to_owned())?,
        payload: payload.ok_or_else(|| "auth hmac-sign: --payload is required".to_owned())?,
    })
}

fn parse_auth_constants_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => return Err(format!("auth constants: unknown argument {other}")),
        }
    }
    Ok(AuthPlanAction::Constants { plan_json })
}

fn parse_auth_sign_v3_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut common = AuthCommonArgs {
        plan_json: false,
        method: "GET".to_owned(),
        path: "/".to_owned(),
        timestamp: 0,
        body: None,
    };
    let mut peer_key = None;
    let mut from_address = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => common.plan_json = true,
            "--peer-key" => {
                peer_key = Some(take_auth_value(argv, index, "--peer-key")?);
                index += 1;
            }
            "--from" => {
                from_address = Some(take_auth_value(argv, index, "--from")?);
                index += 1;
            }
            "--method" => {
                common.method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                common.path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                common.timestamp = parse_i64_arg(&raw, "auth: --now")?;
                index += 1;
            }
            "--body" => {
                common.body = Some(take_auth_value(argv, index, "--body")?);
                index += 1;
            }
            other => return Err(format!("auth sign-v3: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::SignV3 {
        plan_json: common.plan_json,
        peer_key: peer_key.ok_or_else(|| "auth sign-v3: --peer-key is required".to_owned())?,
        from_address: from_address.ok_or_else(|| "auth sign-v3: --from is required".to_owned())?,
        method: common.method,
        path: common.path,
        timestamp: common.timestamp,
        body: common.body,
    })
}

fn parse_auth_loopback_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut address = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--address" => {
                address = Some(take_auth_value(argv, index, "--address")?);
                index += 1;
            }
            other => return Err(format!("auth loopback: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::Loopback {
        plan_json,
        address: address.ok_or_else(|| "auth loopback: --address is required".to_owned())?,
    })
}

fn parse_auth_from_address_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut oracle = None;
    let mut node = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--oracle" => {
                oracle = Some(take_auth_value(argv, index, "--oracle")?);
                index += 1;
            }
            "--node" => {
                node = Some(take_auth_value(argv, index, "--node")?);
                index += 1;
            }
            other => return Err(format!("auth from-address: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::FromAddress {
        plan_json,
        oracle,
        node: node.ok_or_else(|| "auth from-address: --node is required".to_owned())?,
    })
}

fn parse_auth_hash_body_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut plan_json = false;
    let mut body = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--body" => {
                body = Some(take_auth_value(argv, index, "--body")?);
                index += 1;
            }
            other => return Err(format!("auth hash-body: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::HashBody { plan_json, body })
}

fn parse_auth_verify_args(argv: &[String]) -> Result<AuthPlanAction, String> {
    let mut common = AuthCommonArgs {
        plan_json: false,
        method: "GET".to_owned(),
        path: "/".to_owned(),
        timestamp: 0,
        body: None,
    };
    let mut cached_pubkey = None;
    let mut headers = Vec::new();
    let mut peer_ip = None;
    let mut workspace_key_env = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => common.plan_json = true,
            "--method" => {
                common.method = take_auth_value(argv, index, "--method")?;
                index += 1;
            }
            "--path" => {
                common.path = take_auth_value(argv, index, "--path")?;
                index += 1;
            }
            "--now" => {
                let raw = take_auth_value(argv, index, "--now")?;
                common.timestamp = parse_i64_arg(&raw, "auth: --now")?;
                index += 1;
            }
            "--body" => {
                common.body = Some(take_auth_value(argv, index, "--body")?);
                index += 1;
            }
            "--cached-pubkey" => {
                cached_pubkey = Some(take_auth_value(argv, index, "--cached-pubkey")?);
                index += 1;
            }
            "--peer-ip" => {
                let raw = take_auth_value(argv, index, "--peer-ip")?;
                peer_ip = Some(
                    raw.parse::<IpAddr>()
                        .map_err(|_| "auth verify-request: --peer-ip must be an IP address".to_owned())?,
                );
                index += 1;
            }
            "--workspace-key-env" => {
                let raw = take_auth_value(argv, index, "--workspace-key-env")?;
                auth_validate_env_name(&raw)?;
                workspace_key_env = Some(raw);
                index += 1;
            }
            "--header" => {
                let raw = take_auth_value(argv, index, "--header")?;
                let Some((name, value)) = raw.split_once('=') else {
                    return Err("auth verify-request: --header must be key=value".to_owned());
                };
                headers.push((name.to_owned(), value.to_owned()));
                index += 1;
            }
            other => return Err(format!("auth verify-request: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(AuthPlanAction::VerifyRequest {
        plan_json: common.plan_json,
        method: common.method,
        path: common.path,
        timestamp: common.timestamp,
        body: common.body,
        cached_pubkey,
        headers,
        peer_ip,
        workspace_key_env,
    })
}

fn auth_validate_env_name(name: &str) -> Result<(), String> {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return Err("auth verify-request: --workspace-key-env must not be empty".to_owned());
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err("auth verify-request: --workspace-key-env must be an env var name".to_owned());
    }
    if chars.any(|ch| !(ch == '_' || ch.is_ascii_alphanumeric())) {
        return Err("auth verify-request: --workspace-key-env must be an env var name".to_owned());
    }
    Ok(())
}

fn take_auth_value(argv: &[String], index: usize, name: &str) -> Result<String, String> {
    argv.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("auth: missing {name} value"))
}

fn parse_i64_arg(value: &str, name: &str) -> Result<i64, String> {
    value
        .parse::<i64>()
        .map_err(|_| format!("{name} must be an integer"))
}

fn render_auth_sign_v1_json(
    method: &str,
    path: &str,
    timestamp: i64,
    body_hash: &str,
    signature: &str,
) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"sign-v1\",\"method\":{},\"path\":{},\"timestamp\":{timestamp},\"bodyHash\":{},\"signature\":{}}}\n",
        json_string(method),
        json_string(path),
        json_string(body_hash),
        json_string(signature)
    )
}

fn render_auth_sign_headers_json(
    method: &str,
    path: &str,
    timestamp: i64,
    body_hash: &str,
    headers: &Headers,
) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"sign-headers\",\"method\":{},\"path\":{},\"timestamp\":{timestamp},\"bodyHash\":{},\"headers\":{{{}}}}}\n",
        json_string(method),
        json_string(path),
        json_string(body_hash),
        render_auth_header_fields(headers).join(",")
    )
}

#[allow(clippy::too_many_arguments)]
fn render_auth_verify_v1_json(
    method: &str,
    path: &str,
    signed_at: i64,
    now: i64,
    delta: i64,
    body_hash: &str,
    signature: &str,
    valid: bool,
    reason: &str,
) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"verify-v1\",\"method\":{},\"path\":{},\"signedAt\":{signed_at},\"now\":{now},\"deltaSec\":{delta},\"windowSec\":{WINDOW_SEC},\"bodyHash\":{},\"signature\":{},\"valid\":{valid},\"reason\":{}}}\n",
        json_string(method),
        json_string(path),
        json_string(body_hash),
        json_string(signature),
        json_string(reason)
    )
}

fn render_auth_headers_text(headers: &Headers) -> String {
    let mut out = String::new();
    for (key, value) in auth_rendered_headers(headers) {
        out.push_str(&key);
        out.push_str(": ");
        out.push_str(&value);
        out.push('\n');
    }
    out
}

fn render_auth_header_fields(headers: &Headers) -> Vec<String> {
    auth_rendered_headers(headers)
        .into_iter()
        .map(|(key, value)| format!("{}:{}", json_string(&key), json_string(&value)))
        .collect()
}

fn auth_rendered_headers(headers: &Headers) -> Vec<(String, String)> {
    let header_map = headers.to_btree_map();
    [
        ("x-maw-auth-version", "X-Maw-Auth-Version"),
        ("x-maw-from", "X-Maw-From"),
        ("x-maw-signature", "X-Maw-Signature"),
        ("x-maw-signature-v3", "X-Maw-Signature-V3"),
        ("x-maw-timestamp", "X-Maw-Timestamp"),
    ]
    .into_iter()
    .filter_map(|(key, rendered)| {
        header_map
            .get(key)
            .map(|value| (rendered.to_owned(), value.clone()))
    })
    .collect()
}

fn render_auth_sign_v3_json(
    method: &str,
    path: &str,
    timestamp: i64,
    from_address: &str,
    signature: &str,
    body_hash: &str,
    headers: &Headers,
) -> String {
    let header_fields = render_auth_header_fields(headers);
    format!(
        "{{\"command\":\"auth\",\"kind\":\"sign-v3\",\"method\":{},\"path\":{},\"timestamp\":{timestamp},\"from\":{},\"signature\":{},\"bodyHash\":{},\"headers\":{{{}}}}}\n",
        json_string(method),
        json_string(path),
        json_string(from_address),
        json_string(signature),
        json_string(body_hash),
        header_fields.join(",")
    )
}

fn render_auth_loopback_json(address: &str, loopback: bool) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"loopback\",\"address\":{},\"loopback\":{loopback}}}\n",
        json_string(address)
    )
}

fn render_auth_from_address_json(oracle: Option<&str>, node: &str, from: &str) -> String {
    let oracle_json = oracle.map_or_else(|| "null".to_owned(), json_string);
    format!(
        "{{\"command\":\"auth\",\"kind\":\"from-address\",\"oracle\":{oracle_json},\"node\":{},\"from\":{}}}\n",
        json_string(node),
        json_string(from)
    )
}

fn render_auth_hash_body_json(present: bool, body_hash: &str) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"hash-body\",\"present\":{present},\"bodyHash\":{}}}\n",
        json_string(body_hash)
    )
}

fn render_auth_verify_json(decision: &FromVerifyDecision) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"verify-request\",\"decision\":{{{}}}}}\n",
        render_auth_decision_fields(decision).join(",")
    )
}

fn render_auth_verify_d2_json(decision: &RequestAuthDecision) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"verify-request\",\"mode\":\"d2\",\"decision\":{{{}}}}}\n",
        render_auth_request_decision_fields(decision).join(",")
    )
}

fn render_auth_request_decision_fields(decision: &RequestAuthDecision) -> Vec<String> {
    match decision {
        RequestAuthDecision::Accept { who } => vec![
            "\"kind\":\"accept\"".to_owned(),
            format!("\"who\":{}", json_string(who)),
        ],
        RequestAuthDecision::Reject { reason } => vec![
            "\"kind\":\"reject\"".to_owned(),
            format!("\"reason\":{}", json_string(reason)),
        ],
    }
}

fn render_auth_verify_legacy_from_json(
    method: &str,
    path: &str,
    now: i64,
    from: &str,
    signed_at: &str,
    decision: &FromVerifyDecision,
) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"verify-legacy-from\",\"method\":{},\"path\":{},\"now\":{now},\"from\":{},\"signedAt\":{},\"decision\":{{{}}}}}\n",
        json_string(method),
        json_string(path),
        json_string(from),
        json_string(signed_at),
        render_auth_decision_fields(decision).join(",")
    )
}

fn render_auth_verify_v3_from_json(
    method: &str,
    path: &str,
    now: i64,
    from: &str,
    timestamp: i64,
    decision: &FromVerifyDecision,
) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"verify-v3-from\",\"method\":{},\"path\":{},\"now\":{now},\"from\":{},\"timestamp\":{timestamp},\"decision\":{{{}}}}}\n",
        json_string(method),
        json_string(path),
        json_string(from),
        render_auth_decision_fields(decision).join(",")
    )
}

