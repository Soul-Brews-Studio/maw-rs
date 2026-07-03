fn run_auth_loopback(plan_json: bool, address: &str) -> CliOutput {
    let loopback = is_loopback(Some(address));
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_loopback_json(address, loopback)
        } else {
            format!("{loopback}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_from_address(plan_json: bool, oracle: Option<&str>, node: &str) -> CliOutput {
    let from = resolve_from_address(&FromAddressConfig {
        oracle: oracle.map(str::to_owned),
        node: Some(node.to_owned()),
    })
    .expect("parser requires node for auth from-address");
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_from_address_json(oracle, node, &from)
        } else {
            format!("{from}\n")
        },
        stderr: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auth_verify_request(
    plan_json: bool,
    method: String,
    path: String,
    timestamp: i64,
    body: Option<String>,
    cached_pubkey: Option<String>,
    headers: Vec<(String, String)>,
    peer_ip: Option<IpAddr>,
    workspace_key_env: Option<String>,
) -> CliOutput {
    if peer_ip.is_some() || workspace_key_env.is_some() {
        return run_auth_verify_request_d2(
            plan_json,
            method,
            path,
            timestamp,
            body,
            cached_pubkey,
            headers,
            peer_ip,
            workspace_key_env,
        );
    }
    let decision = verify_request(&VerifyRequestArgs {
        method,
        path,
        headers: Headers::new(headers),
        body: body.map(std::string::String::into_bytes),
        cached_pubkey,
        now: timestamp,
    });
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_verify_json(&decision)
        } else {
            format!("{}\n", decision.kind())
        },
        stderr: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auth_verify_request_d2(
    plan_json: bool,
    method: String,
    path: String,
    timestamp: i64,
    body: Option<String>,
    cached_pubkey: Option<String>,
    headers: Vec<(String, String)>,
    peer_ip: Option<IpAddr>,
    workspace_key_env: Option<String>,
) -> CliOutput {
    let workspace_key = workspace_key_env.and_then(|name| auth_read_workspace_key_env(&name));
    let headers = Headers::new(headers);
    let ed25519_pins = auth_ed25519_cli_pins(&headers, cached_pubkey.as_deref());
    let decision = verify_request(&RequestAuthParts {
        method,
        path,
        headers,
        body: body.map(std::string::String::into_bytes),
        peer_ip,
        workspace_key,
        cached_pubkey,
        ed25519_pins: Some(ed25519_pins),
        now: timestamp,
    });
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_verify_d2_json(&decision)
        } else {
            format!("{}\n", auth_request_decision_kind(&decision))
        },
        stderr: String::new(),
    }
}


fn auth_ed25519_cli_pins(
    headers: &Headers,
    cached_pubkey: Option<&str>,
) -> std::sync::Arc<std::sync::Mutex<Ed25519TofuStore>> {
    let mut store = Ed25519TofuStore::default();
    if headers.get("x-maw-ed25519-signature").is_some() {
        if let (Some(from), Some(pubkey)) = (headers.get("x-maw-from"), cached_pubkey) {
            let _ = store.pin_first_contact(from, pubkey);
        }
    }
    std::sync::Arc::new(std::sync::Mutex::new(store))
}

fn auth_read_workspace_key_env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|value| !value.is_empty())
}

