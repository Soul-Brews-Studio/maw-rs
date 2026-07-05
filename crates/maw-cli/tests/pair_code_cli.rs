use maw_cli::run_cli;
use serde_json::Value;

#[test]
fn pair_code_plan_formats_validates_and_redacts_like_maw_js() {
    let output = run_cli(&[
        "pair-code".to_owned(),
        "--plan-json".to_owned(),
        "--code".to_owned(),
        " ab c-2 34\n".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");
    assert_eq!(json["command"], "pair-code");
    assert_eq!(json["normalized"], "ABC234");
    assert_eq!(json["pretty"], "ABC-234");
    assert_eq!(json["redacted"], "ABC-***");
    assert_eq!(json["valid"], true);
}

#[test]
fn pair_code_plan_reports_invalid_shape_without_throwing() {
    let output = run_cli(&[
        "pair-code".to_owned(),
        "--plan-json".to_owned(),
        "--code".to_owned(),
        "ABCDE0".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");
    assert_eq!(json["normalized"], "ABCDE0");
    assert_eq!(json["valid"], false);
    assert_eq!(json["redacted"], "ABC-***");
}

#[test]
fn pair_code_generate_plan_uses_deterministic_bytes() {
    let output = run_cli(&[
        "pair-code".to_owned(),
        "--plan-json".to_owned(),
        "--bytes".to_owned(),
        "0,1,31,32,33,255".to_owned(),
    ]);

    assert_eq!(output.code, 0, "{}", output.stderr);
    let json: Value = serde_json::from_str(&output.stdout).expect("json output");
    assert_eq!(json["normalized"], "AB9AB9");
    assert_eq!(json["pretty"], "AB9-AB9");
    assert_eq!(json["valid"], true);
}

#[test]
fn pair_code_plan_requires_code_or_bytes() {
    let output = run_cli(&["pair-code".to_owned()]);

    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("expected --code or --bytes"),
        "{}",
        output.stderr
    );
}
