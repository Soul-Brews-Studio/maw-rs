fn render_consent_pin_plan_json(
    normalized: &str,
    redacted: &str,
    valid: bool,
    pin_hash: &str,
    verified: Option<bool>,
) -> String {
    let verified = verified.map_or_else(|| "null".to_owned(), |value| value.to_string());
    format!(
        "{{\"command\":\"consent-pin\",\"pin\":null,\"normalized\":{},\"redacted\":{},\"valid\":{valid},\"hash\":{},\"verified\":{verified},\"requestId\":null}}\n",
        json_string(normalized),
        json_string(redacted),
        json_string(pin_hash)
    )
}

fn render_consent_pin_request_id_json(request_id: &str) -> String {
    format!(
        "{{\"command\":\"consent-pin\",\"pin\":null,\"normalized\":null,\"redacted\":null,\"valid\":null,\"hash\":null,\"verified\":null,\"requestId\":{}}}\n",
        json_string(request_id)
    )
}

fn render_consent_pin_plan_text(redacted: &str, valid: bool, verified: Option<bool>) -> String {
    match verified {
        Some(verified) => {
            format!("consent-pin redacted={redacted} valid={valid} verified={verified}\n")
        }
        None => format!("consent-pin redacted={redacted} valid={valid}\n"),
    }
}

fn consent_pin_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_pin_usage()),
    }
}

fn consent_pin_usage() -> &'static str {
    "usage: maw-rs consent-pin (--pin <pin> [--expected-hash <sha256>]|--request-id-bytes <b0,b1,...>) [--plan-json]"
}

fn run_consent_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return consent_constants_usage_error(&format!(
                    "consent-constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_constants_json()
        } else {
            "consent-constants actions=hey,team-invite,plugin-install statuses=pending,approved,rejected,expired approvedBy=human,auto\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_consent_constants_json() -> String {
    "{\"command\":\"consent-constants\",\"actions\":[\"hey\",\"team-invite\",\"plugin-install\"],\"statuses\":[\"pending\",\"approved\",\"rejected\",\"expired\"],\"approvedBy\":[\"human\",\"auto\"]}\n".to_owned()
}

fn consent_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_constants_usage()),
    }
}

fn consent_constants_usage() -> &'static str {
    "usage: maw-rs consent-constants [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_consent_request_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut from = None::<String>;
    let mut to = None::<String>;
    let mut action = None::<ConsentAction>;
    let mut summary = None::<String>;
    let mut peer_url = None::<String>;
    let mut request_id = None::<String>;
    let mut pin = None::<String>;
    let mut now_ms = None::<i64>;
    let mut peer_post = PeerPostResult::Skipped;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--from" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error("consent-request: missing --from value");
                };
                from = Some(value.to_owned());
                index += 1;
            }
            "--to" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error("consent-request: missing --to value");
                };
                to = Some(value.to_owned());
                index += 1;
            }
            "--action" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error("consent-request: missing --action value");
                };
                match parse_consent_action(value) {
                    Ok(parsed) => action = Some(parsed),
                    Err(message) => return consent_request_usage_error(&message),
                }
                index += 1;
            }
            "--summary" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error("consent-request: missing --summary value");
                };
                summary = Some(value.to_owned());
                index += 1;
            }
            "--peer-url" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error(
                        "consent-request: missing --peer-url value",
                    );
                };
                peer_url = Some(value.to_owned());
                index += 1;
            }
            "--request-id" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error(
                        "consent-request: missing --request-id value",
                    );
                };
                request_id = Some(value.to_owned());
                index += 1;
            }
            "--pin" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error("consent-request: missing --pin value");
                };
                pin = Some(value.to_owned());
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error("consent-request: missing --now value");
                };
                match parse_i64_arg(value, "consent-request: --now") {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(message) => return consent_request_usage_error(&message),
                }
                index += 1;
            }
            "--peer-ok" => peer_post = PeerPostResult::Ok,
            "--peer-http-status" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error(
                        "consent-request: missing --peer-http-status value",
                    );
                };
                match value.parse::<u16>() {
                    Ok(status) => peer_post = PeerPostResult::HttpStatus(status),
                    Err(_) => {
                        return consent_request_usage_error(
                            "consent-request: --peer-http-status must be u16",
                        )
                    }
                }
                index += 1;
            }
            "--peer-network-error" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_request_usage_error(
                        "consent-request: missing --peer-network-error value",
                    );
                };
                peer_post = PeerPostResult::NetworkError(value.to_owned());
                index += 1;
            }
            arg => {
                return consent_request_usage_error(&format!(
                    "consent-request: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(from) = from else {
        return consent_request_usage_error("consent-request: missing --from value");
    };
    let Some(to) = to else {
        return consent_request_usage_error("consent-request: missing --to value");
    };
    let Some(action) = action else {
        return consent_request_usage_error("consent-request: missing --action value");
    };
    let Some(summary) = summary else {
        return consent_request_usage_error("consent-request: missing --summary value");
    };
    let Some(request_id) = request_id else {
        return consent_request_usage_error("consent-request: missing --request-id value");
    };
    let Some(pin) = pin else {
        return consent_request_usage_error("consent-request: missing --pin value");
    };
    let Some(now_ms) = now_ms else {
        return consent_request_usage_error("consent-request: missing --now value");
    };

    let request_args = ConsentRequestArgs {
        from,
        to,
        action,
        summary,
        peer_url,
        request_id,
        pin: pin.clone(),
        now_ms,
        peer_post,
    };
    let mut store = ConsentStore::default();
    let result = request_consent_plan(&mut store, request_args);
    let pending = result
        .request_id
        .as_deref()
        .and_then(|request_id| store.read_pending(request_id));
    let pin_redacted = redact_pair_code(&pin);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_request_plan_json(&result, pending.as_ref(), &pin_redacted)
        } else {
            render_consent_request_plan_text(&result, &pin_redacted)
        },
        stderr: String::new(),
    }
}

