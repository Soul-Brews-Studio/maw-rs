const DISPATCH_333: &[DispatcherEntry] = &[DispatcherEntry { command: "squad", handler: Handler::Sync(run_squad_command) }];

fn run_squad_command(argv: &[String]) -> CliOutput {
    if squad_plugin_owned_args(argv) {
        return CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: "squad: unknown native subcommand\n".to_owned(),
        };
    }
    match squad_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn squad_plugin_owned_args(argv: &[String]) -> bool {
    !matches!(argv.first().map(String::as_str), Some("token" | "import"))
}

fn squad_run(argv: &[String]) -> Result<String, String> {
    match argv.first().map(String::as_str) {
        Some("token") => squad_token_set(argv),
        Some("import") => squad_import(argv),
        _ => Err("usage: maw squad token set <squad> <name> [--member M] | maw squad import <seed.json>".to_owned()),
    }
}

fn squad_token_set(argv: &[String]) -> Result<String, String> {
    let (Some(cmd), Some(group), Some(name)) = (argv.get(1), argv.get(2), argv.get(3)) else {
        return Err("usage: maw squad token set <squad> <name> [--member M]".to_owned());
    };
    if cmd != "set" { return Err("usage: maw squad token set <squad> <name> [--member M]".to_owned()); }
    fleet_validate_session_name(group)?;
    token_validate_name("token name", name)?;
    let mut member = None::<String>;
    let mut idx = 4;
    while idx < argv.len() {
        match argv[idx].as_str() {
            "--member" => {
                idx += 1;
                let Some(value) = argv.get(idx) else { return Err("maw squad token set: --member requires a value".to_owned()); };
                fleet_validate_session_name(value)?;
                member = Some(value.clone());
            }
            value if value.starts_with("--member=") => {
                let value = value.trim_start_matches("--member=");
                fleet_validate_session_name(value)?;
                member = Some(value.to_owned());
            }
            value => return Err(format!("maw squad token set: unknown argument {value}")),
        }
        idx += 1;
    }
    squad_set_token(&current_xdg_env(), group, name, member.as_deref())?;
    Ok(match member {
        Some(member) => format!("squad token set {group}: {member} -> {name}\n"),
        None => format!("squad token set {group}: token -> {name}\n"),
    })
}

fn squad_set_token(env: &MawXdgEnv, group: &str, name: &str, member: Option<&str>) -> Result<(), String> {
    let entry = fleet_roster_find_entry(env, group, "token set")?;
    let text = std::fs::read_to_string(&entry.path).map_err(|error| format!("squad token set: read {}: {error}", entry.path.display()))?;
    let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("squad token set: parse {}: {error}", entry.path.display()))?;
    let object = value.as_object_mut().ok_or_else(|| "squad token set: squad file is not a JSON object".to_owned())?;
    if let Some(member) = member {
        let members = object.get_mut("members").and_then(serde_json::Value::as_array_mut).ok_or_else(|| format!("squad token set: squad {group} has no members roster"))?;
        let target = members.iter_mut().find(|item| item.get("handle").and_then(serde_json::Value::as_str) == Some(member)).ok_or_else(|| format!("squad token set: unknown member {member} in {group}"))?;
        target["token"] = serde_json::json!(name);
    } else {
        object.insert("token".to_owned(), serde_json::json!(name));
    }
    let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
    std::fs::write(&entry.path, format!("{body}\n")).map_err(|error| format!("squad token set: write {}: {error}", entry.path.display()))
}

