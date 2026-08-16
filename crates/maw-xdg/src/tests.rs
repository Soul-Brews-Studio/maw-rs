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
