use super::*;
use std::{
    fs,
    path::{Path, PathBuf},
};

fn temp_root(label: &str) -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "maw-xdg-config-{label}-{}-{seq}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("temp root");
    root
}

fn write_json(path: &Path, body: &str) {
    fs::create_dir_all(path.parent().expect("parent")).expect("parent dir");
    fs::write(path, body).expect("write json");
}

#[test]
fn layered_config_sorts_and_deep_merges_like_maw_js() {
    let root = temp_root("merge");
    let home = root.join("home");
    let cwd = root.join("repo/sub");
    fs::create_dir_all(&cwd).expect("cwd");
    let env = MawXdgEnv::with_vars(
        &home,
        [("XDG_CONFIG_HOME", root.join("xdg").display().to_string())],
    );
    write_json(
        &root.join("xdg/maw/maw.config.50.json"),
        r#"{"commands":{"default":"claude","omx":"base"},"env":{"A":"1","B":"1"},"arr":[1],"deleteMe":"yes"}"#,
    );
    write_json(
        &root.join("xdg/maw/maw.config.60.local.json"),
        r#"{"commands":{"omx":"local"},"env":{"B":"2"},"arr":[2],"deleteMe":null}"#,
    );
    write_json(
        &root.join("repo/.maw/maw.config.40.json"),
        r#"{"commands":{"early":"project-low"},"env":{"A":"project-low","Z":"0"}}"#,
    );
    write_json(
        &root.join("repo/.maw/maw.config.60.json"),
        r#"{"commands":{"project":"codex"},"env":{"C":"3"}}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &cwd);

    assert_eq!(
        loaded
            .sources
            .iter()
            .map(|source| (source.weight, source.scope.as_str(), source.is_local))
            .collect::<Vec<_>>(),
        vec![
            (40, "project", false),
            (50, "user", false),
            (60, "user", true),
            (60, "project", false)
        ]
    );
    assert_eq!(loaded.config["commands"]["default"], "claude");
    assert_eq!(loaded.config["commands"]["early"], "project-low");
    assert_eq!(loaded.config["commands"]["omx"], "local");
    assert_eq!(loaded.config["commands"]["project"], "codex");
    assert_eq!(
        loaded.config["env"],
        serde_json::json!({"A": "1", "B": "2", "C": "3", "Z": "0"})
    );
    assert_eq!(loaded.config["arr"], serde_json::json!([2]));
    assert!(loaded.config.get("deleteMe").is_none());
    let _ = fs::remove_dir_all(root);
}

#[test]
fn weighted_config_loads_without_base_file() {
    let root = temp_root("weighted-only");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [("XDG_CONFIG_HOME", root.join("xdg").display().to_string())],
    );
    write_json(
        &root.join("xdg/maw/maw.config.50.json"),
        r#"{"commands":{"omx":"CODEX_HOME=$PWD/.codex omx --direct","codex-t1":"codex --profile t1"}}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &root);

    assert_eq!(loaded.sources.len(), 1);
    assert_eq!(
        loaded.config["commands"]["omx"],
        "CODEX_HOME=$PWD/.codex omx --direct"
    );
    assert_eq!(loaded.config["commands"]["codex-t1"], "codex --profile t1");
    let _ = fs::remove_dir_all(root);
}

/// #840: the un-numbered `maw.config.json` is a fallback, not a layer. Callers that
/// want to name "the config file" must go through `discover_user_config_layers`
/// rather than joining the un-numbered name onto the config dir.
#[test]
fn user_config_layers_drop_the_un_numbered_file_once_a_numbered_layer_exists() {
    let root = temp_root("user-layers");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [("MAW_CONFIG_DIR", root.join("cfg").display().to_string())],
    );

    // Alone, the un-numbered file is the whole set.
    write_json(
        &root.join("cfg/maw.config.json"),
        r#"{"node":"unnumbered"}"#,
    );
    assert_eq!(
        discover_user_config_layers(&env)
            .into_iter()
            .map(|source| (source.scope.as_str(), source.weight, source.path))
            .collect::<Vec<_>>(),
        vec![("legacy", 50, root.join("cfg/maw.config.json"))]
    );

    // Add numbered layers and the un-numbered file drops out entirely, in the same
    // ascending-weight order the merge applies.
    write_json(
        &root.join("cfg/maw.config.70.json"),
        r#"{"node":"seventy"}"#,
    );
    write_json(&root.join("cfg/maw.config.30.json"), r#"{"node":"thirty"}"#);
    assert_eq!(
        discover_user_config_layers(&env)
            .into_iter()
            .map(|source| (source.scope.as_str(), source.weight, source.path))
            .collect::<Vec<_>>(),
        vec![
            ("user", 30, root.join("cfg/maw.config.30.json")),
            ("user", 70, root.join("cfg/maw.config.70.json")),
        ]
    );

    // And what the full loader reports for this dir agrees, so the extracted helper
    // cannot drift from `discover_config_layers`.
    assert_eq!(
        discover_config_layers(&env, &root)
            .into_iter()
            .filter(|source| source.path.parent() == Some(root.join("cfg").as_path()))
            .map(|source| source.path)
            .collect::<Vec<_>>(),
        vec![
            root.join("cfg/maw.config.30.json"),
            root.join("cfg/maw.config.70.json"),
        ]
    );

    // The merge really does ignore the un-numbered file: "unnumbered" never wins.
    let loaded = load_merged_config_in_dir(&env, &root);
    assert_eq!(loaded.config["node"], "seventy");

    let _ = fs::remove_dir_all(root);
}

#[test]
fn maw_home_instance_overrides_singleton_config() {
    let root = temp_root("maw-home");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [
            ("MAW_HOME", root.join("instance").display().to_string()),
            ("XDG_CONFIG_HOME", root.join("xdg").display().to_string()),
        ],
    );
    write_json(
        &root.join("xdg/maw/maw.config.50.json"),
        r#"{"commands":{"omx":"base","default":"claude"}}"#,
    );
    write_json(
        &root.join("instance/config/maw.config.50.json"),
        r#"{"commands":{"omx":"instance"}}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &root);

    assert_eq!(
        loaded
            .sources
            .iter()
            .map(|source| (
                source.scope.as_str(),
                source.scope_rank,
                source.path.clone()
            ))
            .collect::<Vec<_>>(),
        vec![
            ("user", 10, root.join("xdg/maw/maw.config.50.json")),
            ("user", 20, root.join("instance/config/maw.config.50.json")),
        ]
    );
    assert_eq!(loaded.config["commands"]["default"], "claude");
    assert_eq!(loaded.config["commands"]["omx"], "instance");
    let _ = fs::remove_dir_all(root);
}

/// #874: `namedPeers` is a list of *named records*, not an ordered list of
/// values. A higher-weight layer must add to (and override entries of) the
/// lower layer's peers, never silently substitute its own list — the failure
/// mode was invisible at the config layer and only surfaced downstream as
/// `node '<name>' not in namedPeers or peers`.
#[test]
fn named_peers_merge_by_name_across_weighted_layers() {
    let root = temp_root("named-peers-merge");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [("MAW_CONFIG_DIR", root.join("cfg").display().to_string())],
    );
    write_json(
        &root.join("cfg/maw.config.50.json"),
        r#"{"namedPeers":[{"name":"white","url":"http://white:3456"},{"name":"mba","url":"http://mba:3456","pubKey":"mba-key"}]}"#,
    );
    write_json(
        &root.join("cfg/maw.config.51.json"),
        r#"{"namedPeers":[{"name":"mba","url":"http://mba.wg:3457"},{"name":"clinic","url":"http://clinic:3456"}]}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &root);

    // The weight-50 peer the weight-51 layer never mentions survives; the one it
    // does mention is replaced whole (a peer's url/pubKey are one credential set,
    // so a half-merged entry could pair a new url with a stale key); and the new
    // peer is appended.
    assert_eq!(
        loaded.config["namedPeers"],
        serde_json::json!([
            {"name":"white","url":"http://white:3456"},
            {"name":"mba","url":"http://mba.wg:3457"},
            {"name":"clinic","url":"http://clinic:3456"},
        ])
    );
    let _ = fs::remove_dir_all(root);
}

/// #874's concrete user exposure: `MAW_HOME` set without `MAW_CONFIG_DIR` makes
/// `inherit_singleton_configs_for_maw_home` add the singleton config as a
/// `scope_rank`-10 layer (deliberate, documented). A personal instance layer at
/// weight 51 then used to erase every peer that inheritance brought in.
#[test]
fn maw_home_inherited_named_peers_survive_a_higher_weight_instance_layer() {
    let root = temp_root("named-peers-maw-home");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [
            ("MAW_HOME", root.join("instance").display().to_string()),
            ("XDG_CONFIG_HOME", root.join("xdg").display().to_string()),
        ],
    );
    write_json(
        &root.join("xdg/maw/maw.config.50.json"),
        r#"{"namedPeers":[{"name":"white","url":"http://white:3456"}]}"#,
    );
    write_json(
        &root.join("instance/config/maw.config.51.json"),
        r#"{"namedPeers":[{"name":"black","url":"http://black:3456"}]}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &root);

    assert_eq!(
        loaded
            .sources
            .iter()
            .map(|source| (source.weight, source.scope_rank))
            .collect::<Vec<_>>(),
        vec![(50, 10), (51, 20)]
    );
    assert_eq!(
        loaded.config["namedPeers"],
        serde_json::json!([
            {"name":"white","url":"http://white:3456"},
            {"name":"black","url":"http://black:3456"},
        ])
    );
    let _ = fs::remove_dir_all(root);
}

/// #874 scoping guard: name-keyed merging is deliberately limited to
/// `namedPeers`. Every other array-valued key — including one whose elements
/// happen to carry a `name` — keeps wholesale replacement, and the
/// weight-then-`scope_rank` ordering that decides who wins is untouched: at
/// equal weight the higher `scope_rank` still wins, now per *entry* rather than
/// per list.
#[test]
fn only_named_peers_merges_by_name_and_scope_rank_still_breaks_weight_ties() {
    let root = temp_root("named-peers-scope");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [
            ("MAW_HOME", root.join("instance").display().to_string()),
            ("XDG_CONFIG_HOME", root.join("xdg").display().to_string()),
        ],
    );
    write_json(
        &root.join("xdg/maw/maw.config.50.json"),
        r#"{"namedPeers":[{"name":"white","url":"singleton"}],"widgets":[{"name":"alpha"},{"name":"beta"}],"peers":["http://a"],"arr":[1]}"#,
    );
    write_json(
        &root.join("instance/config/maw.config.50.json"),
        r#"{"namedPeers":[{"name":"white","url":"instance"}],"widgets":[{"name":"gamma"}],"peers":["http://b"],"arr":[2]}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &root);

    // Same weight, scope_rank 10 then 20 — unchanged ordering.
    assert_eq!(
        loaded
            .sources
            .iter()
            .map(|source| (source.weight, source.scope_rank))
            .collect::<Vec<_>>(),
        vec![(50, 10), (50, 20)]
    );
    // namedPeers: the tie-break still decides, per entry.
    assert_eq!(
        loaded.config["namedPeers"],
        serde_json::json!([{"name":"white","url":"instance"}])
    );
    // Every other array — named elements or not — is still replaced wholesale.
    assert_eq!(
        loaded.config["widgets"],
        serde_json::json!([{"name":"gamma"}])
    );
    assert_eq!(loaded.config["peers"], serde_json::json!(["http://b"]));
    assert_eq!(loaded.config["arr"], serde_json::json!([2]));
    let _ = fs::remove_dir_all(root);
}

/// #874 escape hatch: the existing `null` deletion rule is the way to drop
/// inherited peers wholesale, and it still applies to `namedPeers`. Without
/// this a layered user could add peers but never remove one.
#[test]
fn a_null_named_peers_layer_still_clears_the_inherited_list() {
    let root = temp_root("named-peers-null");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [("MAW_CONFIG_DIR", root.join("cfg").display().to_string())],
    );
    write_json(
        &root.join("cfg/maw.config.50.json"),
        r#"{"namedPeers":[{"name":"white","url":"http://white:3456"}]}"#,
    );
    write_json(
        &root.join("cfg/maw.config.51.json"),
        r#"{"namedPeers":null}"#,
    );

    let loaded = load_merged_config_in_dir(&env, &root);

    assert!(loaded.config.get("namedPeers").is_none());
    let _ = fs::remove_dir_all(root);
}

