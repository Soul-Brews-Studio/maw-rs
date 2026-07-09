// maw fleet roster — squadron members[] on fleet squad files (#291). Pure roster (de)serialization
// stays deterministic and I/O-free; commands compose it with fleet read dirs + oracles.json cache.

const FLEET_ROSTER_USAGE: &str = "usage: maw fleet create <squad> | maw fleet show <squad> [--json] | maw fleet status <squad> [--json]";

#[derive(Debug, Clone, Default, serde::Deserialize, serde::Serialize, PartialEq, Eq)]
struct NativeFleetMember {
    handle: String,
    #[serde(default, skip_serializing_if = "Option::is_none")] org_repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")] joined_at: Option<String>,
}

struct FleetRosterMemberView { member: NativeFleetMember, node: Option<String>, is_live: Option<bool> }

fn fleet_roster_intercept(argv: &[String]) -> Option<Result<(i32, String), String>> {
    let sub = argv.iter().find(|arg| !arg.starts_with('-'))?;
    match sub.as_str() {
        "create" | "show" | "status" => Some(fleet_roster_run(argv)),
        "join" => Some(fleet_join_run(argv)),
        _ => None,
    }
}

fn fleet_roster_run(argv: &[String]) -> Result<(i32, String), String> {
    let mut json = false;
    let mut positional = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            value if value.starts_with('-') => return Err(FLEET_ROSTER_USAGE.to_owned()),
            value => positional.push(value),
        }
    }
    let (Some(&sub), Some(&group), None) = (positional.first(), positional.get(1), positional.get(2)) else {
        return Err(FLEET_ROSTER_USAGE.to_owned());
    };
    fleet_validate_session_name(group)?;
    let env = current_xdg_env();
    match sub {
        "create" => fleet_roster_create(&env, group),
        "show" => fleet_roster_show(&env, group, json, None),
        _ => fleet_roster_show(&env, group, json, Some(&TmuxClient::local().list_all())),
    }
}

fn fleet_roster_create(env: &MawXdgEnv, group: &str) -> Result<(i32, String), String> {
    let entries = fleet_load_entries_result_for_env(env, "fleet create")?;
    if entries.iter().any(|entry| fleet_roster_entry_matches(entry, group)) {
        return Err(format!("fleet create: squad {group} already exists"));
    }
    let nn = fleet_roster_next_nn(&fleet_roster_used_nns(env))
        .ok_or_else(|| "fleet create: no free NN slot in 01-99".to_owned())?;
    let dir = maw_state_path(env, &["fleet"]).join("squads").join(format!("{nn:02}-{group}"));
    std::fs::create_dir_all(&dir).map_err(|error| format!("fleet create: create {}: {error}", dir.display()))?;
    let path = dir.join("squad.json");
    let body = fleet_roster_new_file_json(nn, group, &fleet_registry_now_iso())?;
    std::fs::write(&path, body).map_err(|error| format!("fleet create: write {}: {error}", path.display()))?;
    Ok((0, format!("fleet create {group}: created {}\n", path.display())))
}

// Pure + deterministic: same inputs render the same file body.
fn fleet_roster_new_file_json(nn: u32, group: &str, created_at: &str) -> Result<String, String> {
    let value = serde_json::json!({
        "name": format!("{nn:02}-{group}"),
        "squadName": group,
        "created_at": created_at,
        "created_by": "maw fleet create",
        "windows": [],
        "members": [],
    });
    serde_json::to_string_pretty(&value).map(|text| format!("{text}\n")).map_err(|error| error.to_string())
}

fn fleet_roster_used_nns(env: &MawXdgEnv) -> BTreeSet<u32> {
    let mut used = fleet_load_entries_for_env(env)
        .into_iter()
        .filter_map(|entry| fleet_roster_nn_prefix(&entry.file))
        .collect::<BTreeSet<_>>();
    for dir in fleet_read_dirs_for_env(env) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            used.extend(entries.flatten().filter_map(|entry| fleet_roster_nn_prefix(&entry.file_name().to_string_lossy())));
        }
        if let Ok(entries) = std::fs::read_dir(dir.join("squads")) {
            used.extend(entries.flatten().filter_map(|entry| fleet_roster_nn_prefix(&entry.file_name().to_string_lossy())));
        }
    }
    used
}

