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
        "approve" | "reject" | "trust" | "untrust" => Err(format!(
            "maw consent {sub} is not native in maw-rs ZERO-BUN B2; use a human-at-terminal consent command\n\n{}",
            consent_help_135()
        )),
        value if value.starts_with('-') => Err(format!("consent: unknown argument {value}\n\n{}", consent_help_135())),
        value => Err(format!("unknown subcommand: {value}\n\n{}", consent_help_135())),
    }
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
        "actions: hey | team-invite | plugin-install",
        "consent gating is opt-in via MAW_CONSENT=1 (Phase 1).",
    ].join("\n")
}
