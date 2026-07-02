fn render_consent_expiry_plan_json(
    before: &PendingRequest,
    after: &PendingRequest,
    now_ms: i64,
    expired: bool,
) -> String {
    format!(
        "{{\"command\":\"consent-expiry\",\"now\":{now_ms},\"expired\":{expired},\"before\":{},\"after\":{}}}\n",
        render_pending_request_json(Some(before)),
        render_pending_request_json(Some(after))
    )
}

fn consent_expiry_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_expiry_usage()),
    }
}

fn consent_expiry_usage() -> &'static str {
    "usage: maw-rs consent-expiry --request <id=...,from=...,to=...,action=...,summary=...,pin_hash=...,created_at=...,expires_at=...,status=...> --now <ms> [--plan-json]"
}

fn run_consent_cleanup_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut delete_id = None::<String>;
    let mut store = ConsentStore::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--request" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_cleanup_usage_error("consent-cleanup: missing --request value");
                };
                match parse_consent_store_pending_request(value) {
                    Ok(request) => store.write_pending(request),
                    Err(message) => return consent_cleanup_usage_error(&message),
                }
                index += 1;
            }
            "--delete" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_cleanup_usage_error("consent-cleanup: missing --delete value");
                };
                if value.is_empty() {
                    return consent_cleanup_usage_error("consent-cleanup: missing --delete value");
                }
                delete_id = Some(value.to_owned());
                index += 1;
            }
            arg => {
                return consent_cleanup_usage_error(&format!(
                    "consent-cleanup: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(delete_id) = delete_id else {
        return consent_cleanup_usage_error("consent-cleanup: missing --delete value");
    };
    let deleted = store.delete_pending(&delete_id);
    let entries = store.list_pending();

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_cleanup_plan_json(&delete_id, deleted, &entries)
        } else {
            format!("consent-cleanup deletedId={delete_id} deleted={deleted}\n")
        },
        stderr: String::new(),
    }
}

fn render_consent_cleanup_plan_json(
    delete_id: &str,
    deleted: bool,
    entries: &[PendingRequest],
) -> String {
    format!(
        "{{\"command\":\"consent-cleanup\",\"deletedId\":{},\"deleted\":{deleted},\"entries\":{}}}\n",
        json_string(delete_id),
        render_pending_requests_json(entries)
    )
}

fn consent_cleanup_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_cleanup_usage()),
    }
}

fn consent_cleanup_usage() -> &'static str {
    "usage: maw-rs consent-cleanup --request <id=...,from=...,to=...,action=...,summary=...,pin_hash=...,created_at=...,expires_at=...,status=...>... --delete <id> [--plan-json]"
}

fn run_consent_trust_revoke_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut revoke = None::<(String, String, ConsentAction)>;
    let mut store = ConsentStore::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--entry" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_trust_revoke_usage_error(
                        "consent-trust-revoke: missing --entry value",
                    );
                };
                match parse_consent_store_trust_entry(value) {
                    Ok(entry) => store.record_trust(entry),
                    Err(message) => return consent_trust_revoke_usage_error(&message),
                }
                index += 1;
            }
            "--revoke" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_trust_revoke_usage_error(
                        "consent-trust-revoke: missing --revoke value",
                    );
                };
                match parse_consent_store_key(value) {
                    Ok(parsed) => revoke = Some(parsed),
                    Err(message) => return consent_trust_revoke_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return consent_trust_revoke_usage_error(&format!(
                    "consent-trust-revoke: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some((from, to, action)) = revoke else {
        return consent_trust_revoke_usage_error("consent-trust-revoke: missing --revoke value");
    };
    let revoked_key = trust_key(&from, &to, action);
    let revoked = store.remove_trust(&from, &to, action);
    let entries = store.list_trust();

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_trust_revoke_plan_json(&revoked_key, revoked, &entries)
        } else {
            format!("consent-trust-revoke revokedKey={revoked_key} revoked={revoked}\n")
        },
        stderr: String::new(),
    }
}

fn render_consent_trust_revoke_plan_json(
    revoked_key: &str,
    revoked: bool,
    entries: &[TrustEntry],
) -> String {
    format!(
        "{{\"command\":\"consent-trust-revoke\",\"revokedKey\":{},\"revoked\":{revoked},\"entries\":{}}}\n",
        json_string(revoked_key),
        render_trust_entries_json(entries)
    )
}

fn consent_trust_revoke_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_trust_revoke_usage()),
    }
}

fn consent_trust_revoke_usage() -> &'static str {
    "usage: maw-rs consent-trust-revoke [--entry <from=...,to=...,action=...,approved_at=...,approved_by=...>]... --revoke <from:to:action> [--plan-json]"
}