fn fleet_roster_nn_prefix(file: &str) -> Option<u32> {
    let (prefix, _) = file.split_once('-')?;
    if prefix.is_empty() || !prefix.bytes().all(|byte| byte.is_ascii_digit()) { return None; }
    prefix.parse().ok()
}

fn fleet_roster_next_nn(used: &BTreeSet<u32>) -> Option<u32> {
    (1..=99).find(|nn| !used.contains(nn))
}

fn fleet_roster_entry_matches(entry: &NativeFleetEntry, group: &str) -> bool {
    if fleet_roster_squad_name(entry).is_none() {
        return false;
    }
    let stem = entry.file.strip_suffix(".json").unwrap_or(&entry.file);
    group == stem
        || group == entry.file
        || group == fleet_roster_unnumbered_stem(entry)
        || group == entry.session.name
        || (!entry.session.squad_name.is_empty() && group == entry.session.squad_name)
}

fn fleet_roster_unnumbered_stem(entry: &NativeFleetEntry) -> &str {
    let stem = entry.file.strip_suffix(".json").unwrap_or(&entry.file);
    stem.split_once('-')
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(stem, |(_, tail)| tail)
}

// Squadron-squad view for completions + ls filtering (#307/#317): only members[] files
// are rosters; squadName is just the display alias when present.
fn fleet_roster_squad_name(entry: &NativeFleetEntry) -> Option<String> {
    entry.session.members.as_ref()?;
    if !entry.session.squad_name.is_empty() { return Some(entry.session.squad_name.clone()); }
    Some(fleet_roster_unnumbered_stem(entry).to_owned())
}

fn fleet_roster_show(env: &MawXdgEnv, group: &str, json: bool, live: Option<&[TmuxSession]>) -> Result<(i32, String), String> {
    let entries = fleet_load_entries_result_for_env(env, "fleet")?;
    let entry = entries
        .into_iter()
        .find(|entry| fleet_roster_entry_matches(entry, group))
        .ok_or_else(|| format!("fleet: no squad named {group} — try: maw fleet create {group}"))?;
    let cache = locate_load_registry_cache();
    let members = entry.session.members.clone().unwrap_or_default().into_iter()
        .map(|member| fleet_roster_member_view(member, cache.as_ref(), live))
        .collect::<Vec<_>>();
    if json { return fleet_roster_json(group, &entry, &members); }
    Ok((0, fleet_roster_render(group, &entry, &members)))
}

fn fleet_roster_member_view(member: NativeFleetMember, cache: Option<&LocateRegistryCache>, live: Option<&[TmuxSession]>) -> FleetRosterMemberView {
    let cached = cache.and_then(|cache| cache.oracles.iter().find(|oracle| oracle.name == member.handle));
    let node = member.node.clone().or_else(|| cached.and_then(|oracle| oracle.federation_node.clone()));
    let is_live = live.map(|sessions| locate_resolve_session(&member.handle, sessions).is_some());
    FleetRosterMemberView { member, node, is_live }
}

fn fleet_roster_json(group: &str, entry: &NativeFleetEntry, members: &[FleetRosterMemberView]) -> Result<(i32, String), String> {
    let value = serde_json::json!({
        "group": group,
        "name": entry.session.name,
        "path": entry.path,
        "legacy": entry.session.members.is_none(),
        "memberCount": members.len(),
        "members": members.iter().map(fleet_roster_member_json).collect::<Vec<_>>(),
    });
    serde_json::to_string_pretty(&value).map(|text| (0, format!("{text}\n"))).map_err(|error| error.to_string())
}

fn fleet_roster_member_json(view: &FleetRosterMemberView) -> serde_json::Value {
    let mut value = serde_json::to_value(&view.member).unwrap_or_else(|_| serde_json::json!({}));
    if let Some(node) = &view.node { value["node"] = serde_json::json!(node); }
    if let Some(is_live) = view.is_live { value["is_live"] = serde_json::json!(is_live); }
    value
}