/// #874: `namedPeers` also has an object form (`{"<name>": "<url>"}`), which
/// the object branch already deep-merges correctly. Switching *forms* between
/// layers must not try to key an array against an object — the later form wins
/// whole, as it did before.
#[test]
fn named_peers_object_form_deep_merges_and_form_switches_replace() {
    let root = temp_root("named-peers-object");
    let home = root.join("home");
    let env = MawXdgEnv::with_vars(
        &home,
        [("MAW_CONFIG_DIR", root.join("cfg").display().to_string())],
    );
    write_json(
        &root.join("cfg/maw.config.50.json"),
        r#"{"namedPeers":{"white":"http://white:3456","mba":"http://mba:3456"}}"#,
    );
    write_json(
        &root.join("cfg/maw.config.51.json"),
        r#"{"namedPeers":{"mba":"http://mba.wg:3457"}}"#,
    );
    assert_eq!(
        load_merged_config_in_dir(&env, &root).config["namedPeers"],
        serde_json::json!({"white":"http://white:3456","mba":"http://mba.wg:3457"})
    );

    write_json(
        &root.join("cfg/maw.config.52.json"),
        r#"{"namedPeers":[{"name":"clinic","url":"http://clinic:3456"}]}"#,
    );
    assert_eq!(
        load_merged_config_in_dir(&env, &root).config["namedPeers"],
        serde_json::json!([{"name":"clinic","url":"http://clinic:3456"}])
    );
    let _ = fs::remove_dir_all(root);
}
