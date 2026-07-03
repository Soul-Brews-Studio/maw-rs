use maw_cli::{run_cli, CliOutput};
use serde_json::json;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_dir(label: &str) -> PathBuf {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-cli-text-error-{label}-{}-{unique}-{counter}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

fn assert_ok_text(output: &CliOutput, expected: &str) {
    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(output.stdout, expected);
    assert!(output.stderr.is_empty(), "{}", output.stderr);
}

fn assert_usage_error(output: &CliOutput, expected: &str) {
    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty(), "{}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "expected {expected:?} in stderr:\n{}",
        output.stderr
    );
}

#[test]
fn plugin_scaffold_text_rendering_and_parser_errors_are_stable() {
    assert_ok_text(
        &run_cli(&[
            "plugin-scaffold".to_owned(),
            "validate-name".to_owned(),
            "--name".to_owned(),
            "good-plugin".to_owned(),
        ]),
        "valid\n",
    );
    assert_ok_text(
        &run_cli(&[
            "plugin-scaffold".to_owned(),
            "validate-name".to_owned(),
            "--name".to_owned(),
            "Bad Plugin".to_owned(),
        ]),
        "\"Bad Plugin\" is invalid — use lowercase letters, digits, - or _ (must start with a letter)\n",
    );

    let manifest = run_cli(&[
        "plugin-scaffold".to_owned(),
        "manifest".to_owned(),
        "--name".to_owned(),
        "my-plugin".to_owned(),
        "--as".to_owned(),
    ]);
    assert_eq!(manifest.code, 0, "{}", manifest.stderr);
    let manifest_json: serde_json::Value =
        serde_json::from_str(&manifest.stdout).expect("manifest stdout is JSON text");
    assert_eq!(manifest_json["name"], "my-plugin");
    assert_eq!(manifest_json["wasm"], "./build/release.wasm");

    assert_usage_error(
        &run_cli(&[
            "plugin-scaffold".to_owned(),
            "validate-name".to_owned(),
            "--name".to_owned(),
        ]),
        "plugin-scaffold: missing --name value",
    );
    assert_usage_error(
        &run_cli(&["plugin-scaffold".to_owned(), "unknown".to_owned()]),
        "plugin-scaffold: unknown subcommand unknown",
    );
    assert_usage_error(
        &run_cli(&[
            "plugin-scaffold".to_owned(),
            "manifest".to_owned(),
            "--name".to_owned(),
            "Bad".to_owned(),
            "--rust".to_owned(),
        ]),
        "Invalid plugin name",
    );
}

