use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-costs-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], cwd: &Path, maw_home: &Path, projects_dir: &Path, plugins_dir: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("HOME", maw_home)
        .env("MAW_HOME", maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", plugins_dir)
        .env("MAW_CLAUDE_PROJECTS_DIR", projects_dir)
        .env("MAW_COSTS_TODAY", "2026-06-25")
        .output()
        .expect("run maw-rs")
}

fn install_costs_plugin(root: &Path) -> PathBuf {
    let plugin = root.join("plugins").join("costs");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(plugin.join("plugin.json"), include_str!("fixtures/native-costs/costs-plugin/plugin.json")).expect("plugin json");
    fs::write(plugin.join("plugin.wasm"), include_bytes!("fixtures/native-costs/costs-plugin/plugin.wasm")).expect("plugin wasm");
    root.join("plugins")
}

fn assistant_line(model: &str, timestamp: &str, input: u64, output: u64) -> String {
    serde_json::json!({
        "type": "assistant",
        "timestamp": timestamp,
        "message": {
            "model": model,
            "usage": {
                "input_tokens": input,
                "output_tokens": output,
                "cache_read_input_tokens": 0,
                "cache_creation_input_tokens": 0
            }
        }
    })
    .to_string()
}

fn seed_projects(projects: &Path) {
    let alpha = projects.join("-home-agent-github-com-tonkmac-alpha-oracle");
    let beta = projects.join("-tmp-random-beta-agent");
    let old = projects.join("-tmp-random-old-agent");
    fs::create_dir_all(&alpha).expect("alpha dir");
    fs::create_dir_all(&beta).expect("beta dir");
    fs::create_dir_all(&old).expect("old dir");
    fs::write(
        alpha.join("one.jsonl"),
        [
            "{bad json".to_owned(),
            serde_json::json!({"type":"user","message":{"content":"ignored"}}).to_string(),
            assistant_line(
                "claude-opus",
                "2026-06-25T01:00:00.000Z",
                1_000_000,
                1_000_000,
            ),
        ]
        .join("\n"),
    )
    .expect("alpha file");
    fs::write(
        beta.join("one.jsonl"),
        assistant_line(
            "claude-haiku",
            "2026-06-24T01:00:00.000Z",
            1_000_000,
            1_000_000,
        ),
    )
    .expect("beta file");
    fs::write(
        old.join("old.jsonl"),
        assistant_line("unknown", "1970-01-01T00:00:00.000Z", 1_000_000, 1_000_000),
    )
    .expect("old file");
}

#[test]
fn native_costs_summary_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("summary");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    let projects = root.join("claude-projects");
    let plugins = install_costs_plugin(&root);
    fs::create_dir_all(&cwd).expect("cwd");
    seed_projects(&projects);

    let output = run(&["costs"], &cwd, &maw_home, &projects, &plugins);

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-costs/summary.stdout")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_costs_daily_json_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("daily-json");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    let projects = root.join("claude-projects");
    let plugins = install_costs_plugin(&root);
    fs::create_dir_all(&cwd).expect("cwd");
    seed_projects(&projects);

    let output = run(
        &["costs", "--daily", "--days", "2", "--json"],
        &cwd,
        &maw_home,
        &projects,
        &plugins,
    );

    assert!(
        output.status.success(),
        "stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).expect("stdout"),
        include_str!("fixtures/native-costs/daily.json")
    );
    assert_eq!(String::from_utf8(output.stderr).expect("stderr"), "");
}

#[test]
fn native_costs_dispatcher_registration_removed_for_plugin_fallthrough() {
    assert_eq!(
        maw_cli::dispatcher_status("costs"),
        maw_cli::DispatchKind::NativeError
    );
}
