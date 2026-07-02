fn render_consent_approval_plan_json(
    mode: &str,
    result: &ConsentApprovalResult,
    pending_status: &str,
    trusted: bool,
) -> String {
    format!(
        "{{\"command\":\"consent-approval\",\"mode\":{},\"ok\":{},\"error\":{},\"pin\":null,\"entry\":{},\"pendingStatus\":{},\"trusted\":{}}}\n",
        json_string(mode),
        result.ok,
        json_optional_string(result.error.as_deref()),
        render_trust_entry_json(result.entry.as_ref()),
        json_string(pending_status),
        trusted
    )
}

fn render_trust_entry_json(entry: Option<&TrustEntry>) -> String {
    entry.map_or_else(|| "null".to_owned(), |entry| {
        format!(
            "{{\"from\":{},\"to\":{},\"action\":{},\"approvedAt\":{},\"approvedBy\":{},\"requestId\":{}}}",
            json_string(&entry.from),
            json_string(&entry.to),
            json_string(entry.action.as_str()),
            json_string(&entry.approved_at),
            json_string(approved_by_name(entry.approved_by)),
            json_optional_string(entry.request_id.as_deref())
        )
    })
}

fn approved_by_name(approved_by: ApprovedBy) -> &'static str {
    match approved_by {
        ApprovedBy::Human => "human",
        ApprovedBy::Auto => "auto",
    }
}

fn render_consent_approval_plan_text(
    mode: &str,
    result: &ConsentApprovalResult,
    pending_status: &str,
    trusted: bool,
) -> String {
    format!(
        "consent-approval mode={mode} ok={} pendingStatus={pending_status} trusted={trusted}\n",
        result.ok
    )
}

fn consent_approval_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_approval_usage()),
    }
}

fn consent_approval_usage() -> &'static str {
    "usage: maw-rs consent-approval <approve|reject> --request-id <id> --from <from> --to <to> --action <hey|team-invite|plugin-install> --summary <summary> --pin <pin> --created-at <ms> --now <ms> [--seed-pin <pin>] [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_consent_store_plan(argv: &[String]) -> CliOutput {
    let Some(mode) = argv.first().map(String::as_str) else {
        return consent_store_usage_error("consent-store: expected trust or pending");
    };
    if mode != "trust" && mode != "pending" {
        return consent_store_usage_error("consent-store: expected trust or pending");
    }

    let mut store = ConsentStore::default();
    let mut plan_json = false;
    let mut check = None::<(String, String, ConsentAction)>;
    let mut key = None::<(String, String, ConsentAction)>;
    let mut set_status = None::<(String, ConsentStatus)>;

    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--entry" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_store_usage_error("consent-store: missing --entry value");
                };
                match parse_consent_store_trust_entry(value) {
                    Ok(entry) => store.record_trust(entry),
                    Err(message) => return consent_store_usage_error(&message),
                }
                index += 1;
            }
            "--request" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_store_usage_error("consent-store: missing --request value");
                };
                match parse_consent_store_pending_request(value) {
                    Ok(request) => store.write_pending(request),
                    Err(message) => return consent_store_usage_error(&message),
                }
                index += 1;
            }
            "--check" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_store_usage_error("consent-store: missing --check value");
                };
                match parse_consent_store_key(value) {
                    Ok(parsed) => check = Some(parsed),
                    Err(message) => return consent_store_usage_error(&message),
                }
                index += 1;
            }
            "--key" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_store_usage_error("consent-store: missing --key value");
                };
                match parse_consent_store_key(value) {
                    Ok(parsed) => key = Some(parsed),
                    Err(message) => return consent_store_usage_error(&message),
                }
                index += 1;
            }
            "--set-status" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_store_usage_error("consent-store: missing --set-status value");
                };
                match parse_consent_store_status_update(value) {
                    Ok(parsed) => set_status = Some(parsed),
                    Err(message) => return consent_store_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return consent_store_usage_error(&format!("consent-store: unknown argument {arg}"))
            }
        }
        index += 1;
    }

    if mode == "trust" {
        let trusted = check
            .as_ref()
            .map(|(from, to, action)| store.is_trusted(from, to, *action));
        let trust_key_value = key
            .as_ref()
            .map(|(from, to, action)| trust_key(from, to, *action));
        let entries = store.list_trust();
        return CliOutput {
            code: 0,
            stdout: if plan_json {
                render_consent_store_trust_plan_json(trusted, trust_key_value.as_deref(), &entries)
            } else {
                render_consent_store_trust_plan_text(trusted, trust_key_value.as_deref())
            },
            stderr: String::new(),
        };
    }

    let updated = set_status
        .as_ref()
        .map(|(id, status)| store.update_status(id, *status));
    let entries = store.list_pending();
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_store_pending_plan_json(updated, &entries)
        } else {
            render_consent_store_pending_plan_text(updated)
        },
        stderr: String::new(),
    }
}

