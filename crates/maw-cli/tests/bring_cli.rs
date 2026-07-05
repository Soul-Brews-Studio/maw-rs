// Ported from maw-js `parseBringArgs` coverage in dispatch-match and top-aliases
// tests, rendered as maw-rs side-by-side plan-only CLI output.

use maw_cli::{run_cli, CliOutput};

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn bring_plan_json_defaults_to_wake_split() {
    assert_eq!(
        run_cli(&args(&["bring", "neo", "--plan-json"])),
        CliOutput {
            code: 0,
            stdout: "{\"command\":\"bring\",\"opts\":{\"oracle\":\"neo\",\"split\":true}}\n"
                .to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn b_shorthand_plan_json_preserves_engine_and_tab_contract() {
    assert_eq!(
        run_cli(&args(&[
            "b",
            "neo",
            "--tab",
            "--split",
            "-e",
            "codex",
            "--plan-json",
        ])),
        CliOutput {
            code: 0,
            stdout: "{\"command\":\"bring\",\"opts\":{\"oracle\":\"neo\",\"split\":true,\"engine\":\"codex\"}}\n".to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn bring_plan_json_renders_to_pick_and_split_target() {
    assert_eq!(
        run_cli(&args(&[
            "bring",
            "mawjs-features",
            "--pick",
            "--engine",
            "claude",
            "--to",
            "50-mawjs:maw-js-1816",
            "--plan-json",
        ])),
        CliOutput {
            code: 0,
            stdout: "{\"command\":\"bring\",\"opts\":{\"oracle\":\"mawjs-features\",\"split\":true,\"engine\":\"claude\",\"pick\":true,\"session\":\"50-mawjs\",\"splitTarget\":\"50-mawjs:maw-js-1816\"}}\n".to_owned(),
            stderr: String::new(),
        }
    );
}

#[test]
fn bring_plan_rejects_missing_oracle_with_maw_js_usage() {
    let output = run_cli(&args(&["bring", "--tab", "--plan-json"]));
    assert_eq!(output.code, 2);
    assert!(output.stdout.is_empty());
    assert!(output.stderr.contains("bring: missing oracle name"));
    assert!(output.stderr.contains("usage: maw bring"));
}
