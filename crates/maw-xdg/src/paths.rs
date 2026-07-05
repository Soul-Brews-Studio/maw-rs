use std::{fs, io, path::{Path, PathBuf}};

use super::types::{MawCorePaths, MawXdgEnv};

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


fn is_lower_alnum(ch: char) -> bool {
    ch.is_ascii_lowercase() || ch.is_ascii_digit()
}

pub(super) fn truthy_env(value: Option<&str>) -> bool {
    value.is_some_and(|value| matches!(value.to_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

pub(super) fn absolute_env(env: &MawXdgEnv, name: &str) -> Option<PathBuf> {
    let value = env.var(name)?;
    let path = Path::new(value);
    path.is_absolute().then(|| path.to_path_buf())
}

pub(super) fn legacy_home(env: &MawXdgEnv) -> PathBuf {
    env.home_dir().join(".maw")
}

pub(super) fn xdg_base(env: &MawXdgEnv, env_name: &str, fallback: &[&str]) -> PathBuf {
    absolute_env(env, env_name)
        .unwrap_or_else(|| join_parts(env.home_dir().to_path_buf(), fallback))
}

pub(super) fn join_parts(mut base: PathBuf, parts: &[&str]) -> PathBuf {
    for part in parts {
        base.push(part);
    }
    base
}