fn parse_consent_action(value: &str) -> Result<ConsentAction, String> {
    match value {
        "hey" => Ok(ConsentAction::Hey),
        "team-invite" => Ok(ConsentAction::TeamInvite),
        "plugin-install" => Ok(ConsentAction::PluginInstall),
        _ => Err("consent-request: invalid --action value".to_owned()),
    }
}

fn render_consent_request_plan_json(
    result: &ConsentRequestResult,
    pending: Option<&PendingRequest>,
    pin_redacted: &str,
) -> String {
    format!(
        "{{\"command\":\"consent-request\",\"ok\":{},\"requestId\":{},\"pin\":null,\"pinRedacted\":{},\"expiresAt\":{},\"error\":{},\"alreadyTrusted\":{},\"peerUrl\":{},\"peerMethod\":{},\"peerBody\":{},\"pending\":{}}}\n",
        result.ok,
        json_optional_string(result.request_id.as_deref()),
        json_string(pin_redacted),
        json_optional_string(result.expires_at.as_deref()),
        json_optional_string(result.error.as_deref()),
        result.already_trusted,
        json_optional_string(result.peer_url.as_deref()),
        json_optional_string(result.peer_method.as_deref()),
        render_peer_pending_request_json(result.peer_body.as_ref()),
        render_pending_request_json(pending)
    )
}

fn render_consent_request_plan_text(result: &ConsentRequestResult, pin_redacted: &str) -> String {
    format!(
        "consent-request ok={} requestId={} pin={} peerUrl={}\n",
        result.ok,
        result.request_id.as_deref().unwrap_or("-"),
        pin_redacted,
        result.peer_url.as_deref().unwrap_or("-")
    )
}

fn render_peer_pending_request_json(request: Option<&PeerPendingRequest>) -> String {
    request.map_or_else(|| "null".to_owned(), |request| {
        format!(
            "{{\"id\":{},\"from\":{},\"to\":{},\"action\":{},\"summary\":{},\"pinHash\":{},\"createdAt\":{},\"expiresAt\":{},\"status\":{},\"pin\":null}}",
            json_string(&request.id),
            json_string(&request.from),
            json_string(&request.to),
            json_string(request.action.as_str()),
            json_string(&request.summary),
            json_string(&request.pin_hash),
            json_string(&request.created_at),
            json_string(&request.expires_at),
            json_string(consent_status_name(request.status))
        )
    })
}

fn render_pending_request_json(request: Option<&PendingRequest>) -> String {
    request.map_or_else(|| "null".to_owned(), |request| {
        format!(
            "{{\"id\":{},\"from\":{},\"to\":{},\"action\":{},\"summary\":{},\"pinHash\":{},\"createdAt\":{},\"expiresAt\":{},\"status\":{}}}",
            json_string(&request.id),
            json_string(&request.from),
            json_string(&request.to),
            json_string(request.action.as_str()),
            json_string(&request.summary),
            json_string(&request.pin_hash),
            json_string(&request.created_at),
            json_string(&request.expires_at),
            json_string(consent_status_name(request.status))
        )
    })
}

fn consent_status_name(status: maw_auth::ConsentStatus) -> &'static str {
    match status {
        maw_auth::ConsentStatus::Pending => "pending",
        maw_auth::ConsentStatus::Approved => "approved",
        maw_auth::ConsentStatus::Rejected => "rejected",
        maw_auth::ConsentStatus::Expired => "expired",
    }
}

