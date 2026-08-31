#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
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
        "3 plugins (3 active, 0 disabled)\n  core: 1 · standard: 1 · extra: 1\n  cli: 3 · api: 2 · health: ok\n  bravo · alpha · charlie\n"
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
    // Was `--json` until that became a real flag. The assertion is about rejecting
    // unknown arguments, so it needs an example that stays unknown.
    let output = run(&["plugin".to_owned(), "ls".to_owned(), "--not-a-flag".to_owned()]);

    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("plugin ls: unknown argument --not-a-flag"));
    assert!(output.stderr.contains("usage: maw-rs plugin ls"));
}

/// Strip SGR escapes so a rendered `plugin ls -v` row can be tokenised.
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            out.push(ch);
            continue;
        }
        for escaped in chars.by_ref() {
            if escaped == 'm' {
                break;
            }
        }
    }
    out
}

/// `plugin ls -v` row shape: `<name> <version> <icon> <tier> <surfaces> <dir>`.
fn ls_verbose_tier(stdout: &str, name: &str) -> String {
    let plain = strip_ansi(stdout);
    let row = plain
        .lines()
        .find(|line| line.split_whitespace().next() == Some(name))
        .unwrap_or_else(|| panic!("no `plugin ls -v` row for {name} in:\n{plain}"));
    let fields = row.split_whitespace().collect::<Vec<_>>();
    assert_eq!(fields[2], "●", "unexpected row shape for {name}: {row}");
    fields[3].to_owned()
}

/// `plugin info` text shape: a `  tier: <tier>` line.
fn info_tier(stdout: &str) -> String {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("tier: "))
        .unwrap_or_else(|| panic!("no tier line in `plugin info`:\n{stdout}"))
        .to_owned()
}

/// #792: `plugin info` hardcoded `core` for a tier-less manifest while `plugin ls -v`
/// derived the tier from `weight`, so the same plugin read `core` in one command and
/// `extra` in the other. Both must now report the shared effective tier.
#[test]
fn plugin_info_and_ls_verbose_agree_on_tier_when_the_manifest_omits_it() {
    let root = temp_plugin_root("omitted-tier");
    // The issue's repro: `"weight": 50`, no `"tier"` key at all.
    write_plugin(
        &root,
        "atlas",
        r#"{
          "name": "atlas",
          "version": "1.0.0",
          "sdk": "*",
          "entry": "index.ts",
          "weight": 50,
          "cli": { "command": "atlas" }
        }"#,
    );
    // No `"tier"` and no `"weight"` — the default weight still decides.
    write_plugin(
        &root,
        "bare",
        r#"{
          "name": "bare",
          "version": "1.0.0",
          "sdk": "*",
          "entry": "index.ts",
          "cli": { "command": "bare" }
        }"#,
    );
    // No `"tier"`, low weight — lands in core through the same resolver.
    write_plugin(
        &root,
        "kernel",
        r#"{
          "name": "kernel",
          "version": "1.0.0",
          "sdk": "*",
          "entry": "index.ts",
          "weight": 3,
          "cli": { "command": "kernel" }
        }"#,
    );
    // Explicit tier still wins over a contradicting weight.
    write_plugin(
        &root,
        "pinned",
        r#"{
          "name": "pinned",
          "version": "1.0.0",
          "sdk": "*",
          "entry": "index.ts",
          "tier": "standard",
          "weight": 90,
          "cli": { "command": "pinned" }
        }"#,
    );

    let listed = run(&[
        "plugin".to_owned(),
        "ls".to_owned(),
        "-v".to_owned(),
        "--scan-dir".to_owned(),
        root.display().to_string(),
    ]);
    assert_eq!(listed.code, 0, "{}", listed.stderr);

    for (name, expected) in [
        ("atlas", "extra"),
        ("bare", "extra"),
        ("kernel", "core"),
        ("pinned", "standard"),
    ] {
        let from_ls = ls_verbose_tier(&listed.stdout, name);

        let text = run(&[
            "plugin".to_owned(),
            "info".to_owned(),
            name.to_owned(),
            "--scan-dir".to_owned(),
            root.display().to_string(),
        ]);
        assert_eq!(text.code, 0, "{}", text.stderr);
        let from_info = info_tier(&text.stdout);

        let json = run(&[
            "plugin".to_owned(),
            "info".to_owned(),
            name.to_owned(),
            "--json".to_owned(),
            "--scan-dir".to_owned(),
            root.display().to_string(),
        ]);
        assert_eq!(json.code, 0, "{}", json.stderr);

        assert_eq!(
            from_info, from_ls,
            "plugin info and plugin ls -v disagree on {name}'s tier"
        );
        assert_eq!(from_info, expected, "unexpected effective tier for {name}");
        assert!(
            json.stdout.contains(&format!("\"tier\":\"{expected}\"")),
            "plugin info --json disagrees for {name}: {}",
            json.stdout
        );
    }
}

#[test]
fn plugin_ls_json_is_parseable_on_every_path_including_the_empty_one() {
    let root = temp_plugin_root("json");
    write_plugin(
        &root,
        "delta",
        r#"{
          "name": "delta",
          "version": "1.2.3",
          "sdk": "*",
          "tier": "core",
          "entry": "index.ts",
          "cli": { "command": "delta" }
        }"#,
    );

    let scan = |extra: &[&str]| {
        let mut args = vec![
            "plugin".to_owned(),
            "ls".to_owned(),
            "--json".to_owned(),
            "--scan-dir".to_owned(),
            root.display().to_string(),
        ];
        args.extend(extra.iter().map(|arg| (*arg).to_owned()));
        run(&args)
    };

    let output = scan(&[]);
    assert_eq!(output.code, 0, "{}", output.stderr);
    let parsed: serde_json::Value =
        serde_json::from_str(&output.stdout).expect("--json must emit parseable JSON");
    assert_eq!(parsed["total"], 1);
    assert_eq!(parsed["active"], 1);
    assert_eq!(parsed["plugins"][0]["name"], "delta");
    assert_eq!(parsed["plugins"][0]["version"], "1.2.3");
    assert_eq!(parsed["plugins"][0]["tier"], "core");
    assert_eq!(parsed["plugins"][0]["cli"], "delta");
    assert_eq!(parsed["plugins"][0]["api"], serde_json::Value::Null);
    assert_eq!(parsed["filters"], serde_json::json!([]));
    // The absolute path, not the ~-shortened form the human tables print.
    assert_eq!(parsed["plugins"][0]["dir"], root.join("delta").display().to_string());

    // A filter that matches nothing is the path where the text renderer emits an
    // English sentence. JSON must stay JSON there, or every consumer breaks on a
    // condition that is not an error.
    let empty = scan(&["--extra"]);
    assert_eq!(empty.code, 0, "{}", empty.stderr);
    let parsed_empty: serde_json::Value =
        serde_json::from_str(&empty.stdout).expect("empty --json must still parse");
    assert_eq!(parsed_empty["total"], 0);
    assert_eq!(parsed_empty["plugins"], serde_json::json!([]));
    assert_eq!(parsed_empty["filters"], serde_json::json!(["extra"]));
}
