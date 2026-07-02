//! XDG/legacy maw path resolver ported from maw-js `src/core/xdg.ts`,
//! `src/core/paths.ts`, and `src/cli/instance-preset.ts`.

use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MawCorePaths {
    pub runtime_home: PathBuf,
    pub config_dir: PathBuf,
    pub fleet_dir: PathBuf,
    pub config_file: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MawConfigScope {
    User,
    Project,
    Legacy,
}

impl MawConfigScope {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Project => "project",
            Self::Legacy => "legacy",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MawConfigLayerSource {
    pub path: PathBuf,
    pub weight: u32,
    pub is_local: bool,
    pub scope: MawConfigScope,
    pub scope_rank: u32,
    pub depth: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MergedMawConfig {
    pub config: Value,
    pub sources: Vec<MawConfigLayerSource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MawXdgEnv {
    home_dir: PathBuf,
    vars: BTreeMap<String, String>,
}

impl MawXdgEnv {
    #[must_use]
    pub fn new(home_dir: impl Into<PathBuf>) -> Self {
        Self {
            home_dir: home_dir.into(),
            vars: BTreeMap::new(),
        }
    }

    #[must_use]
    pub fn with_vars(
        home_dir: impl Into<PathBuf>,
        vars: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> Self {
        let mut env = Self::new(home_dir);
        for (key, value) in vars {
            env.vars.insert(key.into(), value.into());
        }
        env
    }

    #[must_use]
    pub fn var(&self, name: &str) -> Option<&str> {
        self.vars.get(name).map(String::as_str)
    }

    #[must_use]
    pub fn home_dir(&self) -> &Path {
        &self.home_dir
    }
}

#[must_use]
pub fn is_maw_xdg_enabled(env: &MawXdgEnv) -> bool {
    truthy_env(env.var("MAW_XDG"))
}

/// Resolve the maw instance home directory, mirroring maw-js `resolveHome()`.
#[must_use]
pub fn resolve_home(env: &MawXdgEnv) -> PathBuf {
    maw_runtime_home_dir(env)
}

/// Resolve import-time core paths from maw-js `src/core/paths.ts`.
#[must_use]
pub fn maw_core_paths(env: &MawXdgEnv) -> MawCorePaths {
    let runtime_home = resolve_home(env);
    let config_dir = maw_config_dir(env);
    let fleet_dir = config_dir.join("fleet");
    let config_file = config_dir.join("maw.config.json");
    MawCorePaths {
        runtime_home,
        config_dir,
        fleet_dir,
        config_file,
    }
}

/// Resolve core paths and create the fleet directory like maw-js module import.
///
/// # Errors
///
/// Returns any filesystem error from creating the `<config>/fleet` directory.
pub fn ensure_maw_core_paths(env: &MawXdgEnv) -> io::Result<MawCorePaths> {
    let paths = maw_core_paths(env);
    fs::create_dir_all(&paths.fleet_dir)?;
    Ok(paths)
}

/// Validate `maw serve --as <name>` instance names.
///
/// Mirrors maw-js `INSTANCE_NAME_RE`: lowercase alphanumeric first character,
/// then lowercase alphanumeric, `_`, or `-`, with max length 32.
#[must_use]
pub fn is_valid_instance_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if name.len() > 32 || !is_lower_alnum(first) {
        return false;
    }
    chars.all(|ch| is_lower_alnum(ch) || matches!(ch, '_' | '-'))
}

#[must_use]
pub fn maw_config_dir(env: &MawXdgEnv) -> PathBuf {
    if let Some(maw_home) = env.var("MAW_HOME") {
        return Path::new(maw_home).join("config");
    }
    if let Some(config_dir) = env.var("MAW_CONFIG_DIR") {
        return PathBuf::from(config_dir);
    }
    xdg_base(env, "XDG_CONFIG_HOME", &[".config"]).join("maw")
}

#[must_use]
pub fn maw_runtime_home_dir(env: &MawXdgEnv) -> PathBuf {
    if let Some(maw_home) = env.var("MAW_HOME") {
        return PathBuf::from(maw_home);
    }
    if is_maw_xdg_enabled(env) {
        maw_state_dir(env)
    } else {
        legacy_home(env)
    }
}

#[must_use]
pub fn maw_data_dir(env: &MawXdgEnv) -> PathBuf {
    if let Some(maw_home) = env.var("MAW_HOME") {
        return PathBuf::from(maw_home);
    }
    if let Some(data_dir) = env.var("MAW_DATA_DIR") {
        return PathBuf::from(data_dir);
    }
    if is_maw_xdg_enabled(env) {
        xdg_base(env, "XDG_DATA_HOME", &[".local", "share"]).join("maw")
    } else {
        legacy_home(env)
    }
}

#[must_use]
pub fn maw_state_dir(env: &MawXdgEnv) -> PathBuf {
    if let Some(maw_home) = env.var("MAW_HOME") {
        return PathBuf::from(maw_home);
    }
    if let Some(state_dir) = env.var("MAW_STATE_DIR") {
        return PathBuf::from(state_dir);
    }
    if is_maw_xdg_enabled(env) {
        xdg_base(env, "XDG_STATE_HOME", &[".local", "state"]).join("maw")
    } else {
        legacy_home(env)
    }
}

#[must_use]
pub fn maw_cache_dir(env: &MawXdgEnv) -> PathBuf {
    if let Some(maw_home) = env.var("MAW_HOME") {
        return PathBuf::from(maw_home);
    }
    if let Some(cache_dir) = env.var("MAW_CACHE_DIR") {
        return PathBuf::from(cache_dir);
    }
    if is_maw_xdg_enabled(env) {
        xdg_base(env, "XDG_CACHE_HOME", &[".cache"]).join("maw")
    } else {
        legacy_home(env)
    }
}

#[must_use]
pub fn maw_config_path(env: &MawXdgEnv, parts: &[&str]) -> PathBuf {
    join_parts(maw_config_dir(env), parts)
}

#[must_use]
pub fn maw_data_path(env: &MawXdgEnv, parts: &[&str]) -> PathBuf {
    join_parts(maw_data_dir(env), parts)
}

#[must_use]
pub fn maw_state_path(env: &MawXdgEnv, parts: &[&str]) -> PathBuf {
    join_parts(maw_state_dir(env), parts)
}

#[must_use]
pub fn maw_cache_path(env: &MawXdgEnv, parts: &[&str]) -> PathBuf {
    join_parts(maw_cache_dir(env), parts)
}

/// Discover maw config layers for a cwd-aware invocation.
///
/// Mirrors maw-js `src/core/paths.ts` and the `MAW_HOME` singleton inheritance
/// in `src/config/load.ts`.
#[must_use]
pub fn discover_config_layers(env: &MawXdgEnv, cwd: &Path) -> Vec<MawConfigLayerSource> {
    let mut found = Vec::new();
    let config_dir = maw_config_dir(env);
    let user_weighted = scan_config_dir(&config_dir, MawConfigScope::User, 20, 0);
    found.extend(user_weighted.iter().cloned());
    if user_weighted.is_empty() {
        let legacy = config_dir.join("maw.config.json");
        if legacy.exists() {
            found.push(MawConfigLayerSource {
                path: legacy,
                weight: 50,
                is_local: false,
                scope: MawConfigScope::Legacy,
                scope_rank: 20,
                depth: 0,
            });
        }
    }

    let mut chain = Vec::new();
    let mut dir = if cwd.is_absolute() {
        cwd.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(cwd)
    };
    for _ in 0..32 {
        chain.push(dir.clone());
        let Some(parent) = dir.parent() else {
            break;
        };
        if parent == dir {
            break;
        }
        dir = parent.to_path_buf();
    }
    chain.reverse();
    for (index, dir) in (0_u32..).zip(chain) {
        found.extend(scan_config_dir(
            &dir.join(".maw"),
            MawConfigScope::Project,
            30 + index,
            index,
        ));
    }

    sort_config_sources(&mut found);
    inherit_singleton_configs_for_maw_home(env, found)
}

#[must_use]
pub fn load_merged_config(env: &MawXdgEnv) -> MergedMawConfig {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    load_merged_config_in_dir(env, &cwd)
}

#[must_use]
pub fn load_merged_config_in_dir(env: &MawXdgEnv, cwd: &Path) -> MergedMawConfig {
    let mut sources = discover_config_layers(env, cwd);
    if sources.is_empty() {
        sources.push(legacy_config_source(
            maw_config_dir(env).join("maw.config.json"),
            20,
        ));
    }
    let config_file = maw_config_dir(env).join("maw.config.json");
    let mut merged = Value::Object(serde_json::Map::new());
    let mut loaded_any = false;
    for source in &sources {
        let Some(layer) = read_config_layer(&source.path) else {
            continue;
        };
        loaded_any = true;
        deep_merge_config(&mut merged, &layer);
    }
    if !loaded_any && !sources.iter().any(|source| source.path == config_file) {
        let legacy_source = legacy_config_source(config_file, 20);
        if let Some(layer) = read_config_layer(&legacy_source.path) {
            sources = vec![legacy_source];
            deep_merge_config(&mut merged, &layer);
        }
    }
    MergedMawConfig {
        config: merged,
        sources,
    }
}

pub fn deep_merge_config(target: &mut Value, layer: &Value) {
    let (Some(target_map), Some(layer_map)) = (target.as_object_mut(), layer.as_object()) else {
        *target = layer.clone();
        return;
    };
    for (key, value) in layer_map {
        if value.is_null() {
            target_map.remove(key);
        } else if value.is_object() && target_map.get(key).is_some_and(Value::is_object) {
            if let Some(target_child) = target_map.get_mut(key) {
                deep_merge_config(target_child, value);
            }
        } else if value.is_object() {
            let mut child = Value::Object(serde_json::Map::new());
            deep_merge_config(&mut child, value);
            target_map.insert(key.clone(), child);
        } else {
            target_map.insert(key.clone(), value.clone());
        }
    }
}

fn is_lower_alnum(ch: char) -> bool {
    ch.is_ascii_lowercase() || ch.is_ascii_digit()
}

fn truthy_env(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

fn absolute_env(env: &MawXdgEnv, name: &str) -> Option<PathBuf> {
    let value = env.var(name)?;
    let path = Path::new(value);
    path.is_absolute().then(|| path.to_path_buf())
}

fn legacy_home(env: &MawXdgEnv) -> PathBuf {
    env.home_dir().join(".maw")
}

fn xdg_base(env: &MawXdgEnv, env_name: &str, fallback: &[&str]) -> PathBuf {
    absolute_env(env, env_name)
        .unwrap_or_else(|| join_parts(env.home_dir().to_path_buf(), fallback))
}

fn scan_config_dir(
    dir: &Path,
    scope: MawConfigScope,
    scope_rank: u32,
    depth: u32,
) -> Vec<MawConfigLayerSource> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Some((weight, is_local)) = parse_config_layer_name(name) else {
            continue;
        };
        out.push(MawConfigLayerSource {
            path: entry.path(),
            weight,
            is_local,
            scope,
            scope_rank,
            depth,
        });
    }
    sort_config_sources(&mut out);
    out
}

fn parse_config_layer_name(name: &str) -> Option<(u32, bool)> {
    let rest = name.strip_prefix("maw.config.")?;
    let (digits, is_local) = rest.strip_suffix(".local.json").map_or_else(
        || rest.strip_suffix(".json").map(|value| (value, false)),
        |value| Some((value, true)),
    )?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Some((digits.parse().ok()?, is_local))
}

fn sort_config_sources(sources: &mut [MawConfigLayerSource]) {
    sources.sort_by(|a, b| {
        a.weight
            .cmp(&b.weight)
            .then(a.scope_rank.cmp(&b.scope_rank))
            .then(a.is_local.cmp(&b.is_local))
            .then(a.path.cmp(&b.path))
    });
}

fn inherit_singleton_configs_for_maw_home(
    env: &MawXdgEnv,
    sources: Vec<MawConfigLayerSource>,
) -> Vec<MawConfigLayerSource> {
    if env.var("MAW_TEST_MODE") == Some("1")
        || env.var("MAW_HOME").is_none()
        || env.var("MAW_CONFIG_DIR").is_some()
    {
        return sources;
    }
    let config_file = maw_config_dir(env).join("maw.config.json");
    let inherited = discover_inherited_singleton_configs(env, &config_file);
    if inherited.is_empty() {
        return sources;
    }
    let seen = sources
        .iter()
        .map(|source| source.path.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let mut merged = inherited
        .into_iter()
        .filter(|source| !seen.contains(&source.path))
        .chain(sources)
        .collect::<Vec<_>>();
    sort_config_sources(&mut merged);
    merged
}

fn discover_inherited_singleton_configs(
    env: &MawXdgEnv,
    active_config_file: &Path,
) -> Vec<MawConfigLayerSource> {
    let dir = singleton_config_dir(env);
    if Some(dir.as_path()) == active_config_file.parent() {
        return Vec::new();
    }
    let weighted = scan_config_dir(&dir, MawConfigScope::User, 10, 0);
    if !weighted.is_empty() {
        return weighted;
    }
    let legacy = dir.join("maw.config.json");
    if legacy.exists() {
        return vec![legacy_config_source(legacy, 10)];
    }
    Vec::new()
}

fn singleton_config_dir(env: &MawXdgEnv) -> PathBuf {
    xdg_base(env, "XDG_CONFIG_HOME", &[".config"]).join("maw")
}

fn legacy_config_source(path: PathBuf, scope_rank: u32) -> MawConfigLayerSource {
    MawConfigLayerSource {
        path,
        weight: 50,
        is_local: false,
        scope: MawConfigScope::Legacy,
        scope_rank,
        depth: 0,
    }
}

fn read_config_layer(path: &Path) -> Option<Value> {
    let raw = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<Value>(&raw).ok()?;
    value.is_object().then_some(value)
}

fn join_parts(mut base: PathBuf, parts: &[&str]) -> PathBuf {
    for part in parts {
        base.push(part);
    }
    base
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
