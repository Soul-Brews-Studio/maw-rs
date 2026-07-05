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

fn parse_send_args(command: &str, argv: &[String]) -> Result<SendArgs, String> {
    let mut inbox = None;
    let mut from = None;
    let mut positional = Vec::new();
    let mut approve = false;
    let mut trust = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--inbox" => inbox = Some(true),
            "--no-inbox" => inbox = Some(false),
            "--approve" => approve = true,
            "--trust" => trust = true,
            "--from" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err(format!("{command}: missing --from value"));
                };
                from = Some(value.clone());
                index += 1;
            }
            value if value.starts_with("--from=") => {
                from = Some(value["--from=".len()..].to_owned());
            }
            value if value.starts_with('-') => return Err(format!("{command}: unknown argument {value}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if trust && !approve {
        return Err(format!("{command}: --trust requires --approve"));
    }
    if positional.len() < 2 {
        return Err(format!("{command}: target and message are required"));
    }
    Ok(SendArgs {
        target: positional[0].clone(),
        text: positional[1..].join(" "),
        inbox,
        from,
        approve,
        trust,
    })
}

fn send_usage_error(command: &str, message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs {command} <target> <message> [--inbox|--no-inbox] [--from <oracle:node>] [--approve] [--trust]\n"
        ),
    }
}

fn wake_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs wake <target> [--task <task>] [--from <oracle:node>]\n"
        ),
    }
}

fn send_local_message(
    command: &str,
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
    target: &str,
    text: &str,
    config: &HeyConfig,
    from: Option<&str>,
) -> CliOutput {
    let outbound = format_local_hey_message(text, config, from);
    if let Err(error) = tmux.send_keys_literal(target, &outbound) {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{command}: tmux send-keys failed: {error}\n"),
        };
    }
    if let Err(error) = tmux.send_enter(target) {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{command}: tmux send-enter failed: {error}\n"),
        };
    }
    CliOutput {
        code: 0,
        stdout: format!("delivered {target}\n"),
        stderr: String::new(),
    }
}

