use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

#[test]
fn consent_pin_plan_hashes_normalized_pin_without_echoing_plaintext() {
    let json = json(&[
        "consent-pin".to_owned(),
        "--plan-json".to_owned(),
        "--pin".to_owned(),
        " ab c-2 34\n".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-pin");
    assert_eq!(json["normalized"], "ABC234");
    assert_eq!(json["redacted"], "ABC-***");
    assert_eq!(json["valid"], true);
    assert_eq!(json["pin"], Value::Null);
    assert_eq!(
        json["hash"],
        "8c640c4e71f90160b2b3615af86739e6b15ddc877ae79e18aada753565f756c4"
    );
}

#[test]
fn consent_pin_plan_verifies_expected_hash_and_rejects_invalid_shape() {
    let good_hash = "8c640c4e71f90160b2b3615af86739e6b15ddc877ae79e18aada753565f756c4";
    let good = json(&[
        "consent-pin".to_owned(),
        "--plan-json".to_owned(),
        "--pin".to_owned(),
        "ABC-234".to_owned(),
        "--expected-hash".to_owned(),
        good_hash.to_owned(),
    ]);
    assert_eq!(good["verified"], true);

    let bad_shape = json(&[
        "consent-pin".to_owned(),
        "--plan-json".to_owned(),
        "--pin".to_owned(),
        "ABCDE0".to_owned(),
        "--expected-hash".to_owned(),
        good_hash.to_owned(),
    ]);
    assert_eq!(bad_shape["valid"], false);
    assert_eq!(bad_shape["verified"], false);
    assert_eq!(bad_shape["redacted"], "ABC-***");
}

#[test]
fn consent_pin_plan_generates_request_id_from_first_twelve_bytes() {
    let json = json(&[
        "consent-pin".to_owned(),
        "--plan-json".to_owned(),
        "--request-id-bytes".to_owned(),
        "0,1,2,3,4,5,6,7,8,9,10,255,99".to_owned(),
    ]);

    assert_eq!(json["command"], "consent-pin");
    assert_eq!(json["requestId"], "000102030405060708090aff");
    assert_eq!(json["hash"], Value::Null);
    assert_eq!(json["verified"], Value::Null);
}

#[test]
fn consent_pin_plan_requires_pin_or_request_id_bytes() {
    let output = run_cli(&["consent-pin".to_owned()]);

    assert_eq!(output.code, 2);
    assert!(
        output
            .stderr
            .contains("expected --pin or --request-id-bytes"),
        "{}",
        output.stderr
    );
}
