use maw_cli::run_cli;
use serde_json::Value;

const EXPECTED_PROOF: &str = "0be65d88e459264a48dffea592e7e31d155f81ba245718f4cd1605a382757f80";

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

fn base_args() -> Vec<String> {
    vec![
        "auto-pair-proof".to_owned(),
        "--plan-json".to_owned(),
        "--node".to_owned(),
        "m5".to_owned(),
        "--oracle".to_owned(),
        "mawjs".to_owned(),
        "--url".to_owned(),
        "http://m5.local:3456".to_owned(),
        "--pubkey".to_owned(),
        "pub-abc".to_owned(),
        "--token".to_owned(),
        "token-a".to_owned(),
    ]
}

#[test]
fn auto_pair_proof_plan_signs_stable_canonical_identity() {
    let json = json(&base_args());

    assert_eq!(json["command"], "auto-pair-proof");
    assert_eq!(json["node"], "m5");
    assert_eq!(json["oracle"], "mawjs");
    assert_eq!(json["url"], "http://m5.local:3456");
    assert_eq!(json["pubkey"], "pub-abc");
    assert_eq!(json["proof"], EXPECTED_PROOF);
    assert_eq!(json["valid"], Value::Null);
    assert_eq!(json["token"], Value::Null);
}

#[test]
fn auto_pair_proof_plan_verifies_good_and_bad_proofs() {
    let mut good_args = base_args();
    good_args.extend(["--proof".to_owned(), EXPECTED_PROOF.to_owned()]);
    let good = json(&good_args);
    assert_eq!(good["valid"], true);

    let mut bad_args = base_args();
    bad_args.extend(["--proof".to_owned(), "z".repeat(64)]);
    let bad = json(&bad_args);
    assert_eq!(bad["valid"], false);
    assert_eq!(bad["proof"], EXPECTED_PROOF);
}

#[test]
fn auto_pair_proof_plan_rejects_missing_identity_fields() {
    let output = run_cli(&[
        "auto-pair-proof".to_owned(),
        "--node".to_owned(),
        "m5".to_owned(),
    ]);

    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("missing --oracle value"),
        "{}",
        output.stderr
    );
}
