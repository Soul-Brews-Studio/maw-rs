use maw_cli::run_cli;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn run(args: &[String]) -> maw_cli::CliOutput {
    run_cli(args)
}

fn temp_plugin_root(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time after unix epoch")
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("maw-rs-plugin-ls-{label}-{nonce}-{count}"));
    fs::create_dir_all(&root).expect("create temp plugin root");
    root
}

fn write_plugin(root: &Path, dir_name: &str, manifest: &str) {
    let dir = root.join(dir_name);
    fs::create_dir_all(&dir).expect("create plugin dir");
    fs::write(dir.join("index.ts"), "export function handle() {}\n").expect("write plugin entry");
    fs::write(dir.join("plugin.json"), manifest).expect("write plugin manifest");
}

#[test]
fn plugin_ls_defaults_to_compact_summary_with_tier_and_surface_counts() {
    let root = temp_plugin_root("tiers");
    write_plugin(
        &root,
        "alpha",
        r#"{
          "name": "alpha",
          "version": "1.2.3",
          "sdk": "*",
          "tier": "standard",
          "entry": "index.ts",
          "cli": { "command": "alpha" },
          "api": { "path": "/api/plugins/alpha", "methods": ["GET"] }
        }"#,
    );
    write_plugin(
        &root,
        "bravo",
        r#"{
          "name": "bravo",
          "version": "0.2.0",
          "sdk": "*",
          "tier": "core",
          "entry": "index.ts",
          "cli": { "command": "bravo" }
        }"#,
    );
    write_plugin(
        &root,
        "charlie",
        r#"{
          "name": "charlie",
          "version": "0.3.0",
          "sdk": "*",
          "tier": "extra",
          "entry": "index.ts",
          "api": { "path": "/api/plugins/charlie", "methods": ["POST"] }
        }"#,
    );

    let output = run(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "--scan-dir".to_owned(),
        root.display().to_string(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert_eq!(
        output.stdout,
        "3 plugins (3 active, 0 disabled)\n  core: 1 · standard: 1 · extra: 1\n  cli: 3 · api: 2 · health: ok\n"
    );
}

#[test]
fn plugin_ls_verbose_renders_maw_js_grouped_table_and_filters_refused_plugins() {
    let root = temp_plugin_root("verbose");
    write_plugin(
        &root,
        "delta",
        r#"{
          "name": "delta",
          "version": "2.0.0",
          "sdk": "*",
          "entry": "index.ts",
          "description": "Delta tools",
          "weight": 7,
          "cli": { "command": "delta-tools" },
          "api": { "path": "/api/plugins/delta", "methods": ["GET", "POST"] }
        }"#,
    );
    write_plugin(
        &root,
        "future",
        r#"{
          "name": "future",
          "version": "9.0.0",
          "sdk": ">99.0.0",
          "entry": "index.ts",
          "tier": "extra"
        }"#,
    );

    let output = run(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "-v".to_owned(),
        "--scan-dir".to_owned(),
        root.display().to_string(),
        "--runtime-version".to_owned(),
        "1.0.0".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    assert!(
        output.stdout.starts_with("\n\x1b[1mcore\x1b[0m (1)\n"),
        "{}",
        output.stdout
    );
    assert!(
        output.stdout.contains(&format!(
            "delta  2.0.0    \x1b[32m●\x1b[0m core  cli:delta-tools, api:/api/plugins/delta  {}/delta",
            root.display()
        )),
        "{}",
        output.stdout
    );
    assert!(!output.stdout.contains("future"), "{}", output.stdout);
    assert!(!output.stdout.contains("description:"), "{}", output.stdout);
    assert!(output.stdout.ends_with("\n1 active\n"), "{}", output.stdout);
}

#[test]
fn plugin_ls_rejects_unknown_args() {
    let output = run(&["plugin".to_owned(), "ls".to_owned(), "--json".to_owned()]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("plugin ls: unknown argument --json"));
    assert!(output.stderr.contains("usage: maw-rs plugin ls"));
}
