use std::{
    fs,
    path::{Path, PathBuf},
};

use serde_json::Value;

use super::{
    paths::{maw_config_dir, xdg_base},
    types::{MawConfigLayerSource, MawConfigScope, MawXdgEnv, MergedMawConfig},
};

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