fn parse_consent_store_trust_entry(value: &str) -> Result<TrustEntry, String> {
    let fields = parse_consent_store_fields(value)?;
    let from = required_consent_store_field(&fields, "from")?;
    let to = required_consent_store_field(&fields, "to")?;
    let action = parse_consent_store_action(&required_consent_store_field(&fields, "action")?)?;
    let approved_at = required_consent_store_field(&fields, "approved_at")?;
    let approved_by = parse_approved_by(&required_consent_store_field(&fields, "approved_by")?)?;
    let request_id = fields.get("request_id").cloned();
    Ok(TrustEntry {
        from,
        to,
        action,
        approved_at,
        approved_by,
        request_id,
    })
}

fn parse_consent_store_pending_request(value: &str) -> Result<PendingRequest, String> {
    let fields = parse_consent_store_fields(value)?;
    let id = required_consent_store_field(&fields, "id")?;
    let from = required_consent_store_field(&fields, "from")?;
    let to = required_consent_store_field(&fields, "to")?;
    let action = parse_consent_store_action(&required_consent_store_field(&fields, "action")?)?;
    let summary = required_consent_store_field(&fields, "summary")?;
    let pin_hash = required_consent_store_field(&fields, "pin_hash")?;
    let created_at = required_consent_store_field(&fields, "created_at")?;
    let expires_at = required_consent_store_field(&fields, "expires_at")?;
    let status = parse_consent_status(&required_consent_store_field(&fields, "status")?)?;
    Ok(PendingRequest {
        id,
        from,
        to,
        action,
        summary,
        pin_hash,
        created_at,
        expires_at,
        status,
    })
}

fn parse_consent_store_fields(value: &str) -> Result<BTreeMap<String, String>, String> {
    let mut fields = BTreeMap::new();
    for part in value.split(',') {
        let Some((key, field_value)) = part.split_once('=') else {
            return Err("consent-store: expected key=value fields".to_owned());
        };
        if key.is_empty() {
            return Err("consent-store: expected non-empty field name".to_owned());
        }
        fields.insert(key.to_owned(), field_value.to_owned());
    }
    Ok(fields)
}

fn required_consent_store_field(
    fields: &BTreeMap<String, String>,
    name: &str,
) -> Result<String, String> {
    fields
        .get(name)
        .filter(|value| !value.is_empty())
        .cloned()
        .ok_or_else(|| format!("consent-store: missing {name}"))
}

fn parse_consent_store_key(value: &str) -> Result<(String, String, ConsentAction), String> {
    let mut parts = value.split(':');
    let from = parts.next().filter(|part| !part.is_empty());
    let to = parts.next().filter(|part| !part.is_empty());
    let action = parts.next().filter(|part| !part.is_empty());
    if parts.next().is_some() || from.is_none() || to.is_none() || action.is_none() {
        return Err("consent-store: key must use from:to:action".to_owned());
    }
    Ok((
        from.expect("checked").to_owned(),
        to.expect("checked").to_owned(),
        parse_consent_store_action(action.expect("checked"))?,
    ))
}

fn parse_consent_store_status_update(value: &str) -> Result<(String, ConsentStatus), String> {
    let Some((id, status)) = value.split_once(':') else {
        return Err("consent-store: --set-status must use id:status".to_owned());
    };
    if id.is_empty() {
        return Err("consent-store: --set-status missing id".to_owned());
    }
    Ok((id.to_owned(), parse_consent_status(status)?))
}

fn parse_consent_store_action(value: &str) -> Result<ConsentAction, String> {
    parse_consent_action(value).map_err(|_| "consent-store: invalid action".to_owned())
}

