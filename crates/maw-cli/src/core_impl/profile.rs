const DISPATCH_135: &[DispatcherEntry] = &[DispatcherEntry {
    command: "profile",
    handler: Handler::Sync(profile_run_command),
}];

const PROFILE_USAGE: &str = "usage: maw profile <list|use|show|current>\n  list                 — list all profiles (active is marked with *)\n  use     <name>       — set active profile (refuses unknown names)\n  show    <name>       — print one profile's JSON\n  current              — print active profile name\n\nstorage:\n  <CONFIG_DIR>/profiles/<name>.json   — one file per profile\n  <CONFIG_DIR>/profile-active         — active profile pointer (text)\n\nnote: Phase 1 of #640 — additive read + active-pointer only. Profile\n      authoring is operator-driven (hand-edit JSON). Phase 2 wires this\n      into the plugin loader.";

fn profile_run_command(argv: &[String]) -> CliOutput {
    match profile_dispatch(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(error) => CliOutput { code: 1, stdout: error.stdout, stderr: format!("{}\n", error.message) },
    }
}

#[derive(Debug, Clone)]
struct ProfileError {
    message: String,
    stdout: String,
}

#[derive(Debug, Clone)]
struct ProfileRecord {
    name: String,
    value: serde_json::Value,
}

fn profile_dispatch(argv: &[String]) -> Result<String, ProfileError> {
    let positional: Vec<&str> = argv.iter().map(String::as_str).filter(|arg| !arg.starts_with("--")).collect();
    let Some(sub) = positional.first().copied() else { return Ok(format!("{PROFILE_USAGE}\n")); };
    match sub {
        "list" | "ls" => profile_list(),
        "current" | "active" => Ok(format!("{}\n", profile_get_active())),
        "show" | "info" => {
            let name = profile_required_name(argv, &positional, "usage: maw profile show <name>")?;
            profile_show(name)
        }
        "use" | "set" => {
            let name = profile_required_name(argv, &positional, "usage: maw profile use <name>")?;
            profile_use(name)
        }
        _ => Err(ProfileError {
            message: format!("maw profile: unknown subcommand \"{sub}\" (expected list|use|show|current)"),
            stdout: format!("{PROFILE_USAGE}\n"),
        }),
    }
}

fn profile_required_name<'a>(argv: &'a [String], positional: &[&'a str], usage: &str) -> Result<&'a str, ProfileError> {
    if let Some(raw) = argv.get(1).filter(|value| value.starts_with('-')) {
        return Err(profile_error(profile_validate_name(raw).expect_err("invalid name")));
    }
    positional.get(1).copied().ok_or_else(|| ProfileError { message: usage.to_owned(), stdout: String::new() })
}

fn profile_list() -> Result<String, ProfileError> {
    let rows = profile_load_all()?;
    Ok(format!("{}\n", profile_format_list(&rows, &profile_get_active())))
}

fn profile_show(name: &str) -> Result<String, ProfileError> {
    profile_validate_name(name).map_err(profile_error)?;
    let Some(profile) = profile_load(name)? else {
        return Err(ProfileError { message: format!("profile \"{name}\" not found"), stdout: String::new() });
    };
    serde_json::to_string_pretty(&profile.value)
        .map(|body| format!("{body}\n"))
        .map_err(|error| profile_error(format!("maw profile: failed to render JSON: {error}")))
}

fn profile_use(name: &str) -> Result<String, ProfileError> {
    profile_validate_name(name).map_err(profile_error)?;
    let Some(profile) = profile_load(name)? else {
        return Err(ProfileError { message: format!("profile \"{name}\" not found — see \"maw profile list\""), stdout: String::new() });
    };
    profile_set_active(name)?;
    Ok(format!("active profile: \"{}\"\n", profile.name))
}

fn profile_load(name: &str) -> Result<Option<ProfileRecord>, ProfileError> {
    if profile_validate_name(name).is_err() { return Ok(None); }
    if name == "all" { profile_ensure_default()?; }
    let path = profile_path(name);
    if !path.exists() { return Ok(None); }
    let Ok(raw) = std::fs::read_to_string(&path) else { return Ok(None) };
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&raw) else { return Ok(None) };
    if value.get("name").and_then(serde_json::Value::as_str).is_none_or(str::is_empty) {
        if let Some(object) = value.as_object_mut() {
            object.insert("name".to_owned(), serde_json::Value::String(name.to_owned()));
        }
    }
    let profile_name = value.get("name").and_then(serde_json::Value::as_str).unwrap_or(name).to_owned();
    Ok(Some(ProfileRecord { name: profile_name, value }))
}

fn profile_load_all() -> Result<Vec<ProfileRecord>, ProfileError> {
    profile_ensure_default()?;
    let dir = profile_profiles_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else { return Ok(Vec::new()) };
    let mut rows = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") { continue; }
        let Some(name) = path.file_stem().and_then(|value| value.to_str()) else { continue; };
        if let Some(profile) = profile_load(name)? { rows.push(profile); }
    }
    rows.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(rows)
}

fn profile_format_list(rows: &[ProfileRecord], active: &str) -> String {
    if rows.is_empty() { return "no profiles".to_owned(); }
    let mut table = vec![vec![String::new(), "name".to_owned(), "plugins".to_owned(), "tiers".to_owned(), "description".to_owned()]];
    for row in rows {
        table.push(vec![
            if row.name == active { "*" } else { " " }.to_owned(),
            row.name.clone(),
            profile_array_len(&row.value, "plugins").map_or_else(|| "-".to_owned(), |len| len.to_string()),
            profile_string_array(&row.value, "tiers").filter(|value| !value.is_empty()).unwrap_or_else(|| "-".to_owned()),
            row.value.get("description").and_then(serde_json::Value::as_str).unwrap_or("").to_owned(),
        ]);
    }
    let widths = (0..5).map(|index| table.iter().map(|row| row[index].chars().count()).max().unwrap_or(0)).collect::<Vec<_>>();
    let mut out = Vec::new();
    for (index, row) in table.iter().enumerate() {
        if index == 1 { out.push(profile_format_row(&widths, &["-", "----", "-------", "-----", "-----------"])); }
        out.push(profile_format_row(&widths, row));
    }
    out.join("\n")
}

