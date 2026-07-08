const DISPATCH_142: &[DispatcherEntry] = &[DispatcherEntry { command: "consent", handler: Handler::Sync(run_consent_command_135) }];

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentPendingRow135 {
    id: String,
    from: String,
    to: String,
    action: String,
    summary: String,
    created_at: String,
    expires_at: String,
    status: String,
}

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentTrustRow135 {
    from: String,
    to: String,
    action: String,
    approved_at: String,
}

#[derive(Debug, serde::Deserialize)]
struct ConsentTrustFile135 {
    #[serde(default)]
    trust: BTreeMap<String, ConsentTrustRow135>,
}

fn run_consent_command_135(argv: &[String]) -> CliOutput {
    match consent_run_135(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn consent_run_135(argv: &[String]) -> Result<String, String> {
    let sub = argv.first().map_or("list", String::as_str);
    match sub {
        "list" => {
            consent_expect_no_extra_args_135("list", argv, 1)?;
            Ok(format!("{}\n", consent_format_pending_135(&consent_read_pending_135())))
        }
        "list-trust" => {
            consent_expect_no_extra_args_135("list-trust", argv, 1)?;
            Ok(format!("{}\n", consent_format_trust_135(&consent_read_trust_135())))
        }
        "help" | "--help" | "-h" => {
            consent_expect_no_extra_args_135("help", argv, 1)?;
            Ok(format!("{}\n", consent_help_135()))
        }
        "approve" => consent_approve_native_135(argv),
        "reject" => consent_reject_native_135(argv),
        "trust" | "untrust" => Err(format!(
            "maw consent {sub} is not native in maw-rs ZERO-BUN B2; use a human-at-terminal consent command\n\n{}",
            consent_help_135()
        )),
        value if value.starts_with('-') => Err(format!("consent: unknown argument {value}\n\n{}", consent_help_135())),
        value => Err(format!("unknown subcommand: {value}\n\n{}", consent_help_135())),
    }
}

fn consent_approve_native_135(argv: &[String]) -> Result<String, String> {
    let (Some(id), Some(pin)) = (argv.get(1), argv.get(2)) else {
        return Err(format!("consent approve: expected <id> <pin>\n\n{}", consent_help_135()));
    };
    consent_expect_no_extra_args_135("approve", argv, 3)?;
    let (path, mut value, mut store) = consent_load_store_135(id)?;
    let result = approve_consent_plan(&mut store, id, pin, consent_now_ms_135()?);
    if !result.ok {
        return Err(format!("consent approve: {}", result.error.unwrap_or_else(|| "approval failed".to_owned())));
    }
    let entry = result.entry.ok_or_else(|| "consent approve: missing trust entry".to_owned())?;
    consent_append_trust_135(&entry)?;
    consent_persist_status_135(&path, &mut value, "approved")?;
    Ok(format!("approved {id}: trust {} → {} action={}\n", entry.from, entry.to, entry.action.as_str()))
}

fn consent_reject_native_135(argv: &[String]) -> Result<String, String> {
    let Some(id) = argv.get(1) else {
        return Err(format!("consent reject: expected <id>\n\n{}", consent_help_135()));
    };
    consent_expect_no_extra_args_135("reject", argv, 2)?;
    let (path, mut value, mut store) = consent_load_store_135(id)?;
    let result = reject_consent_plan(&mut store, id);
    if !result.ok {
        return Err(format!("consent reject: {}", result.error.unwrap_or_else(|| "reject failed".to_owned())));
    }
    consent_persist_status_135(&path, &mut value, "rejected")?;
    Ok(format!("rejected {id}\n"))
}

fn consent_load_store_135(id: &str) -> Result<(std::path::PathBuf, serde_json::Value, ConsentStore), String> {
    for dir in consent_pending_dirs_135() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue; };
        let mut paths = entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Ok(text) = std::fs::read_to_string(&path) else { continue; };
            let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else { continue; };
            if value.get("id").and_then(serde_json::Value::as_str) == Some(id) {
                let mut store = ConsentStore::default();
                store.write_pending(consent_pending_from_value_135(&value)?);
                return Ok((path, value, store));
            }
        }
    }
    Err(format!("consent: request not found: {id}"))
}

