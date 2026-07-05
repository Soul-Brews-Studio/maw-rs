use maw_cli::run_cli;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_home(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "maw-rs-cli-xdg-{label}-{}-{unique}-{counter}",
        std::process::id()
    ))
}

fn json(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

#[test]
fn xdg_paths_plan_cli_matches_legacy_and_xdg_maw_js_cases() {
    let legacy_home = PathBuf::from("/home/tester");
    let legacy = json(&run_cli(&[
        "xdg".to_owned(),
        "paths".to_owned(),
        "--home".to_owned(),
        legacy_home.display().to_string(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(legacy["xdgEnabled"], false);
    assert_eq!(legacy["runtimeHome"], "/home/tester/.maw");
    assert_eq!(legacy["dataDir"], "/home/tester/.maw");
    assert_eq!(legacy["stateDir"], "/home/tester/.maw");
    assert_eq!(legacy["cacheDir"], "/home/tester/.maw");
    assert_eq!(legacy["configDir"], "/home/tester/.config/maw");
    assert_eq!(legacy["dataPath"], "/home/tester/.maw/plugins");
    assert_eq!(legacy["statePath"], "/home/tester/.maw/peers.json");
    assert_eq!(legacy["cachePath"], "/home/tester/.maw/registry-cache.json");
    assert_eq!(
        legacy["configPath"],
        "/home/tester/.config/maw/maw.config.json"
    );

    let xdg = json(&run_cli(&[
        "xdg".to_owned(),
        "paths".to_owned(),
        "--home".to_owned(),
        legacy_home.display().to_string(),
        "--env".to_owned(),
        "MAW_XDG=yes".to_owned(),
        "--env".to_owned(),
        "XDG_DATA_HOME=/xdg-data".to_owned(),
        "--env".to_owned(),
        "XDG_STATE_HOME=/xdg-state".to_owned(),
        "--env".to_owned(),
        "XDG_CACHE_HOME=/xdg-cache".to_owned(),
        "--env".to_owned(),
        "XDG_CONFIG_HOME=/xdg-config".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(xdg["xdgEnabled"], true);
    assert_eq!(xdg["runtimeHome"], "/xdg-state/maw");
    assert_eq!(xdg["dataDir"], "/xdg-data/maw");
    assert_eq!(xdg["stateDir"], "/xdg-state/maw");
    assert_eq!(xdg["cacheDir"], "/xdg-cache/maw");
    assert_eq!(xdg["configDir"], "/xdg-config/maw");

    let maw_home = json(&run_cli(&[
        "xdg".to_owned(),
        "paths".to_owned(),
        "--home".to_owned(),
        legacy_home.display().to_string(),
        "--env".to_owned(),
        "MAW_HOME=/maw-home".to_owned(),
        "--env".to_owned(),
        "MAW_XDG=on".to_owned(),
        "--env".to_owned(),
        "XDG_DATA_HOME=relative-data".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(maw_home["runtimeHome"], "/maw-home");
    assert_eq!(maw_home["configDir"], "/maw-home/config");
    assert_eq!(maw_home["dataDir"], "/maw-home");
    assert_eq!(maw_home["stateDir"], "/maw-home");
    assert_eq!(maw_home["cacheDir"], "/maw-home");
}

#[test]
fn xdg_core_paths_plan_cli_creates_fleet_dir_like_maw_js_import() {
    let home = temp_home("home");
    let output = json(&run_cli(&[
        "xdg".to_owned(),
        "core-paths".to_owned(),
        "--home".to_owned(),
        home.display().to_string(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(
        output["runtimeHome"],
        home.join(".maw").display().to_string()
    );
    assert_eq!(
        output["configDir"],
        home.join(".config").join("maw").display().to_string()
    );
    let fleet_dir = home.join(".config").join("maw").join("fleet");
    assert_eq!(output["fleetDir"], fleet_dir.display().to_string());
    assert!(fleet_dir.exists());
    fs::remove_dir_all(home).ok();
}

#[test]
fn xdg_paths_default_home_falls_back_to_tmp_when_home_env_is_missing() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = ENV_LOCK.lock().expect("env lock");
    let original_home = std::env::var_os("HOME");
    std::env::remove_var("HOME");

    let output = json(&run_cli(&[
        "xdg".to_owned(),
        "paths".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(output["runtimeHome"], "/tmp/.maw");

    if let Some(home) = original_home {
        std::env::set_var("HOME", home);
    }
}

#[test]
fn xdg_instance_name_plan_cli_matches_maw_js_regex() {
    for name in ["dev", "prod", "node-1", "a", "inst_2", "a1b2c3"] {
        let output = json(&run_cli(&[
            "xdg".to_owned(),
            "validate-instance".to_owned(),
            "--name".to_owned(),
            name.to_owned(),
            "--plan-json".to_owned(),
        ]));
        assert_eq!(output["valid"], true, "{name}");
    }

    for name in [
        "",
        "-leading-dash",
        "Upper",
        "has space",
        "has.dot",
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        let output = json(&run_cli(&[
            "xdg".to_owned(),
            "validate-instance".to_owned(),
            "--name".to_owned(),
            name.to_owned(),
            "--plan-json".to_owned(),
        ]));
        assert_eq!(output["valid"], false, "{name}");
    }
}

#[test]
fn xdg_plan_rejects_bad_env_shape() {
    let output = run_cli(&[
        "xdg".to_owned(),
        "paths".to_owned(),
        "--env".to_owned(),
        "MAW_XDG".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("--env must be KEY=VALUE"));
}