#[test]
fn plugin_manifest_text_rendering_and_parser_errors_are_stable() {
    let root = temp_dir("manifest-text");
    let plugins_dir = root.join("plugins");
    write(
        root.join("index.ts"),
        b"export default async function plugin() {}\n",
    )
    .expect("parse entry");
    create_dir_all(&plugins_dir).expect("plugins");
    write_ts_plugin(&plugins_dir, "alpha", serde_json::Map::new());
    write_wasm_plugin(&plugins_dir, "wasm-plug");

    let manifest = json!({
        "name": "parsed-plugin",
        "version": "1.0.0",
        "sdk": "*",
        "target": "js",
        "entry": "index.ts"
    });
    assert_ok_text(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "parse".to_owned(),
            "--dir".to_owned(),
            root.display().to_string(),
            "--json".to_owned(),
            manifest.to_string(),
        ]),
        "parsed-plugin\n",
    );
    assert_ok_text(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "load".to_owned(),
            "--dir".to_owned(),
            root.join("missing").display().to_string(),
        ]),
        "missing\n",
    );
    assert_ok_text(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "load".to_owned(),
            "--dir".to_owned(),
            plugins_dir.join("alpha").display().to_string(),
        ]),
        "ts alpha\n",
    );
    let discovered = run_cli(&[
        "plugin-manifest".to_owned(),
        "discover".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.display().to_string(),
    ]);
    assert_eq!(discovered.code, 0, "{}", discovered.stderr);
    let mut discovered_names = discovered.stdout.lines().collect::<Vec<_>>();
    discovered_names.sort_unstable();
    assert_eq!(discovered_names, ["alpha", "wasm-plug"]);
    assert!(discovered.stderr.is_empty(), "{}", discovered.stderr);
    assert_ok_text(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "import-symbol".to_owned(),
            "--scan-dir".to_owned(),
            plugins_dir.display().to_string(),
            "--plugin".to_owned(),
            "alpha".to_owned(),
            "--symbol".to_owned(),
            "answer".to_owned(),
            "--module-symbol".to_owned(),
            "answer=42".to_owned(),
        ]),
        "42\n",
    );
    assert_usage_error(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "invoke".to_owned(),
            "--scan-dir".to_owned(),
            plugins_dir.display().to_string(),
            "--plugin".to_owned(),
            "alpha".to_owned(),
        ]),
        "TS source plugin 'alpha' is not executable",
    );
    let invalid_wasm = run_cli(&[
        "plugin-manifest".to_owned(),
        "invoke".to_owned(),
        "--scan-dir".to_owned(),
        plugins_dir.display().to_string(),
        "--plugin".to_owned(),
        "wasm-plug".to_owned(),
    ]);
    assert_eq!(invalid_wasm.code, 0, "{}", invalid_wasm.stderr);
    assert!(
        invalid_wasm.stdout.contains("wasm instantiation failed"),
        "{}",
        invalid_wasm.stdout
    );
    assert!(invalid_wasm.stderr.is_empty(), "{}", invalid_wasm.stderr);

    assert_usage_error(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "parse".to_owned(),
            "--json".to_owned(),
        ]),
        "plugin-manifest: missing --json value",
    );
    assert_usage_error(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "discover".to_owned(),
            "--runtime-version".to_owned(),
            "1.0.0".to_owned(),
        ]),
        "plugin-manifest discover: --scan-dir is required",
    );
    assert_usage_error(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "invoke".to_owned(),
            "--scan-dir".to_owned(),
            plugins_dir.display().to_string(),
            "--source".to_owned(),
            "socket".to_owned(),
        ]),
        "plugin-manifest invoke: unknown --source socket",
    );
    assert_usage_error(
        &run_cli(&[
            "plugin-manifest".to_owned(),
            "import-symbol".to_owned(),
            "--scan-dir".to_owned(),
            plugins_dir.display().to_string(),
            "--plugin".to_owned(),
            "alpha".to_owned(),
        ]),
        "plugin-manifest import-symbol: --symbol is required",
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn hub_xdg_feed_fuzzy_text_rendering_and_parser_errors_are_stable() {
    let hub_dir = temp_dir("hub");
    assert_ok_text(
        &run_cli(&[
            "hub".to_owned(),
            "validate-workspace".to_owned(),
            "--id".to_owned(),
            "ws".to_owned(),
            "--hub-url".to_owned(),
            "wss://hub.example.test".to_owned(),
            "--token".to_owned(),
            "secret".to_owned(),
        ]),
        "ok\n",
    );
    assert_ok_text(
        &run_cli(&[
            "hub".to_owned(),
            "validate-workspace".to_owned(),
            "--id".to_owned(),
            "ws".to_owned(),
            "--hub-url".to_owned(),
            "http://hub.example.test".to_owned(),
            "--token".to_owned(),
            "secret".to_owned(),
        ]),
        "invalid: hubUrl must be ws:|wss: (got http:)\n",
    );
    assert_ok_text(
        &run_cli(&[
            "hub".to_owned(),
            "load-workspaces".to_owned(),
            "--config-dir".to_owned(),
            hub_dir.display().to_string(),
        ]),
        "configs=0 warnings=0\n",
    );
    assert_usage_error(
        &run_cli(&["hub".to_owned(), "load-workspaces".to_owned()]),
        "hub load-workspaces: --config-dir is required",
    );

    assert_ok_text(
        &run_cli(&[
            "xdg".to_owned(),
            "paths".to_owned(),
            "--home".to_owned(),
            "/home/tester".to_owned(),
        ]),
        "/home/tester/.maw\n",
    );
    assert_ok_text(
        &run_cli(&[
            "xdg".to_owned(),
            "validate-instance".to_owned(),
            "--name".to_owned(),
            "Upper".to_owned(),
        ]),
        "false\n",
    );
    assert_usage_error(
        &run_cli(&[
            "xdg".to_owned(),
            "validate-instance".to_owned(),
            "--name".to_owned(),
        ]),
        "xdg: missing --name value",
    );

    assert_ok_text(
        &run_cli(&[
            "feed".to_owned(),
            "describe".to_owned(),
            "Notification".to_owned(),
            "--message".to_owned(),
            "ping".to_owned(),
        ]),
        "🔔 ping\n",
    );
    assert_ok_text(
        &run_cli(&[
            "feed".to_owned(),
            "parse-line".to_owned(),
            "2026-05-18 12:34:56 | alpha | m5 | Notification | /repo | sess » hello".to_owned(),
        ]),
        "hello\n",
    );
    let bad_feed = run_cli(&["feed".to_owned(), "parse-line".to_owned(), "bad".to_owned()]);
    assert_eq!(bad_feed.code, 1);
    assert!(bad_feed.stdout.is_empty());
    assert_usage_error(
        &run_cli(&["feed".to_owned(), "active".to_owned(), "--now".to_owned()]),
        "feed: missing --now value",
    );
    assert_usage_error(
        &run_cli(&[
            "feed".to_owned(),
            "active".to_owned(),
            "--event".to_owned(),
            "alpha".to_owned(),
        ]),
        "feed: --event must be oracle:ts:message",
    );

    assert_ok_text(
        &run_cli(&[
            "fuzzy".to_owned(),
            "distance".to_owned(),
            "kitten".to_owned(),
            "sitting".to_owned(),
        ]),
        "3\n",
    );
    assert_ok_text(
        &run_cli(&[
            "fuzzy".to_owned(),
            "match".to_owned(),
            "oracl".to_owned(),
            "--candidate".to_owned(),
            "oracle".to_owned(),
            "--candidate".to_owned(),
            "plugin".to_owned(),
        ]),
        "oracle\n",
    );
    assert_usage_error(
        &run_cli(&[
            "fuzzy".to_owned(),
            "distance".to_owned(),
            "only-left".to_owned(),
        ]),
        "fuzzy: missing distance right value",
    );
    assert_usage_error(
        &run_cli(&[
            "fuzzy".to_owned(),
            "match".to_owned(),
            "oracle".to_owned(),
            "--max-distance".to_owned(),
            "far".to_owned(),
        ]),
        "fuzzy: --max-distance must be a non-negative integer",
    );

    remove_dir_all(hub_dir).expect("cleanup hub");
}

#[test]
fn route_resolve_worktree_calver_text_rendering_and_parser_errors_are_stable() {
    assert_ok_text(
        &run_cli(&[
            "route".to_owned(),
            "--query".to_owned(),
            "alpha".to_owned(),
            "--session".to_owned(),
            "local".to_owned(),
            "--window".to_owned(),
            "1:alpha:true".to_owned(),
        ]),
        "route alpha: local local:1\n",
    );
    assert_ok_text(
        &run_cli(&[
            "route".to_owned(),
            "--query".to_owned(),
            "remote:agent".to_owned(),
            "--node".to_owned(),
            "local".to_owned(),
            "--named-peer".to_owned(),
            "remote=wss://remote.example".to_owned(),
        ]),
        "route remote:agent: peer remote agent via wss://remote.example\n",
    );
    assert_ok_text(
        &run_cli(&[
            "route".to_owned(),
            "--query".to_owned(),
            "ghost".to_owned(),
            "--session".to_owned(),
            "local".to_owned(),
            "--window".to_owned(),
            "1:alpha:true".to_owned(),
        ]),
        "route ghost: error not_found 'ghost' not in local sessions or agents map hint=check: maw ls\n",
    );
    assert_usage_error(
        &run_cli(&[
            "route".to_owned(),
            "--query".to_owned(),
            "alpha".to_owned(),
            "--named-peer".to_owned(),
            "broken".to_owned(),
        ]),
        "route: --named-peer must use <name=url>",
    );
    assert_usage_error(
        &run_cli(&[
            "route".to_owned(),
            "--query".to_owned(),
            "alpha".to_owned(),
            "--session".to_owned(),
            "local".to_owned(),
            "--window".to_owned(),
            "x:alpha:true".to_owned(),
        ]),
        "route: invalid window index",
    );

    assert_ok_text(
        &run_cli(&[
            "resolve".to_owned(),
            "--mode".to_owned(),
            "by-name".to_owned(),
            "view".to_owned(),
            "mawjs-view".to_owned(),
            "view".to_owned(),
        ]),
        "resolve by-name view: exact view\n",
    );
    assert_ok_text(
        &run_cli(&[
            "resolve".to_owned(),
            "--mode".to_owned(),
            "by-name".to_owned(),
            "yeast".to_owned(),
            "110-yeast".to_owned(),
            "120-brew".to_owned(),
        ]),
        "resolve by-name yeast: fuzzy 110-yeast\n",
    );
    assert_ok_text(
        &run_cli(&[
            "resolve".to_owned(),
            "--mode".to_owned(),
            "worktree".to_owned(),
            "pay".to_owned(),
            "2-pay-v1".to_owned(),
            "2-pay-v2".to_owned(),
        ]),
        "resolve worktree pay: ambiguous 2-pay-v1, 2-pay-v2\n",
    );
    assert_ok_text(
        &run_cli(&[
            "resolve".to_owned(),
            "--mode".to_owned(),
            "by-name".to_owned(),
            "awjs".to_owned(),
            "mawjs-view".to_owned(),
        ]),
        "resolve by-name awjs: none hints=mawjs-view\n",
    );
    assert_usage_error(
        &run_cli(&[
            "resolve".to_owned(),
            "--mode".to_owned(),
            "unknown".to_owned(),
            "target".to_owned(),
            "item".to_owned(),
        ]),
        "resolve: unknown --mode",
    );

    assert_ok_text(
        &run_cli(&[
            "worktree-window".to_owned(),
            "--main-repo-name".to_owned(),
            "mawjs-oracle".to_owned(),
            "--wt-name".to_owned(),
            "1-feature".to_owned(),
            "--session".to_owned(),
            "mawjs-oracle".to_owned(),
            "--window".to_owned(),
            "1:feature:true".to_owned(),
        ]),
        "worktree-window mawjs-oracle 1-feature: bound feature\n",
    );
    assert_ok_text(
        &run_cli(&[
            "worktree-window".to_owned(),
            "--main-repo-name".to_owned(),
            "repo".to_owned(),
            "--wt-name".to_owned(),
            "missing".to_owned(),
            "--session".to_owned(),
            "repo".to_owned(),
            "--window".to_owned(),
            "1:feature:true".to_owned(),
        ]),
        "worktree-window repo missing: none\n",
    );
    assert_usage_error(
        &run_cli(&[
            "worktree-window".to_owned(),
            "--main-repo-name".to_owned(),
            "repo".to_owned(),
            "--wt-name".to_owned(),
            "feature".to_owned(),
            "--session".to_owned(),
            "repo".to_owned(),
            "--window".to_owned(),
            "1:feature:yes".to_owned(),
        ]),
        "worktree-window: window active must be true or false",
    );

    assert_ok_text(
        &run_cli(&[
            "calver".to_owned(),
            "--now".to_owned(),
            "2026-5-21T9:07".to_owned(),
            "--alpha".to_owned(),
            "--package-version".to_owned(),
            "26.5.20".to_owned(),
        ]),
        "26.5.21-alpha.907\n",
    );
    assert_usage_error(
        &run_cli(&[
            "calver".to_owned(),
            "--now".to_owned(),
            "2026-5-21".to_owned(),
        ]),
        "calver: --now must use YYYY-M-DTHH:MM",
    );
    assert_usage_error(
        &run_cli(&[
            "calver".to_owned(),
            "--now".to_owned(),
            "2026-13-21T9:07".to_owned(),
        ]),
        "calver: --now contains out-of-range date/time parts",
    );
}

