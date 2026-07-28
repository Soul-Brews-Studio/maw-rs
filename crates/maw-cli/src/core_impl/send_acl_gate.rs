// May this sender put text in that pane at all?
//
// Every cross-scope send passes here before any transport is touched. A pair
// that is not in scope and not explicitly trusted does not fail and does not go
// through -- it is queued as a pending request for a human to approve, so the
// refusal is recoverable rather than a dead end. Loads scopes and trust pairs
// strictly: a corrupt ACL file must not silently widen access.

#[derive(Debug, Clone, PartialEq, Eq)]
enum SendAclGateResult {
    Proceed { stderr_prefix: String },
    Queued(CliOutput),
    Reject(CliOutput),
}

async fn gated_send_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
    acl_bypass: bool,
) -> CliOutput {
    gated_send_peer_message_with_audit(command, peer_url, target, "", args, config, sender_oracle, &[], acl_bypass).await
}

#[allow(clippy::too_many_arguments)]
async fn gated_send_peer_message_with_audit(
    command: &str,
    peer_url: &str,
    target: &str,
    node: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
    audit_args: &[String],
    acl_bypass: bool,
) -> CliOutput {
    match send_acl_gate_peer(command, target, args, sender_oracle, acl_bypass) {
        SendAclGateResult::Proceed { stderr_prefix } => {
            send_acl_deliver_peer_message_with_audit(
                command,
                peer_url,
                target,
                node,
                args,
                config,
                sender_oracle,
                audit_args,
                stderr_prefix,
            )
            .await
        }
        SendAclGateResult::Queued(output) | SendAclGateResult::Reject(output) => output,
    }
}

async fn send_acl_deliver_peer_message(
    command: &str,
    peer_url: &str,
    target: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
    stderr_prefix: String,
) -> CliOutput {
    send_acl_deliver_peer_message_with_audit(command, peer_url, target, "", args, config, sender_oracle, &[], stderr_prefix).await
}

#[allow(clippy::too_many_arguments)]
async fn send_acl_deliver_peer_message_with_audit(
    command: &str,
    peer_url: &str,
    target: &str,
    node: &str,
    args: &SendArgs,
    config: &HeyConfig,
    sender_oracle: &str,
    audit_args: &[String],
    stderr_prefix: String,
) -> CliOutput {
    send_acl_apply_proceed_stderr(
        send_peer_message(command, peer_url, target, node, args, config, sender_oracle, audit_args).await,
        &stderr_prefix,
    )
}

fn send_acl_apply_proceed_stderr(mut output: CliOutput, stderr_prefix: &str) -> CliOutput {
    if !stderr_prefix.is_empty() {
        output.stderr = format!("{stderr_prefix}{}", output.stderr);
    }
    output
}

fn send_acl_gate_peer(
    command: &str,
    target: &str,
    args: &SendArgs,
    sender_oracle: &str,
    acl_bypass: bool,
) -> SendAclGateResult {
    if args.trust && !args.approve {
        return SendAclGateResult::Reject(CliOutput {
            code: send_error_code(command),
            stdout: String::new(),
            stderr: format!("{command}: --trust requires --approve\n"),
        });
    }
    let sender = match send_acl_sender(args, sender_oracle) {
        Ok(sender) => sender,
        Err(message) => {
            return SendAclGateResult::Reject(CliOutput {
                code: send_error_code(command),
                stdout: String::new(),
                stderr: format!("{command}: {message}\n"),
            })
        }
    };
    let target = send_acl_actor_from_target(target);
    if args.approve || acl_bypass {
        let mut stderr_prefix = String::new();
        if args.approve && args.trust {
            if let Err(error) = scope_trust_add_to_path(&scope_trust_path(), &sender, &target, &inbox_iso_label(inbox_now_ms())) {
                let _ = writeln!(
                    stderr_prefix,
                    "warn: ACL trust add failed, allowing send: {error} — fix {}",
                    scope_trust_path().display()
                );
            }
        }
        return SendAclGateResult::Proceed { stderr_prefix };
    }
    let evaluation = match send_acl_evaluate_loaded(&sender, &target) {
        Ok(decision) => decision,
        Err(error) => {
            return SendAclGateResult::Proceed {
                stderr_prefix: format!("warn: ACL check failed, allowing send: {error}\n"),
            }
        }
    };
    match evaluation {
        ScopeAclDecision::Allow => SendAclGateResult::Proceed {
            stderr_prefix: String::new(),
        },
        ScopeAclDecision::Queue => match send_acl_queue_pending(&sender, &target, args) {
            Ok(output) => SendAclGateResult::Queued(output),
            Err(error) => SendAclGateResult::Proceed {
                stderr_prefix: format!("warn: ACL queue failed, allowing send: {error}\n"),
            },
        },
    }
}