fn run_consent_trust_check_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut check = None::<(String, String, ConsentAction)>;
    let mut store = ConsentStore::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--entry" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_trust_check_usage_error(
                        "consent-trust-check: missing --entry value",
                    );
                };
                match parse_consent_store_trust_entry(value) {
                    Ok(entry) => store.record_trust(entry),
                    Err(message) => return consent_trust_check_usage_error(&message),
                }
                index += 1;
            }
            "--check" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_trust_check_usage_error(
                        "consent-trust-check: missing --check value",
                    );
                };
                match parse_consent_store_key(value) {
                    Ok(parsed) => check = Some(parsed),
                    Err(message) => return consent_trust_check_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return consent_trust_check_usage_error(&format!(
                    "consent-trust-check: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some((from, to, action)) = check else {
        return consent_trust_check_usage_error("consent-trust-check: missing --check value");
    };
    let trust_key_value = trust_key(&from, &to, action);
    let trusted = store.is_trusted(&from, &to, action);
    let entry = store
        .list_trust()
        .into_iter()
        .find(|entry| entry.from == from && entry.to == to && entry.action == action);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_trust_check_plan_json(&trust_key_value, trusted, entry.as_ref())
        } else {
            format!("consent-trust-check trustKey={trust_key_value} trusted={trusted}\n")
        },
        stderr: String::new(),
    }
}

fn render_consent_trust_check_plan_json(
    trust_key_value: &str,
    trusted: bool,
    entry: Option<&TrustEntry>,
) -> String {
    format!(
        "{{\"command\":\"consent-trust-check\",\"trustKey\":{},\"trusted\":{trusted},\"entry\":{}}}\n",
        json_string(trust_key_value),
        render_trust_entry_json(entry)
    )
}

fn consent_trust_check_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_trust_check_usage()),
    }
}

fn consent_trust_check_usage() -> &'static str {
    "usage: maw-rs consent-trust-check [--entry <from=...,to=...,action=...,approved_at=...,approved_by=...>]... --check <from:to:action> [--plan-json]"
}

fn run_consent_pending_read_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut id = None::<String>;
    let mut store = ConsentStore::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--request" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pending_read_usage_error(
                        "consent-pending-read: missing --request value",
                    );
                };
                match parse_consent_store_pending_request(value) {
                    Ok(request) => store.write_pending(request),
                    Err(message) => return consent_pending_read_usage_error(&message),
                }
                index += 1;
            }
            "--id" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pending_read_usage_error(
                        "consent-pending-read: missing --id value",
                    );
                };
                if value.is_empty() {
                    return consent_pending_read_usage_error(
                        "consent-pending-read: missing --id value",
                    );
                }
                id = Some(value.to_owned());
                index += 1;
            }
            arg => {
                return consent_pending_read_usage_error(&format!(
                    "consent-pending-read: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(id) = id else {
        return consent_pending_read_usage_error("consent-pending-read: missing --id value");
    };
    let request = store.read_pending(&id);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_pending_read_plan_json(&id, request.as_ref())
        } else {
            format!("consent-pending-read id={id} found={}\n", request.is_some())
        },
        stderr: String::new(),
    }
}

fn render_consent_pending_read_plan_json(id: &str, request: Option<&PendingRequest>) -> String {
    format!(
        "{{\"command\":\"consent-pending-read\",\"id\":{},\"found\":{},\"request\":{}}}\n",
        json_string(id),
        request.is_some(),
        render_pending_request_json(request)
    )
}

fn consent_pending_read_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_pending_read_usage()),
    }
}

fn consent_pending_read_usage() -> &'static str {
    "usage: maw-rs consent-pending-read [--request <id=...,from=...,to=...,action=...,summary=...,pin_hash=...,created_at=...,expires_at=...,status=...>]... --id <id> [--plan-json]"
}

fn run_consent_pending_status_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut set_status = None::<(String, ConsentStatus)>;
    let mut store = ConsentStore::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--request" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pending_status_usage_error(
                        "consent-pending-status: missing --request value",
                    );
                };
                match parse_consent_store_pending_request(value) {
                    Ok(request) => store.write_pending(request),
                    Err(message) => return consent_pending_status_usage_error(&message),
                }
                index += 1;
            }
            "--set-status" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pending_status_usage_error(
                        "consent-pending-status: missing --set-status value",
                    );
                };
                match parse_consent_store_status_update(value) {
                    Ok(parsed) => set_status = Some(parsed),
                    Err(message) => return consent_pending_status_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return consent_pending_status_usage_error(&format!(
                    "consent-pending-status: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some((id, status)) = set_status else {
        return consent_pending_status_usage_error(
            "consent-pending-status: missing --set-status value",
        );
    };
    let updated = store.update_status(&id, status);
    let request = store.read_pending(&id);
    let entries = store.list_pending();

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_pending_status_plan_json(&id, updated, request.as_ref(), &entries)
        } else {
            format!("consent-pending-status id={id} updated={updated}\n")
        },
        stderr: String::new(),
    }
}

