use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_cli::{run_cli, CliOutput};
use serde_json::Value;

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn run(values: &[&str]) -> CliOutput {
    run_cli(&args(values))
}

fn ok(values: &[&str]) -> CliOutput {
    let output = run(values);
    assert_eq!(output.code, 0, "stderr for {values:?}: {}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "stderr for {values:?}: {}",
        output.stderr
    );
    output
}

fn ok_json(values: &[&str]) -> Value {
    let output = ok(values);
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid json: {error}\n{}", output.stdout))
}

fn usage(values: &[&str], expected: &str) {
    let output = run(values);
    assert_eq!(output.code, 2, "stdout for {values:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {values:?} did not contain {expected:?}: {}",
        output.stderr
    );
    assert!(
        output.stdout.is_empty(),
        "stdout for {values:?}: {}",
        output.stdout
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-cli-tail-final-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn plugin_manifest_tail_parser_and_json_edges_are_stable() {
    let root = temp_dir("plugin-manifest");
    let plugin = root.join("rich-plugin");
    create_dir_all(&plugin).expect("plugin dir");
    write(
        plugin.join("plugin.json"),
        r#"{"name":"rich-plugin","version":"1.2.3","sdk":"*","target":"js","entry":"index.ts","module":{"path":"./module.ts","exports":["answer"]},"cli":{"command":"rich","aliases":["rp"],"help":"Rich plugin","flags":{"loud":"boolean"}},"api":{"path":"/rich","methods":["POST"]}}"#,
    )
    .expect("manifest");
    write(plugin.join("index.ts"), "export default () => null;").expect("entry");
    write(
        plugin.join("module.ts"),
        "export const answer = 'forty-two';",
    )
    .expect("module");

    let root_arg = root.to_string_lossy();
    let plugin_arg = plugin.to_string_lossy();
    let parsed = ok_json(&[
        "plugin-manifest",
        "parse",
        "--dir",
        plugin_arg.as_ref(),
        "--json",
        r#"{"name":"inline-rich","version":"1.0.0","sdk":"*","cli":{"command":"inline","aliases":["in"],"flags":{"dry-run":"boolean"}},"api":{"path":"/inline","methods":["GET"]}}"#,
        "--plan-json",
    ]);
    assert_eq!(
        parsed["manifest"]["cli"]["aliases"],
        serde_json::json!(["in"])
    );

    let loaded = ok_json(&[
        "plugin-manifest",
        "load",
        "--dir",
        plugin_arg.as_ref(),
        "--plan-json",
    ]);
    assert_eq!(
        loaded["plugin"]["manifest"]["cli"]["aliases"],
        serde_json::json!(["rp"])
    );

    let imported = ok_json(&[
        "plugin-manifest",
        "import-symbol",
        "--scan-dir",
        root_arg.as_ref(),
        "--disabled",
        "other-plugin",
        "--runtime-version",
        "9.9.9",
        "--plugin",
        "rich-plugin",
        "--symbol",
        "answer",
        "--module-symbol",
        "answer=forty-two",
        "--plan-json",
    ]);
    assert_eq!(imported["value"], "forty-two");
    assert!(imported["modulePath"]
        .as_str()
        .unwrap()
        .ends_with("module.ts"));

    usage(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "rich-plugin",
            "--source",
        ],
        "plugin-manifest: missing --source value",
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn pair_api_and_discover_tail_text_edges_are_stable() {
    let generated = ok_json(&[
        "pair-api",
        "generate",
        "--code",
        "ABC123",
        "--node",
        "mba",
        "--oracle",
        "homekeeper",
        "--port",
        "4444",
        "--base-url",
        "http://127.0.0.1:4444",
        "--federation-token",
        "token",
        "--pubkey",
        "pub",
        "--now",
        "1000",
        "--ttl-ms",
        "90000",
        "--plan-json",
    ]);
    assert_eq!(generated["ttlMs"], 90000);

    let discover = ok(&[
        "discover",
        "--oracle",
        "neo|manifest|mba|neo-session|neo-window|owner/neo|-|true|false",
        "--pane",
        "%1|claude|neo-session:1.0|neo-window|100|/repo|10",
    ]);
    assert!(
        discover.stdout.contains("neo offline"),
        "{}",
        discover.stdout
    );
}

#[test]
fn route_worktree_calver_and_ls_tail_edges_are_stable() {
    let route = ok_json(&[
        "route",
        "--query",
        "ghost",
        "--node",
        "local",
        "--session",
        "busy-oracle",
        "--window",
        "0:one:true",
        "--window",
        "1:two:false",
        "--plan-json",
    ]);
    assert_eq!(route["type"], "error");
    assert!(route.get("hint").is_some(), "{route}");

    usage(
        &[
            "route",
            "--query",
            "neo",
            "--session",
            "neo",
            "--window",
            "",
        ],
        "route: invalid window index",
    );

    usage(
        &[
            "worktree-window",
            "--main-repo-name",
            "maw-rs",
            "--wt-name",
            "maw-rs.wt-1-cli-tail",
            "--session",
            "maw-rs",
            "--window",
            "",
        ],
        "worktree-window: invalid window index",
    );

    usage(
        &["calver", "--now", "2026-5-21T10:00:30"],
        "calver: --now time must use HH:MM",
    );

    let ls = ok_json(&["ls", "--plan-json"]);
    assert_eq!(ls["command"], "ls");
}
