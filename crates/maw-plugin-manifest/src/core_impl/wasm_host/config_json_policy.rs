fn write_config_json(path: &Path, config: &Value) -> Result<(), HostResult<Value>> {
    let parent = path.parent().ok_or_else(|| {
        HostResult::err(HostErrorCode::InvalidArgs, "config path requires parent")
    })?;
    let parent = canonicalize_checked_path(parent)?;
    if deny_special_path(&parent) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "special config root denied",
        ));
    }
    let path = parent.join("maw.config.json");
    let mut opts = OpenOptions::new();
    opts.write(true)
        .create(true)
        .truncate(true)
        .custom_flags(O_NOFOLLOW_FLAG);
    let mut file = opts.open(&path).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("open config failed: {error}"),
        )
    })?;
    verify_fd_path(&file, &path)?;
    let content = serde_json::to_string_pretty(config).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("serialize config failed: {error}"),
        )
    })?;
    file.write_all(content.as_bytes()).map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("write config failed: {error}"),
        )
    })?;
    file.write_all(b"\n").map_err(|error| {
        HostResult::err(
            HostErrorCode::IoError,
            format!("write config failed: {error}"),
        )
    })?;
    Ok(())
}
fn get_json_path<'a>(value: &'a Value, key_path: &str) -> Option<&'a Value> {
    let mut current = value;
    for part in key_path.split('.').filter(|part| !part.is_empty()) {
        current = current.get(part)?;
    }
    Some(current)
}
fn set_json_path(
    target: &mut Value,
    key_path: &str,
    value: Value,
) -> Result<(), HostResult<Value>> {
    let parts = key_path
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some((last, parents)) = parts.split_last() else {
        return Err(HostResult::err(
            HostErrorCode::InvalidArgs,
            "config key is required",
        ));
    };
    if !target.is_object() {
        *target = json!({});
    }
    let mut current = target;
    for part in parents {
        let object = current.as_object_mut().ok_or_else(|| {
            HostResult::err(
                HostErrorCode::InvalidArgs,
                "config path conflicts with non-object value",
            )
        })?;
        current = object
            .entry((*part).to_owned())
            .or_insert_with(|| json!({}));
    }
    let object = current.as_object_mut().ok_or_else(|| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            "config path conflicts with non-object value",
        )
    })?;
    object.insert((*last).to_owned(), value);
    Ok(())
}
const PLUGIN_WRITABLE_CONFIG_KEYS: &[&str] = &[
    // Keep this host-side allowlist intentionally small: sandboxed plugins may
    // only write config keys that are already parity-backed as safe user-facing
    // `maw config set` targets. Everything else is denied by default.
    "node",
    "port",
];

fn is_plugin_writable_config_key_path(key: &str) -> bool {
    normalized_config_key_path(key).is_some_and(|key| {
        PLUGIN_WRITABLE_CONFIG_KEYS
            .iter()
            .any(|allowed| key == *allowed)
    })
}

fn normalized_config_key_path(key: &str) -> Option<String> {
    let parts = key
        .split('.')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("."))
    }
}

fn is_secret_like_config_key_path(key: &str) -> bool {
    let lower = key.to_lowercase();
    [
        "password",
        "passwd",
        "pwd",
        "credential",
        "private",
        "privatekey",
        "private_key",
        "passphrase",
        "cert",
        "pem",
        "secret",
        "token",
        "apikey",
        "api_key",
        "peerkey",
        "peer_key",
        "oauth",
        "auth_token",
        "auth-token",
        "authtoken",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
        || Path::new(&lower)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("key"))
        || Path::new(&lower)
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("env"))
        || lower == "key"
}
fn value_contains_secret_config_key_path(prefix: &str, value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            is_secret_like_config_key_path(&path) || value_contains_secret_config_key_path(&path, value)
        }),
        Value::Array(values) => values
            .iter()
            .any(|value| value_contains_secret_config_key_path(prefix, value)),
        _ => false,
    }
}
