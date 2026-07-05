use maw_cli::run_cli;
use std::{
    fs,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_config_dir() -> std::path::PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "maw-rs-cli-hub-plan-test-{}-{unique}-{counter}",
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
fn hub_validate_plan_cli_matches_maw_js_workspace_reasons() {
    let cases = [
        (
            "missing id",
            vec!["--hub-url", "ws://hub", "--token", "t"],
            false,
            Some("missing/empty id"),
        ),
        (
            "missing hub url",
            vec!["--id", "ws", "--token", "t"],
            false,
            Some("missing/empty hubUrl"),
        ),
        (
            "missing token",
            vec!["--id", "ws", "--hub-url", "ws://hub"],
            false,
            Some("missing/empty token"),
        ),
        (
            "http hub url rejected",
            vec!["--id", "ws", "--hub-url", "http://hub", "--token", "t"],
            false,
            Some("hubUrl must be ws:|wss: (got http:)"),
        ),
        (
            "valid wss",
            vec![
                "--id",
                "ws",
                "--hub-url",
                "wss://hub",
                "--token",
                "t",
                "--shared-agent",
                "mawjs",
            ],
            true,
            None,
        ),
    ];

    for (name, flags, expected_ok, expected_reason) in cases {
        let mut argv = vec![
            "hub".to_owned(),
            "validate-workspace".to_owned(),
            "--plan-json".to_owned(),
        ];
        argv.extend(flags.into_iter().map(str::to_owned));
        let output = json(&run_cli(&argv));
        assert_eq!(output["command"], "hub", "{name}");
        assert_eq!(output["kind"], "validate-workspace", "{name}");
        assert_eq!(output["ok"], expected_ok, "{name}");
        match expected_reason {
            Some(reason) => assert_eq!(output["reason"], reason, "{name}"),
            None => assert!(output["reason"].is_null(), "{name}: {output}"),
        }
    }
}

#[test]
fn hub_load_plan_cli_creates_dir_keeps_valid_configs_and_reports_bad_files() {
    let config_dir = temp_config_dir();
    let output = json(&run_cli(&[
        "hub".to_owned(),
        "load-workspaces".to_owned(),
        "--config-dir".to_owned(),
        config_dir.display().to_string(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(
        output["configs"].as_array().expect("configs array").len(),
        0
    );
    assert_eq!(
        output["warnings"].as_array().expect("warnings array").len(),
        0
    );
    let workspaces = config_dir.join("workspaces");
    assert!(workspaces.exists());

    fs::write(
        workspaces.join("valid.json"),
        serde_json::json!({
            "id": "alpha",
            "hubUrl": "wss://hub.example.test",
            "token": "secret",
            "sharedAgents": ["mawjs"]
        })
        .to_string(),
    )
    .expect("valid fixture should write");
    fs::write(
        workspaces.join("invalid.json"),
        serde_json::json!({
            "id": "bad",
            "hubUrl": "https://not-websocket.example.test",
            "token": "secret",
            "sharedAgents": []
        })
        .to_string(),
    )
    .expect("invalid fixture should write");
    fs::write(workspaces.join("broken.json"), "{not json").expect("broken fixture should write");
    fs::write(workspaces.join("notes.txt"), "ignored").expect("non-json fixture should write");

    let output = json(&run_cli(&[
        "hub".to_owned(),
        "load-workspaces".to_owned(),
        "--config-dir".to_owned(),
        config_dir.display().to_string(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(output["command"], "hub");
    assert_eq!(output["kind"], "load-workspaces");
    assert_eq!(output["configs"][0]["id"], "alpha");
    assert_eq!(output["configs"][0]["hubUrl"], "wss://hub.example.test");
    assert_eq!(output["configs"][0]["token"], "secret");
    assert_eq!(
        output["configs"][0]["sharedAgents"],
        serde_json::json!(["mawjs"])
    );
    let warnings = output["warnings"]
        .as_array()
        .expect("warnings array")
        .iter()
        .map(|value| value.as_str().expect("warning string"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(warnings.contains("invalid workspace config: invalid.json"));
    assert!(warnings.contains("failed to parse workspace config: broken.json"));
}

#[test]
fn hub_plan_rejects_missing_values() {
    let output = run_cli(&[
        "hub".to_owned(),
        "load-workspaces".to_owned(),
        "--config-dir".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("missing --config-dir value"));
}