fn parse_approved_by(value: &str) -> Result<ApprovedBy, String> {
    match value {
        "human" => Ok(ApprovedBy::Human),
        "auto" => Ok(ApprovedBy::Auto),
        _ => Err("consent-store: invalid approved_by".to_owned()),
    }
}

fn parse_consent_status(value: &str) -> Result<ConsentStatus, String> {
    match value {
        "pending" => Ok(ConsentStatus::Pending),
        "approved" => Ok(ConsentStatus::Approved),
        "rejected" => Ok(ConsentStatus::Rejected),
        "expired" => Ok(ConsentStatus::Expired),
        _ => Err("consent-store: invalid status".to_owned()),
    }
}

fn render_consent_store_trust_plan_json(
    trusted: Option<bool>,
    trust_key_value: Option<&str>,
    entries: &[TrustEntry],
) -> String {
    let trusted = trusted.map_or_else(|| "null".to_owned(), |value| value.to_string());
    format!(
        "{{\"command\":\"consent-store\",\"mode\":\"trust\",\"trusted\":{trusted},\"trustKey\":{},\"entries\":{}}}\n",
        json_optional_string(trust_key_value),
        render_trust_entries_json(entries)
    )
}

fn render_consent_store_pending_plan_json(
    updated: Option<bool>,
    entries: &[PendingRequest],
) -> String {
    let updated = updated.map_or_else(|| "null".to_owned(), |value| value.to_string());
    format!(
        "{{\"command\":\"consent-store\",\"mode\":\"pending\",\"updated\":{updated},\"entries\":{}}}\n",
        render_pending_requests_json(entries)
    )
}

fn render_trust_entries_json(entries: &[TrustEntry]) -> String {
    let mut output = String::from("[");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&render_trust_entry_json(Some(entry)));
    }
    output.push(']');
    output
}

fn render_pending_requests_json(entries: &[PendingRequest]) -> String {
    let mut output = String::from("[");
    for (index, entry) in entries.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&render_pending_request_json(Some(entry)));
    }
    output.push(']');
    output
}

fn render_consent_store_trust_plan_text(
    trusted: Option<bool>,
    trust_key_value: Option<&str>,
) -> String {
    format!(
        "consent-store trust trusted={} trustKey={}\n",
        trusted.map_or_else(|| "-".to_owned(), |value| value.to_string()),
        trust_key_value.unwrap_or("-")
    )
}

fn render_consent_store_pending_plan_text(updated: Option<bool>) -> String {
    format!(
        "consent-store pending updated={}\n",
        updated.map_or_else(|| "-".to_owned(), |value| value.to_string())
    )
}

fn consent_store_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_store_usage()),
    }
}

fn consent_store_usage() -> &'static str {
    "usage: maw-rs consent-store <trust|pending> [--entry <from=...,to=...,action=...,approved_at=...,approved_by=...>]... [--request <id=...,from=...,to=...,action=...,summary=...,pin_hash=...,created_at=...,expires_at=...,status=...>]... [--check <from:to:action>] [--key <from:to:action>] [--set-status <id:status>] [--plan-json]"
}

fn run_consent_expiry_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut request = None::<PendingRequest>;
    let mut now_ms = None::<i64>;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--request" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_expiry_usage_error("consent-expiry: missing --request value");
                };
                match parse_consent_store_pending_request(value) {
                    Ok(parsed) => request = Some(parsed),
                    Err(message) => return consent_expiry_usage_error(&message),
                }
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_expiry_usage_error("consent-expiry: missing --now value");
                };
                match parse_i64_arg(value, "consent-expiry: --now") {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(message) => return consent_expiry_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return consent_expiry_usage_error(&format!(
                    "consent-expiry: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(request) = request else {
        return consent_expiry_usage_error("consent-expiry: missing --request value");
    };
    let Some(now_ms) = now_ms else {
        return consent_expiry_usage_error("consent-expiry: missing --now value");
    };
    let after = apply_consent_expiry(&request, now_ms);
    let expired = request.status != after.status && after.status == ConsentStatus::Expired;

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_expiry_plan_json(&request, &after, now_ms, expired)
        } else {
            format!(
                "consent-expiry id={} status={} expired={expired}\n",
                request.id,
                consent_status_name(after.status)
            )
        },
        stderr: String::new(),
    }
}

