fn read_config_json(path: &Path) -> Result<Value, HostResult<Value>> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let file = open_nofollow_existing(path)?;
    verify_fd_path(&file, path)?;
    let mut raw = String::new();
    if let Err(error) = file.take(MAX_READ_BYTES + 1).read_to_string(&mut raw) {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            format!("read config failed: {error}"),
        ));
    }
    if raw.len() as u64 > MAX_READ_BYTES {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            "config exceeds maxBytes",
        ));
    }
    serde_json::from_str(&raw).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("config JSON parse failed: {error}"),
        )
    })
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentPendingRow {
    id: String,
    from: String,
    to: String,
    action: String,
    summary: String,
    #[serde(rename = "pinHash", skip_serializing)]
    _pin_hash: Option<String>,
    created_at: String,
    expires_at: String,
    status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ConsentTrustRow {
    from: String,
    to: String,
    action: String,
    approved_at: String,
    approved_by: Option<String>,
    request_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ConsentTrustFile {
    #[serde(default)]
    trust: BTreeMap<String, ConsentTrustRow>,
}

fn read_consent_pending(state_root: &Path) -> Result<Vec<ConsentPendingRow>, HostResult<Value>> {
    let dir = state_root.join("consent-pending");
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let dir = canonicalize_checked_path(&dir)?;
    if deny_special_path(&dir) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "special consent pending path denied",
        ));
    }
    let mut rows = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("read pending dir failed: {error}"),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|error| {
            HostResult::err(
                HostErrorCode::IoError,
                format!("read pending entry failed: {error}"),
            )
        })?;
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let extension = Path::new(name).extension().and_then(|ext| ext.to_str());
        if extension.is_none_or(|ext| !ext.eq_ignore_ascii_case("json"))
            || extension.is_some_and(|ext| ext.eq_ignore_ascii_case("tmp"))
        {
            continue;
        }
        if let Ok(value) = read_json_file(&path) {
            if let Ok(row) = serde_json::from_value::<ConsentPendingRow>(value) {
                rows.push(row);
            }
        }
    }
    rows.sort_by(|left, right| right.created_at.cmp(&left.created_at));
    Ok(rows)
}

fn read_consent_trust(state_root: &Path) -> Result<Vec<ConsentTrustRow>, HostResult<Value>> {
    let path = state_root.join("trust.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let file: ConsentTrustFile =
        serde_json::from_value(read_json_file(&path)?).map_err(|error| {
            HostResult::err(
                HostErrorCode::InvalidArgs,
                format!("trust JSON parse failed: {error}"),
            )
        })?;
    let mut rows = file.trust.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.approved_at.cmp(&right.approved_at));
    Ok(rows)
}

fn read_json_file(path: &Path) -> Result<Value, HostResult<Value>> {
    let path = canonicalize_checked_path(path)?;
    let file = open_nofollow_existing(&path)?;
    verify_fd_path(&file, &path)?;
    let mut raw = String::new();
    if let Err(error) = file.take(MAX_READ_BYTES + 1).read_to_string(&mut raw) {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            format!("read JSON failed: {error}"),
        ));
    }
    if raw.len() as u64 > MAX_READ_BYTES {
        return Err(HostResult::err(
            HostErrorCode::IoError,
            "JSON exceeds maxBytes",
        ));
    }
    serde_json::from_str(&raw).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("JSON parse failed: {error}"),
        )
    })
}

fn format_consent_pending(rows: &[ConsentPendingRow]) -> String {
    if rows.is_empty() {
        return "no pending consent requests".to_owned();
    }
    let mut lines = vec![
        "id                        from → to             action            status   summary"
            .to_owned(),
    ];
    for row in rows {
        let id = pad(&row.id, 24);
        let from_to = pad(&format!("{} → {}", row.from, row.to), 20);
        let action = pad(&row.action, 16);
        let status = pad(&row.status, 8);
        let summary = truncate_summary(&row.summary);
        lines.push(format!("{id}  {from_to}  {action}  {status}  {summary}"));
    }
    lines.join("\n")
}

fn format_consent_trust(rows: &[ConsentTrustRow]) -> String {
    if rows.is_empty() {
        return "no trust entries".to_owned();
    }
    let mut lines = vec!["from → to                action            approvedAt".to_owned()];
    for row in rows {
        let from_to = pad(&format!("{} → {}", row.from, row.to), 22);
        let action = pad(&row.action, 16);
        lines.push(format!("{from_to}  {action}  {}", row.approved_at));
    }
    lines.join("\n")
}

fn pad(value: &str, width: usize) -> String {
    if value.chars().count() >= width {
        value.to_owned()
    } else {
        format!("{value}{}", " ".repeat(width - value.chars().count()))
    }
}

fn truncate_summary(value: &str) -> String {
    if value.chars().count() <= 50 {
        return value.to_owned();
    }
    format!("{}…", value.chars().take(47).collect::<String>())
}