fn consent_pending_from_value_135(value: &serde_json::Value) -> Result<PendingRequest, String> {
    let field = |key: &str| value.get(key).and_then(serde_json::Value::as_str).map(str::to_owned)
        .ok_or_else(|| format!("consent: pending request missing {key}"));
    let action_raw = field("action")?;
    let action = ConsentAction::parse(&action_raw).ok_or_else(|| format!("consent: unknown action {action_raw}"))?;
    let status = match field("status")?.as_str() {
        "pending" => ConsentStatus::Pending,
        "approved" => ConsentStatus::Approved,
        "rejected" => ConsentStatus::Rejected,
        "expired" => ConsentStatus::Expired,
        other => return Err(format!("consent: unknown status {other}")),
    };
    Ok(PendingRequest {
        id: field("id")?,
        from: field("from")?,
        to: field("to")?,
        action,
        summary: field("summary")?,
        pin_hash: field("pinHash")?,
        created_at: field("createdAt")?,
        expires_at: field("expiresAt")?,
        status,
    })
}

fn consent_persist_status_135(path: &std::path::Path, value: &mut serde_json::Value, status: &str) -> Result<(), String> {
    let object = value.as_object_mut().ok_or_else(|| "consent: pending request is not a JSON object".to_owned())?;
    object.insert("status".to_owned(), serde_json::Value::String(status.to_owned()));
    let bytes = serde_json::to_vec_pretty(value).map_err(|error| format!("consent: serialize pending request: {error}"))?;
    std::fs::write(path, bytes).map_err(|error| format!("consent: write {}: {error}", path.display()))
}

fn consent_append_trust_135(entry: &TrustEntry) -> Result<(), String> {
    let path = std::env::var_os("CONSENT_TRUST_FILE")
        .map_or_else(|| maw_state_path(&current_xdg_env(), &["trust.json"]), std::path::PathBuf::from);
    let mut root = std::fs::read_to_string(&path).ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({ "version": 1, "trust": {} }));
    let approved_by = match entry.approved_by { ApprovedBy::Human => "human", ApprovedBy::Auto => "auto" };
    let record = serde_json::json!({
        "from": entry.from,
        "to": entry.to,
        "action": entry.action.as_str(),
        "approvedAt": entry.approved_at,
        "approvedBy": approved_by,
        "requestId": entry.request_id,
    });
    let object = root.as_object_mut().ok_or_else(|| "consent: trust store is not a JSON object".to_owned())?;
    let trust = object.entry("trust".to_owned()).or_insert_with(|| serde_json::json!({}));
    let trust = trust.as_object_mut().ok_or_else(|| "consent: trust map is not a JSON object".to_owned())?;
    trust.insert(trust_key(&entry.from, &entry.to, entry.action), record);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| format!("consent: create {}: {error}", parent.display()))?;
    }
    let bytes = serde_json::to_vec_pretty(&root).map_err(|error| format!("consent: serialize trust store: {error}"))?;
    std::fs::write(&path, bytes).map_err(|error| format!("consent: write {}: {error}", path.display()))
}

fn consent_now_ms_135() -> Result<i64, String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| "consent: system clock before unix epoch".to_owned())?;
    i64::try_from(now.as_millis()).map_err(|_| "consent: system clock overflow".to_owned())
}

fn consent_expect_no_extra_args_135(label: &str, argv: &[String], allowed: usize) -> Result<(), String> {
    if argv.len() <= allowed { return Ok(()); }
    let extra = &argv[allowed];
    if extra.starts_with('-') { Err(format!("consent {label}: unknown argument {extra}")) } else { Err(format!("consent {label}: unexpected argument {extra}")) }
}

fn consent_read_pending_135() -> Vec<ConsentPendingRow135> {
    let mut rows = Vec::new();
    let mut seen = BTreeSet::new();
    for dir in consent_pending_dirs_135() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue; };
        let mut paths = entries.flatten().map(|entry| entry.path()).collect::<Vec<_>>();
        paths.sort();
        for path in paths {
            let Some(name) = path.file_name().and_then(std::ffi::OsStr::to_str) else { continue; };
            if !std::path::Path::new(name).extension().is_some_and(|ext| ext.eq_ignore_ascii_case("json")) { continue; }
            let Ok(text) = std::fs::read_to_string(&path) else { continue; };
            let Ok(mut row) = serde_json::from_str::<ConsentPendingRow135>(&text) else { continue; };
            if !seen.insert(row.id.clone()) { continue; }
            consent_apply_expiry_135(&mut row);
            rows.push(row);
        }
    }
    rows.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    rows
}

fn consent_read_trust_135() -> Vec<ConsentTrustRow135> {
    let Some(path) = consent_readable_trust_path_135() else { return Vec::new(); };
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new(); };
    let Ok(file) = serde_json::from_str::<ConsentTrustFile135>(&text) else { return Vec::new(); };
    let mut rows = file.trust.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.approved_at.cmp(&right.approved_at));
    rows
}