fn auth_request_decision_kind(decision: &RequestAuthDecision) -> &'static str {
    match decision {
        RequestAuthDecision::Accept { .. } => "accept",
        RequestAuthDecision::Reject { .. } => "reject",
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auth_verify_legacy_from(
    plan_json: bool,
    cached_pubkey: Option<&str>,
    from: &str,
    signed_at: &str,
    signature: &str,
    method: &str,
    path: &str,
    now: i64,
    body: Option<String>,
) -> CliOutput {
    let decision = verify_request(&VerifyRequestArgs {
        method: method.to_owned(),
        path: path.to_owned(),
        headers: Headers::new([
            ("x-maw-from".to_owned(), from.to_owned()),
            ("x-maw-signed-at".to_owned(), signed_at.to_owned()),
            ("x-maw-signature".to_owned(), signature.to_owned()),
        ]),
        body: body.map(std::string::String::into_bytes),
        cached_pubkey: cached_pubkey.map(str::to_owned),
        now,
    });
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_verify_legacy_from_json(method, path, now, from, signed_at, &decision)
        } else {
            format!("{}\n", decision.kind())
        },
        stderr: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auth_verify_v3_from(
    plan_json: bool,
    cached_pubkey: Option<&str>,
    from: &str,
    timestamp: i64,
    signature_v3: &str,
    method: &str,
    path: &str,
    now: i64,
    body: Option<String>,
) -> CliOutput {
    let decision = verify_request(&VerifyRequestArgs {
        method: method.to_owned(),
        path: path.to_owned(),
        headers: Headers::new([
            ("x-maw-from".to_owned(), from.to_owned()),
            ("x-maw-timestamp".to_owned(), timestamp.to_string()),
            ("x-maw-signature-v3".to_owned(), signature_v3.to_owned()),
        ]),
        body: body.map(std::string::String::into_bytes),
        cached_pubkey: cached_pubkey.map(str::to_owned),
        now,
    });
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_verify_v3_from_json(method, path, now, from, timestamp, &decision)
        } else {
            format!("{}\n", decision.kind())
        },
        stderr: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auth_from_sign_payload(
    plan_json: bool,
    legacy: bool,
    from: &str,
    timestamp: Option<i64>,
    signed_at: Option<&str>,
    method: &str,
    path: &str,
    body_hash: &str,
) -> CliOutput {
    let method = method.to_uppercase();
    let payload = if legacy {
        build_legacy_from_sign_payload(
            from,
            signed_at.expect("parser requires --signed-at with --legacy"),
            &method,
            path,
            body_hash,
        )
    } else {
        build_from_sign_payload(
            from,
            timestamp.expect("parser requires --timestamp without --legacy"),
            &method,
            path,
            body_hash,
        )
    };
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_from_sign_payload_json(&AuthFromSignPayloadRender {
                legacy,
                from,
                timestamp,
                signed_at,
                method: &method,
                path,
                body_hash,
                payload: &payload,
            })
        } else {
            format!("{payload}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_hmac_verify(
    plan_json: bool,
    secret: &str,
    payload: &str,
    signature: &str,
) -> CliOutput {
    let malformed = signature.is_empty() || !signature.chars().all(|c| c.is_ascii_hexdigit());
    let valid = verify_hmac_sig(secret, payload, signature);
    let reason = if valid {
        "ok"
    } else if malformed {
        "signature-malformed"
    } else {
        "signature-mismatch"
    };
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_hmac_verify_json(payload, signature, valid, reason)
        } else {
            format!("{reason}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_hmac_sign(plan_json: bool, secret: &str, payload: &str) -> CliOutput {
    let signature = sign_hmac_sig(secret, payload);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_hmac_sign_json(payload, &signature)
        } else {
            format!("{signature}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_constants(plan_json: bool) -> CliOutput {
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_constants_json()
        } else {
            format!("defaultOracle={DEFAULT_ORACLE} windowSec={WINDOW_SEC}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_hash_body(plan_json: bool, body: Option<&str>) -> CliOutput {
    let body_hash = hash_body(body.map(str::as_bytes));
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_hash_body_json(body.is_some(), &body_hash)
        } else {
            format!("{body_hash}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_sign_headers(
    plan_json: bool,
    token: &str,
    method: &str,
    path: &str,
    timestamp: i64,
    body: Option<&str>,
) -> CliOutput {
    let body_hash = hash_body(body.map(str::as_bytes));
    let headers = sign_headers_at(token, method, path, body.map(str::as_bytes), timestamp);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_sign_headers_json(method, path, timestamp, &body_hash, &headers)
        } else {
            render_auth_headers_text(&headers)
        },
        stderr: String::new(),
    }
}

#[allow(clippy::too_many_arguments)]
fn run_auth_verify_v1(
    plan_json: bool,
    token: &str,
    method: &str,
    path: &str,
    signed_at: i64,
    now: i64,
    signature: &str,
    body_hash: &str,
) -> CliOutput {
    let delta = (now - signed_at).abs();
    let valid = verify(token, method, path, signed_at, signature, body_hash, now);
    let reason = if valid {
        "ok"
    } else if delta > WINDOW_SEC {
        "timestamp-out-of-window"
    } else {
        "signature-mismatch"
    };
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_verify_v1_json(
                method, path, signed_at, now, delta, body_hash, signature, valid, reason,
            )
        } else {
            format!("{reason}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_sign_v1(
    plan_json: bool,
    token: &str,
    method: &str,
    path: &str,
    timestamp: i64,
    body_hash: &str,
) -> CliOutput {
    let signature = sign(token, method, path, timestamp, body_hash);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auth_sign_v1_json(method, path, timestamp, body_hash, &signature)
        } else {
            format!("{signature}\n")
        },
        stderr: String::new(),
    }
}

fn run_auth_sign_v3(
    plan_json: bool,
    peer_key: &str,
    from_address: &str,
    method: &str,
    path: &str,
    timestamp: i64,
    body: Option<&str>,
) -> CliOutput {
    match sign_request_v3(
        peer_key,
        from_address,
        method,
        path,
        timestamp,
        body.map(str::as_bytes),
    ) {
        Ok(signature) => {
            let headers = sign_headers_v3_at(
                peer_key,
                from_address,
                method,
                path,
                body.map(str::as_bytes),
                timestamp,
            )
            .expect("sign_request_v3 succeeded with the same inputs");
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_auth_sign_v3_json(
                        method,
                        path,
                        timestamp,
                        from_address,
                        &signature.signature,
                        &signature.body_hash,
                        &headers,
                    )
                } else {
                    format!("{}\n", signature.signature)
                },
                stderr: String::new(),
            }
        }
        Err(message) => auth_usage_error(&message),
    }
}

enum AuthPlanAction {
    SignV1 {
        plan_json: bool,
        token: String,
        method: String,
        path: String,
        timestamp: i64,
        body_hash: String,
    },
    SignHeaders {
        plan_json: bool,
        token: String,
        method: String,
        path: String,
        timestamp: i64,
        body: Option<String>,
    },
    VerifyV1 {
        plan_json: bool,
        token: String,
        method: String,
        path: String,
        signed_at: i64,
        now: i64,
        signature: String,
        body_hash: String,
    },
    VerifyLegacyFrom {
        plan_json: bool,
        cached_pubkey: Option<String>,
        from: String,
        signed_at: String,
        signature: String,
        method: String,
        path: String,
        now: i64,
        body: Option<String>,
    },
    VerifyV3From {
        plan_json: bool,
        cached_pubkey: Option<String>,
        from: String,
        timestamp: i64,
        signature_v3: String,
        method: String,
        path: String,
        now: i64,
        body: Option<String>,
    },
    FromSignPayload {
        plan_json: bool,
        legacy: bool,
        from: String,
        timestamp: Option<i64>,
        signed_at: Option<String>,
        method: String,
        path: String,
        body_hash: String,
    },
    HmacVerify {
        plan_json: bool,
        secret: String,
        payload: String,
        signature: String,
    },
    HmacSign {
        plan_json: bool,
        secret: String,
        payload: String,
    },
    Constants {
        plan_json: bool,
    },
    SignV3 {
        plan_json: bool,
        peer_key: String,
        from_address: String,
        method: String,
        path: String,
        timestamp: i64,
        body: Option<String>,
    },
    Loopback {
        plan_json: bool,
        address: String,
    },
    FromAddress {
        plan_json: bool,
        oracle: Option<String>,
        node: String,
    },
    HashBody {
        plan_json: bool,
        body: Option<String>,
    },
    VerifyRequest {
        plan_json: bool,
        method: String,
        path: String,
        timestamp: i64,
        body: Option<String>,
        cached_pubkey: Option<String>,
        headers: Vec<(String, String)>,
        peer_ip: Option<IpAddr>,
        workspace_key_env: Option<String>,
    },
}