fn profile_format_row(widths: &[usize], row: &[impl AsRef<str>]) -> String {
    row.iter().enumerate().map(|(index, value)| format!("{:<width$}", value.as_ref(), width = widths[index])).collect::<Vec<_>>().join("  ")
}

fn profile_array_len(value: &serde_json::Value, key: &str) -> Option<usize> {
    value.get(key).and_then(serde_json::Value::as_array).map(Vec::len)
}

fn profile_string_array(value: &serde_json::Value, key: &str) -> Option<String> {
    let array = value.get(key)?.as_array()?;
    Some(array.iter().filter_map(serde_json::Value::as_str).collect::<Vec<_>>().join(","))
}

fn profile_get_active() -> String {
    let path = profile_active_path();
    let raw = std::fs::read_to_string(path).unwrap_or_default();
    let active = raw.trim();
    if active.is_empty() || profile_validate_name(active).is_err() { "all".to_owned() } else { active.to_owned() }
}

fn profile_set_active(name: &str) -> Result<(), ProfileError> {
    profile_validate_name(name).map_err(profile_error)?;
    let dir = profile_config_dir();
    std::fs::create_dir_all(&dir).map_err(|error| profile_error(format!("maw profile: failed to create config dir: {error}")))?;
    profile_atomic_write(&profile_active_path(), &format!("{name}\n"))
}

fn profile_ensure_default() -> Result<(), ProfileError> {
    let path = profile_path("all");
    if path.exists() { return Ok(()); }
    let body = "{\n  \"name\": \"all\",\n  \"description\": \"All plugins (Phase 1 default — equivalent to no profile filter).\"\n}\n";
    std::fs::create_dir_all(profile_profiles_dir()).map_err(|error| profile_error(format!("maw profile: failed to create profiles dir: {error}")))?;
    profile_atomic_write(&path, body)
}

fn profile_atomic_write(path: &std::path::Path, body: &str) -> Result<(), ProfileError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| profile_error(format!("maw profile: failed to create parent dir: {error}")))?;
    }
    let file_name = path.file_name().and_then(|value| value.to_str()).unwrap_or("profile");
    let tmp = path.with_file_name(format!("{file_name}.tmp"));
    std::fs::write(&tmp, body).map_err(|error| profile_error(format!("maw profile: failed to write temp file: {error}")))?;
    std::fs::rename(&tmp, path).map_err(|error| profile_error(format!("maw profile: failed to replace file: {error}")))
}

fn profile_config_dir() -> std::path::PathBuf { maw_config_dir(&current_xdg_env()) }

fn profile_profiles_dir() -> std::path::PathBuf { profile_config_dir().join("profiles") }

fn profile_path(name: &str) -> std::path::PathBuf { profile_profiles_dir().join(format!("{name}.json")) }

fn profile_active_path() -> std::path::PathBuf { profile_config_dir().join("profile-active") }

fn profile_validate_name(name: &str) -> Result<(), String> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || !bytes[0].is_ascii_lowercase() && !bytes[0].is_ascii_digit() {
        return Err(format!("invalid profile name \"{name}\" (must match ^[a-z0-9][a-z0-9_-]{{0,63}}$)"));
    }
    if !bytes.iter().skip(1).all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_' || *byte == b'-') {
        return Err(format!("invalid profile name \"{name}\" (must match ^[a-z0-9][a-z0-9_-]{{0,63}}$)"));
    }
    Ok(())
}

fn profile_error(message: String) -> ProfileError { ProfileError { message, stdout: String::new() } }

#[cfg(test)]
mod profile_tests {
    use super::{dispatcher_status, profile_dispatch, profile_path, DispatchKind, EnvVarRestore};

    #[test]
    fn profile_dispatch_registers_native() {
        assert_eq!(dispatcher_status("profile"), DispatchKind::Native);
    }

    #[test]
    fn profile_rejects_bad_names_before_write() {
        let error = profile_dispatch(&["use".to_owned(), "--bad".to_owned()]).expect_err("flag-like name");
        assert!(error.message.contains("invalid profile name"));
        let error = profile_dispatch(&["show".to_owned(), "BadName".to_owned()]).expect_err("uppercase");
        assert!(error.message.contains("invalid profile name"));
    }

    #[test]
    fn profile_use_writes_active_pointer_atomically() {
        let _lock = super::env_test_lock().lock().expect("env lock");
        let _home = EnvVarRestore::capture("MAW_HOME");
        let _config = EnvVarRestore::capture("MAW_CONFIG_DIR");
        let root = std::env::temp_dir().join(format!("maw-rs-profile-unit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::env::set_var("MAW_CONFIG_DIR", root.join("config"));
        std::env::remove_var("MAW_HOME");
        std::fs::create_dir_all(root.join("config/profiles")).expect("profiles dir");
        std::fs::write(profile_path("lean"), "{\"name\":\"lean\"}\n").expect("profile");
        let stdout = profile_dispatch(&["use".to_owned(), "lean".to_owned()]).expect("use profile");
        assert_eq!(stdout, "active profile: \"lean\"\n");
        assert_eq!(std::fs::read_to_string(root.join("config/profile-active")).expect("active"), "lean\n");
        assert!(!root.join("config/profile-active.tmp").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