fn json_optional_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

fn consent_request_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_request_usage()),
    }
}

fn consent_request_usage() -> &'static str {
    "usage: maw-rs consent-request --from <from> --to <to> --action <hey|team-invite|plugin-install> --summary <summary> --request-id <id> --pin <pin> --now <ms> [--peer-url <url>] [--peer-ok|--peer-http-status <status>|--peer-network-error <message>] [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_consent_approval_plan(argv: &[String]) -> CliOutput {
    let Some(mode) = argv.first().map(String::as_str) else {
        return consent_approval_usage_error("consent-approval: expected approve or reject");
    };
    if mode != "approve" && mode != "reject" {
        return consent_approval_usage_error("consent-approval: expected approve or reject");
    }

    let mut plan_json = false;
    let mut request_id = None::<String>;
    let mut from = None::<String>;
    let mut to = None::<String>;
    let mut action = None::<ConsentAction>;
    let mut summary = None::<String>;
    let mut pin = None::<String>;
    let mut seed_pin = "ABCDEF".to_owned();
    let mut created_at_ms = None::<i64>;
    let mut now_ms = None::<i64>;

    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--request-id" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error(
                        "consent-approval: missing --request-id value",
                    );
                };
                request_id = Some(value.to_owned());
                index += 1;
            }
            "--from" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error("consent-approval: missing --from value");
                };
                from = Some(value.to_owned());
                index += 1;
            }
            "--to" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error("consent-approval: missing --to value");
                };
                to = Some(value.to_owned());
                index += 1;
            }
            "--action" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error(
                        "consent-approval: missing --action value",
                    );
                };
                match parse_consent_action(value) {
                    Ok(parsed) => action = Some(parsed),
                    Err(_) => {
                        return consent_approval_usage_error(
                            "consent-approval: invalid --action value",
                        )
                    }
                }
                index += 1;
            }
            "--summary" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error(
                        "consent-approval: missing --summary value",
                    );
                };
                summary = Some(value.to_owned());
                index += 1;
            }
            "--pin" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error("consent-approval: missing --pin value");
                };
                pin = Some(value.to_owned());
                index += 1;
            }
            "--seed-pin" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error(
                        "consent-approval: missing --seed-pin value",
                    );
                };
                value.clone_into(&mut seed_pin);
                index += 1;
            }
            "--created-at" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error(
                        "consent-approval: missing --created-at value",
                    );
                };
                match parse_i64_arg(value, "consent-approval: --created-at") {
                    Ok(parsed) => created_at_ms = Some(parsed),
                    Err(message) => return consent_approval_usage_error(&message),
                }
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_approval_usage_error("consent-approval: missing --now value");
                };
                match parse_i64_arg(value, "consent-approval: --now") {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(message) => return consent_approval_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return consent_approval_usage_error(&format!(
                    "consent-approval: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(request_id) = request_id else {
        return consent_approval_usage_error("consent-approval: missing --request-id value");
    };
    let Some(from) = from else {
        return consent_approval_usage_error("consent-approval: missing --from value");
    };
    let Some(to) = to else {
        return consent_approval_usage_error("consent-approval: missing --to value");
    };
    let Some(action) = action else {
        return consent_approval_usage_error("consent-approval: missing --action value");
    };
    let Some(summary) = summary else {
        return consent_approval_usage_error("consent-approval: missing --summary value");
    };
    let Some(pin) = pin else {
        return consent_approval_usage_error("consent-approval: missing --pin value");
    };
    let Some(created_at_ms) = created_at_ms else {
        return consent_approval_usage_error("consent-approval: missing --created-at value");
    };
    let Some(now_ms) = now_ms else {
        return consent_approval_usage_error("consent-approval: missing --now value");
    };

    let mut store = ConsentStore::default();
    request_consent_plan(
        &mut store,
        ConsentRequestArgs {
            from: from.clone(),
            to: to.clone(),
            action,
            summary,
            peer_url: None,
            request_id: request_id.clone(),
            pin: seed_pin,
            now_ms: created_at_ms,
            peer_post: PeerPostResult::Skipped,
        },
    );

    let result = if mode == "approve" {
        approve_consent_plan(&mut store, &request_id, &pin, now_ms)
    } else {
        reject_consent_plan(&mut store, &request_id)
    };
    let pending_status = store
        .read_pending(&request_id)
        .map_or("missing", |request| consent_status_name(request.status));
    let trusted = store.is_trusted(&from, &to, action);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_approval_plan_json(mode, &result, pending_status, trusted)
        } else {
            render_consent_approval_plan_text(mode, &result, pending_status, trusted)
        },
        stderr: String::new(),
    }
}

