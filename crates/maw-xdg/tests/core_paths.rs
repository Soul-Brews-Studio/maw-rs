#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
use maw_xdg::{ensure_maw_core_paths, is_valid_instance_name, MawXdgEnv};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

fn unique_temp_dir(label: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock is after unix epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("maw-rs-{label}-{}-{stamp}", std::process::id()))
}

fn env(home: &Path, vars: &[(&str, &str)]) -> MawXdgEnv {
    MawXdgEnv::with_vars(home.to_path_buf(), vars.iter().copied())
}

#[test]
fn defaults_to_singleton_home_and_config_directory() {
    let home = unique_temp_dir("paths-home");
    let paths = ensure_maw_core_paths(&env(&home, &[])).expect("fleet dir can be created");

    assert_eq!(paths.runtime_home, home.join(".maw"));
    assert_eq!(paths.config_dir, home.join(".config").join("maw"));
    assert_eq!(
        paths.fleet_dir,
        home.join(".config").join("maw").join("fleet")
    );
    assert_eq!(
        paths.config_file,
        home.join(".config").join("maw").join("maw.config.json")
    );
    assert!(paths.fleet_dir.exists());

    fs::remove_dir_all(home).ok();
}

#[test]
fn maw_home_controls_runtime_home_and_config_directory() {
    let home = unique_temp_dir("paths-home");
    let maw_home = unique_temp_dir("instance-home");
    let ignored_config = home.join("ignored-config");
    let maw_home_s = maw_home.to_string_lossy().into_owned();
    let ignored_config_s = ignored_config.to_string_lossy().into_owned();
    let paths = ensure_maw_core_paths(&env(
        &home,
        &[
            ("MAW_HOME", maw_home_s.as_str()),
            ("MAW_CONFIG_DIR", ignored_config_s.as_str()),
        ],
    ))
    .expect("fleet dir can be created");

    assert_eq!(paths.runtime_home, maw_home);
    assert_eq!(paths.config_dir, paths.runtime_home.join("config"));
    assert_eq!(
        paths.fleet_dir,
        paths.runtime_home.join("config").join("fleet")
    );
    assert_eq!(
        paths.config_file,
        paths.runtime_home.join("config").join("maw.config.json")
    );
    assert!(paths.fleet_dir.exists());

    fs::remove_dir_all(home).ok();
    fs::remove_dir_all(paths.runtime_home).ok();
}

#[test]
fn maw_config_dir_overrides_singleton_config_when_maw_home_is_unset() {
    let home = unique_temp_dir("paths-home");
    let config_dir = unique_temp_dir("config-dir");
    let config_dir_s = config_dir.to_string_lossy().into_owned();
    let paths = ensure_maw_core_paths(&env(&home, &[("MAW_CONFIG_DIR", config_dir_s.as_str())]))
        .expect("fleet dir can be created");

    assert_eq!(paths.runtime_home, home.join(".maw"));
    assert_eq!(paths.config_dir, config_dir);
    assert_eq!(paths.fleet_dir, paths.config_dir.join("fleet"));
    assert_eq!(paths.config_file, paths.config_dir.join("maw.config.json"));
    assert!(paths.fleet_dir.exists());

    fs::remove_dir_all(home).ok();
    fs::remove_dir_all(paths.config_dir).ok();
}

#[test]
fn instance_name_regex_accepts_valid_names() {
    for name in ["dev", "prod", "node-1", "a", "inst_2", "a1b2c3"] {
        assert!(is_valid_instance_name(name), "{name} should be valid");
    }
}

#[test]
fn instance_name_regex_rejects_invalid_names() {
    for name in [
        "",
        "-leading-dash",
        "Upper",
        "has space",
        "has.dot",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(!is_valid_instance_name(name), "{name} should be invalid");
    }
}