fn fleet_roster_render(group: &str, entry: &NativeFleetEntry, members: &[FleetRosterMemberView]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "fleet {group} ({})", entry.path.display());
    let _ = writeln!(out, "  members: {}", members.len());
    for view in members {
        let _ = write!(out, "  - {}", view.member.handle);
        if let Some(role) = &view.member.role { let _ = write!(out, " [{role}]"); }
        if let Some(node) = &view.node { let _ = write!(out, " node={node}"); }
        if let Some(live) = view.is_live { let _ = write!(out, " live={}", if live { "yes" } else { "no" }); }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod fleet_roster_tests {
    use super::*;

    const ROSTER_LEGACY_FIXTURE: &str = include_str!("../../tests/fixtures/native-fleet-roster/legacy-03-alpha.json");
    const ROSTER_GROUP_FIXTURE: &str = include_str!("../../tests/fixtures/native-fleet-roster/squads/01-3e/squad.json");

    fn roster_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn roster_env(name: &str) -> (std::path::PathBuf, Vec<EnvVarRestore>) {
        let root = std::env::temp_dir().join(format!("maw-rs-fleet-roster-{name}-{}", std::process::id()));
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
    fn fleet_roster_legacy_fixture_parses_without_members() {
        let session: NativeFleetSession = serde_json::from_str(ROSTER_LEGACY_FIXTURE).expect("legacy parse");
        assert_eq!(session.name, "03-alpha");
        assert_eq!(session.windows.len(), 2);
        assert_eq!(session.members, None);
    }

    #[test]
    fn fleet_roster_new_file_json_is_deterministic_and_round_trips() {
        let first = fleet_roster_new_file_json(1, "3e", "2026-07-08T00:00:00.000Z").expect("render");
        assert_eq!(first, fleet_roster_new_file_json(1, "3e", "2026-07-08T00:00:00.000Z").expect("render"));
        let session: NativeFleetSession = serde_json::from_str(&first).expect("parse");
        assert_eq!(session.name, "01-3e");
        assert_eq!(session.squad_name, "3e");
        assert_eq!(session.members, Some(Vec::new()));
        assert_eq!(fleet_roster_next_nn(&BTreeSet::from([1, 2, 4])), Some(3));
        assert_eq!(fleet_roster_next_nn(&(1..=99).collect()), None);
        assert_eq!(fleet_roster_nn_prefix("22-dormant.disabled"), Some(22));
    }

    #[test]
    fn fleet_create_show_round_trip_cache_resolution_and_legacy_files() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, _env) = roster_env("round-trip");
        let created = run_fleet_command(&roster_args(&["create", "3e"]));
        assert_eq!(created.code, 0, "{}", created.stderr);
        assert!(root.join("state/fleet/squads/01-3e/squad.json").exists());
        assert_eq!(run_fleet_command(&roster_args(&["create", "3e"])).code, 1, "duplicate squad refused");
        let shown = run_fleet_command(&roster_args(&["show", "3e", "--json"]));
        assert_eq!(shown.code, 0, "{}", shown.stderr);
        let value: serde_json::Value = serde_json::from_str(&shown.stdout).expect("json");
        assert_eq!(value["name"], "01-3e");
        assert_eq!(value["legacy"], false);
        assert_eq!(value["memberCount"], 0);

        let roster_json = r#"{"name":"05-ccdc","squadName":"ccdc","windows":[],"members":[{"handle":"atlas","role":"lead"},{"handle":"drift","node":"mba"}]}"#;
        std::fs::create_dir_all(root.join("state/fleet/squads/05-ccdc")).expect("squad dir");
        std::fs::write(root.join("state/fleet/squads/05-ccdc/squad.json"), roster_json).expect("squad file");
        std::fs::write(root.join("state/fleet/03-alpha.json"), ROSTER_LEGACY_FIXTURE).expect("legacy file");
        std::fs::create_dir_all(root.join("cache")).expect("cache dir");
        let cache_json = r#"{"schema":1,"oracles":[{"org":"acme","repo":"atlas-oracle","name":"atlas","local_path":"/tmp/atlas","has_psi":true,"has_fleet_config":true,"federation_node":"white"}]}"#;
        std::fs::write(root.join("cache/oracles.json"), cache_json).expect("cache file");
        let roster = run_fleet_command(&roster_args(&["show", "ccdc", "--json"]));
        assert_eq!(roster.code, 0, "{}", roster.stderr);
        let roster: serde_json::Value = serde_json::from_str(&roster.stdout).expect("roster json");
        assert_eq!(roster["members"][0]["handle"], "atlas");
        assert_eq!(roster["members"][0]["role"], "lead");
        assert_eq!(roster["members"][0]["node"], "white", "resolved via oracles.json cache");
        assert!(roster["members"][0].get("is_live").is_none(), "show has no liveness");
        assert_eq!(roster["members"][1]["node"], "mba", "explicit node wins");
        let legacy = run_fleet_command(&roster_args(&["show", "alpha", "--json"]));
        assert_eq!(legacy.code, 1, "flat session snapshots are not squad rosters");
    }

    #[test]
    fn fleet_roster_squads_fixture_and_flat_roster_migrates() {
        let _guard = env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let (root, _env) = roster_env("migration");
        let fleet_dir = root.join("state/fleet");
        std::fs::create_dir_all(fleet_dir.join("groups/01-3e")).expect("squad dir");
        std::fs::write(fleet_dir.join("groups/01-3e/group.json"), ROSTER_GROUP_FIXTURE).expect("legacy group fixture");
        std::fs::write(fleet_dir.join("01-3e.json"), r#"{"name":"01-3e","groupName":"3e","windows":[],"members":[]}"#).expect("duplicate flat roster");
        let shown: serde_json::Value = serde_json::from_str(&run_fleet_command(&roster_args(&["show", "3e", "--json"])).stdout).expect("json");
        assert_eq!(shown["memberCount"], 5);
        assert!(!fleet_dir.join("01-3e.json").exists());
        assert!(!fleet_dir.join("groups/01-3e/group.json").exists());
        assert!(fleet_dir.join("squads/01-3e/squad.json").exists());

        std::fs::write(fleet_dir.join("02-flat.json"), r#"{"name":"02-flat","groupName":"flat","windows":[],"members":[{"handle":"one"}]}"#).expect("flat roster");
        let migrated = run_fleet_command(&roster_args(&["show", "flat", "--json"]));
        assert_eq!(migrated.code, 0, "{}", migrated.stderr);
        assert!(!fleet_dir.join("02-flat.json").exists());
        assert!(fleet_dir.join("squads/02-flat/squad.json").exists());
        let migrated_file = std::fs::read_to_string(fleet_dir.join("squads/02-flat/squad.json")).expect("migrated flat");
        assert!(migrated_file.contains(r#""squadName": "flat""#), "{migrated_file}");
        assert!(!migrated_file.contains("groupName"), "{migrated_file}");
        assert_eq!(serde_json::from_str::<serde_json::Value>(&migrated.stdout).expect("json")["memberCount"], 1);
    }

    #[test]
    fn fleet_status_marks_live_members_from_tmux_inventory() {
        let sessions = vec![TmuxSession { name: "12-atlas".to_owned(), windows: Vec::new() }];
        let atlas = NativeFleetMember { handle: "atlas".to_owned(), ..NativeFleetMember::default() };
        let view = fleet_roster_member_view(atlas, None, Some(&sessions));
        assert_eq!((view.is_live, view.node), (Some(true), None));
        let ghost = NativeFleetMember { handle: "ghost".to_owned(), ..NativeFleetMember::default() };
        assert_eq!(fleet_roster_member_view(ghost, None, Some(&sessions)).is_live, Some(false));
        assert_eq!(fleet_roster_member_view(NativeFleetMember::default(), None, None).is_live, None);
    }
}