fn squad_import(argv: &[String]) -> Result<String, String> {
    let (Some(path), None) = (argv.get(1), argv.get(2)) else { return Err("usage: maw squad import <seed.json>".to_owned()); };
    let text = std::fs::read_to_string(path).map_err(|error| format!("squad import: read {path}: {error}"))?;
    let seed: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("squad import: parse {path}: {error}"))?;
    let squads = seed.get("squads").and_then(serde_json::Value::as_array).ok_or_else(|| "squad import: seed must contain squads[]".to_owned())?;
    let env = current_xdg_env();
    let mut count = 0usize;
    for squad in squads {
        let name = squad.get("name").and_then(serde_json::Value::as_str).ok_or_else(|| "squad import: squad missing name".to_owned())?;
        let token = squad.get("token").and_then(serde_json::Value::as_str).ok_or_else(|| format!("squad import: squad {name} missing token"))?;
        fleet_validate_session_name(name)?;
        token_validate_name("token name", token)?;
        let members = squad.get("members").and_then(serde_json::Value::as_array).ok_or_else(|| format!("squad import: squad {name} missing members[]"))?;
        let entry = if let Ok(entry) = fleet_roster_find_entry(&env, name, "import") { entry } else {
            fleet_roster_create_with_token(&env, name, Some(token))?;
            fleet_roster_find_entry(&env, name, "import")?
        };
        let mut value: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&entry.path).map_err(|error| error.to_string())?).map_err(|error| error.to_string())?;
        let object = value.as_object_mut().ok_or_else(|| "squad import: squad file is not a JSON object".to_owned())?;
        object.insert("token".to_owned(), serde_json::json!(token));
        object.insert("members".to_owned(), members.clone().into_iter().map(|member| {
            let handle = member.as_str().unwrap_or_default();
            serde_json::json!({"handle": handle})
        }).collect::<serde_json::Value>());
        let body = serde_json::to_string_pretty(&value).map_err(|error| error.to_string())?;
        std::fs::write(&entry.path, format!("{body}\n")).map_err(|error| format!("squad import: write {}: {error}", entry.path.display()))?;
        count += 1;
    }
    Ok(format!("squad import: {count} squad(s) imported\n"))
}

#[cfg(test)]
mod squad_tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn squad_env(name: &str) -> (std::path::PathBuf, Vec<EnvVarRestore>) {
        let root = std::env::temp_dir().join(format!("maw-rs-squad-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        let guards = ["HOME", "MAW_HOME", "MAW_CONFIG_DIR", "MAW_STATE_DIR", "MAW_CACHE_DIR", "GHQ_ROOT"].map(EnvVarRestore::capture).into_iter().collect();
        std::env::remove_var("MAW_HOME");
        for (key, dir) in [("HOME", "home"), ("MAW_CONFIG_DIR", "config"), ("MAW_STATE_DIR", "state"), ("MAW_CACHE_DIR", "cache"), ("GHQ_ROOT", "ghq")] {
            std::env::set_var(key, root.join(dir));
        }
        (root, guards)
    }

    #[test]
    fn squad_token_set_preserves_unknown_fields_and_sets_member_override() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, _env) = squad_env("token-set");
        let dir = root.join("state/fleet/squads/01-core");
        std::fs::create_dir_all(&dir).expect("squad dir");
        std::fs::write(dir.join("squad.json"), r#"{"name":"01-core","squadName":"core","mystery":true,"members":[{"handle":"atlas","role":"lead"}]}"#).expect("squad file");
        assert_eq!(squad_run(&args(&["token", "set", "core", "duo"])).expect("set squad"), "squad token set core: token -> duo\n");
        assert_eq!(squad_run(&args(&["token", "set", "core", "dd2", "--member", "atlas"])).expect("set member"), "squad token set core: atlas -> dd2\n");
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("squad.json")).expect("saved")).expect("json");
        assert_eq!(saved["mystery"], true);
        assert_eq!(saved["token"], "duo");
        assert_eq!(saved["members"][0]["role"], "lead");
        assert_eq!(saved["members"][0]["token"], "dd2");
    }

    #[test]
    fn squad_import_creates_seven_fleet_seed() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, _env) = squad_env("import");
        let seed = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/native-fleet-roster/seven-fleet-seed.json");
        assert_eq!(squad_run(&args(&["import", &seed.display().to_string()])).expect("import"), "squad import: 7 squad(s) imported\n");
        let core = root.join("state/fleet/squads/06-core/squad.json");
        let saved: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(core).expect("core squad")).expect("json");
        assert_eq!(saved["token"], "duo");
        assert_eq!(saved["members"][0]["handle"], "token");
    }

    #[test]
    fn squad_start_is_a_plugin_fallthrough_miss_not_native_usage() {
        let output = run_squad_command(&args(&["start"]));

        assert_eq!(output.code, 2);
        assert_eq!(output.stderr, "squad: unknown native subcommand\n");
    }
}
