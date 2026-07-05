// Ported from maw-js shouldAutoWake policy into the maw-rs plan CLI surface.

use maw_cli::run_cli;
use serde_json::json;

fn json_output(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

#[test]
fn auto_wake_plan_cli_matches_flag_based_maw_js_cases() {
    let view = json_output(&run_cli(&[
        "auto-wake".to_owned(),
        "neo".to_owned(),
        "--site".to_owned(),
        "view".to_owned(),
        "--fleet-known".to_owned(),
        "--not-live".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(view["wake"], true);
    assert_eq!(view["reason"], "view: fleet-known and not running");

    let hey = json_output(&run_cli(&[
        "auto-wake".to_owned(),
        "volt".to_owned(),
        "--site".to_owned(),
        "hey".to_owned(),
        "--fleet-known".to_owned(),
        "--not-live".to_owned(),
        "--canonical-target".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(hey["wake"], false);
    assert_eq!(hey["reason"], "hey: canonical target — skip wake");

    let force = json_output(&run_cli(&[
        "auto-wake".to_owned(),
        "volt".to_owned(),
        "--site=hey".to_owned(),
        "--fleet-known".to_owned(),
        "--not-live".to_owned(),
        "--canonical-target".to_owned(),
        "--wake".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(force["wake"], true);
    assert_eq!(force["reason"], "--wake explicit force");
}

#[test]
fn auto_wake_plan_cli_matches_manifest_maw_js_cases() {
    let manifest_wins = json_output(&run_cli(&[
        "auto-wake".to_owned(),
        "neo".to_owned(),
        "--site".to_owned(),
        "hey".to_owned(),
        "--unknown-fleet".to_owned(),
        "--not-live".to_owned(),
        "--manifest-source".to_owned(),
        "fleet".to_owned(),
        "--manifest-live=false".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(manifest_wins["wake"], true);
    assert_eq!(manifest_wins["manifest"]["sources"], json!(["fleet"]));
    assert_eq!(manifest_wins["reason"], "hey: fleet-known and not running");

    let no_wake = json_output(&run_cli(&[
        "auto-wake".to_owned(),
        "neo".to_owned(),
        "--site".to_owned(),
        "hey".to_owned(),
        "--manifest-source".to_owned(),
        "fleet".to_owned(),
        "--manifest-live=false".to_owned(),
        "--no-wake".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(no_wake["wake"], false);
    assert_eq!(no_wake["reason"], "--no-wake explicit deny");
}

#[test]
fn auto_wake_plan_rejects_bad_site_and_missing_target() {
    let missing = run_cli(&["auto-wake".to_owned(), "--plan-json".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing target"),
        "{}",
        missing.stderr
    );

    let bad_site = run_cli(&[
        "auto-wake".to_owned(),
        "neo".to_owned(),
        "--site".to_owned(),
        "bogus".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(bad_site.code, 2);
    assert!(
        bad_site.stderr.contains("invalid --site"),
        "{}",
        bad_site.stderr
    );
}