fn consent_pending_dirs_135() -> Vec<std::path::PathBuf> {
    if let Some(value) = std::env::var_os("CONSENT_PENDING_DIR") { return vec![std::path::PathBuf::from(value)]; }
    let env = current_xdg_env();
    let primary = maw_state_path(&env, &["consent-pending"]);
    let legacy = maw_config_path(&env, &["consent-pending"]);
    if legacy == primary { vec![primary] } else { vec![primary, legacy] }
}

fn consent_readable_trust_path_135() -> Option<std::path::PathBuf> {
    if let Some(value) = std::env::var_os("CONSENT_TRUST_FILE") { return Some(std::path::PathBuf::from(value)).filter(|path| path.exists()); }
    let env = current_xdg_env();
    let primary = maw_state_path(&env, &["trust.json"]);
    if primary.exists() { return Some(primary); }
    let legacy = maw_config_path(&env, &["trust.json"]);
    if legacy != primary && legacy.exists() { Some(legacy) } else { None }
}

fn consent_apply_expiry_135(row: &mut ConsentPendingRow135) {
    if row.status != "pending" { return; }
    let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) else { return; };
    let Some(expires_ms) = consent_parse_iso_millis_135(&row.expires_at) else { return; };
    if now.as_millis() > u128::from(expires_ms) { "expired".clone_into(&mut row.status); }
}

fn consent_parse_iso_millis_135(value: &str) -> Option<u64> {
    let date_time = value.strip_suffix('Z')?;
    let (date, time) = date_time.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() { return None; }
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second_segment = time_parts.next()?;
    if time_parts.next().is_some() { return None; }
    let (whole_seconds, millis_raw) = second_segment.split_once('.').unwrap_or((second_segment, "0"));
    let second = whole_seconds.parse::<u32>().ok()?;
    let millis = millis_raw.get(..millis_raw.len().min(3))?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 { return None; }
    let days = consent_days_from_civil_135(year, month, day);
    let total = i128::from(days) * 86_400_000
        + i128::from(hour) * 3_600_000
        + i128::from(minute) * 60_000
        + i128::from(second) * 1_000
        + i128::from(millis);
    u64::try_from(total).ok()
}

fn consent_days_from_civil_135(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn consent_format_pending_135(rows: &[ConsentPendingRow135]) -> String {
    if rows.is_empty() { return "no pending consent requests".to_owned(); }
    let mut lines = vec!["id                        from → to             action            status   summary".to_owned()];
    for row in rows {
        let id = consent_pad_135(&row.id, 24);
        let from_to = consent_pad_135(&format!("{} → {}", row.from, row.to), 20);
        let action = consent_pad_135(&row.action, 16);
        let status = consent_pad_135(&row.status, 8);
        let summary = consent_truncate_summary_135(&row.summary);
        lines.push(format!("{id}  {from_to}  {action}  {status}  {summary}"));
    }
    lines.join("\n")
}

fn consent_format_trust_135(rows: &[ConsentTrustRow135]) -> String {
    if rows.is_empty() { return "no trust entries".to_owned(); }
    let mut lines = vec!["from → to                action            approvedAt".to_owned()];
    for row in rows {
        let from_to = consent_pad_135(&format!("{} → {}", row.from, row.to), 22);
        let action = consent_pad_135(&row.action, 16);
        lines.push(format!("{from_to}  {action}  {}", row.approved_at));
    }
    lines.join("\n")
}

fn consent_pad_135(value: &str, width: usize) -> String {
    let chars = value.chars().count();
    if chars >= width { value.to_owned() } else { format!("{value}{}", " ".repeat(width - chars)) }
}

fn consent_truncate_summary_135(value: &str) -> String {
    let mut chars = value.chars();
    let first = chars.by_ref().take(47).collect::<String>();
    if chars.next().is_some() { format!("{first}…") } else { value.to_owned() }
}

fn consent_help_135() -> String {
    [
        "usage:",
        "  maw consent                            list pending requests (alias for `list`)",
        "  maw consent list                       list pending requests",
        "  maw consent list-trust                 list approved trust entries",
        "  maw consent approve <id> <pin>         approve a pending request",
        "  maw consent reject <id>                reject a pending request",
        "  maw consent trust <peer> [action]      pre-approve trust (default action=hey)",
        "  maw consent untrust <peer> [action]    revoke trust entry",
        "",
        "actions: hey | team-invite | plugin-install | fleet-recruit",
        "consent gating is opt-in via MAW_CONSENT=1 (Phase 1).",
    ].join("\n")
}