fn send_acl_sender(args: &SendArgs, sender_oracle: &str) -> Result<String, String> {
    if let Some(explicit) = args.from.as_deref() {
        let wire = validate_wire_from(explicit)?;
        return send_acl_oracle_component(&wire);
    }
    send_acl_validate_actor(sender_oracle)
}

fn send_acl_oracle_component(wire_from: &str) -> Result<String, String> {
    let oracle = wire_from
        .split_once(':')
        .map_or(wire_from, |(oracle, _node)| oracle);
    send_acl_validate_actor(oracle)
}

fn send_acl_actor_from_target(target: &str) -> String {
    target
        .split_once(':')
        .map_or(target, |(oracle, _rest)| oracle)
        .to_owned()
}

fn send_acl_validate_actor(value: &str) -> Result<String, String> {
    scope_trust_validate_actor("ACL actor", value).map_err(|error| format!("ACL actor rejected: {error}"))
}

fn send_acl_evaluate_loaded(sender: &str, target: &str) -> Result<ScopeAclDecision, String> {
    let scopes = send_acl_load_scopes_strict()?;
    let trust = send_acl_load_trust_pairs_strict()?;
    if scopes.is_empty() {
        return Ok(ScopeAclDecision::Allow);
    }
    Ok(scope_acl_evaluate(sender, target, &scopes, &trust))
}

fn send_acl_load_scopes_strict() -> Result<Vec<ScopeNativeRecord>, String> {
    let dir = scope_native_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(Vec::new());
    };
    let mut scopes = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("ACL check failed, allowing send: read {}: {error} — fix {}", dir.display(), dir.display()))?;
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let body = std::fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error} — fix {}", path.display(), path.display()))?;
        let scope = serde_json::from_str::<ScopeNativeRecord>(&body)
            .map_err(|error| format!("parse {}: {error} — fix {}", path.display(), path.display()))?;
        scopes.push(scope);
    }
    scopes.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(scopes)
}

fn send_acl_load_trust_pairs_strict() -> Result<Vec<ScopeAclTrustPair>, String> {
    let path = scope_trust_path();
    let Ok(body) = std::fs::read_to_string(&path) else {
        return Ok(Vec::new());
    };
    let value = serde_json::from_str::<serde_json::Value>(&body)
        .map_err(|error| format!("parse {}: {error} — fix {}", path.display(), path.display()))?;
    let Some(items) = value.as_array() else {
        return Err(format!("parse {}: expected array — fix {}", path.display(), path.display()));
    };
    let mut entries = Vec::with_capacity(items.len());
    for item in items {
        let entry = scope_trust_entry_from_json(item)
            .ok_or_else(|| format!("parse {}: invalid trust entry — fix {}", path.display(), path.display()))?;
        entries.push(entry);
    }
    Ok(scope_trust_pairs(&entries))
}

fn send_acl_queue_pending(sender: &str, target: &str, args: &SendArgs) -> Result<CliOutput, String> {
    let env = inbox_real_env();
    let id = send_acl_pending_id()?;
    let message = InboxPendingMessage {
        id: id.clone(),
        sender: sender.to_owned(),
        target: target.to_owned(),
        query: Some(args.target.clone()),
        sent_at: inbox_iso_label(inbox_now_ms()),
        status: "pending".to_owned(),
        message: args.text.clone(),
    };
    inbox_write_pending(&inbox_state_pending_dir(&env), &message)?;
    Ok(CliOutput {
        code: 0,
        stdout: send_acl_format_queue_output(&id, sender, target),
        stderr: String::new(),
    })
}

fn send_acl_format_queue_output(id: &str, sender: &str, target: &str) -> String {
    format!(
        "queued pending ACL approval: {id}\n  sender: {sender}\n  target: {target}\n  review: maw inbox show-pending {id}\n  approve: maw inbox approve {id}\n"
    )
}

fn send_acl_pending_id() -> Result<String, String> {
    let suffix = send_acl_random_hex6().unwrap_or_else(|| {
        format!(
            "{:06x}",
            (current_epoch_seconds() ^ u64::from(std::process::id())) & 0x00ff_ffff
        )
    });
    inbox_pending_id(inbox_now_ms(), &suffix)
}

fn send_acl_random_hex6() -> Option<String> {
    let mut bytes = [0_u8; 3];
    let mut file = std::fs::File::open("/dev/urandom").ok()?;
    std::io::Read::read_exact(&mut file, &mut bytes).ok()?;
    Some(hex_bytes(&bytes))
}
