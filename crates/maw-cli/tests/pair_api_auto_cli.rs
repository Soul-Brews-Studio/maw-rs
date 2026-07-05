use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

fn base() -> Vec<String> {
    vec![
        "pair-api-auto".to_owned(),
        "--node".to_owned(),
        "node-a".to_owned(),
        "--oracle".to_owned(),
        "oracle-a".to_owned(),
        "--port".to_owned(),
        "4567".to_owned(),
        "--base-url".to_owned(),
        "http://localhost:4567".to_owned(),
        "--federation-token".to_owned(),
        "token-a".to_owned(),
        "--pubkey".to_owned(),
        "pppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppp".to_owned(),
        "--now".to_owned(),
        "70001".to_owned(),
        "--plan-json".to_owned(),
    ]
}

#[test]
fn pair_api_auto_plan_returns_signed_identity_and_add_plan_without_token() {
    let mut args = base();
    args.extend([
        "--remote-node".to_owned(),
        "remote".to_owned(),
        "--remote-oracle".to_owned(),
        "remote-oracle".to_owned(),
        "--remote-url".to_owned(),
        "http://remote".to_owned(),
        "--zid".to_owned(),
        "success".to_owned(),
        "--remote-pubkey".to_owned(),
        "rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr".to_owned(),
        "--hello".to_owned(),
        "success:70001".to_owned(),
        "--add-ok".to_owned(),
    ]);

    let json = json(&args);

    assert_eq!(json["command"], "pair-api-auto");
    assert_eq!(json["status"], 200);
    assert_eq!(json["ok"], true);
    assert_eq!(json["error"], Value::Null);
    assert_eq!(json["node"], "node-a");
    assert_eq!(json["oracle"], "oracle-a");
    assert_eq!(json["url"], "http://localhost:4567");
    assert_eq!(json["federationToken"], Value::Null);
    assert_eq!(
        json["proof"],
        "95e63fc871ab14ce17c14e0046cd41b9dd305c086f1ed325fd2c5e62e6ee849f"
    );
    assert_eq!(json["oneWay"], false);
    assert_eq!(json["add"]["alias"], "remote");
    assert_eq!(json["add"]["url"], "http://remote");
    assert_eq!(json["add"]["identityOracle"], "remote-oracle");
    assert_eq!(json["markSymmetricCheck"], true);
}

#[test]
fn pair_api_auto_plan_reports_missing_stale_and_add_refusals() {
    let mut stale_args = base();
    stale_args.extend([
        "--remote-node".to_owned(),
        "remote".to_owned(),
        "--remote-url".to_owned(),
        "http://remote".to_owned(),
        "--zid".to_owned(),
        "old".to_owned(),
        "--hello".to_owned(),
        "old:0".to_owned(),
        "--add-ok".to_owned(),
    ]);
    let stale = json(&stale_args);
    assert_eq!(stale["status"], 403);
    assert_eq!(stale["ok"], false);
    assert_eq!(stale["error"], "no_recent_hello");

    let mut mismatch_args = base();
    mismatch_args.extend([
        "--remote-node".to_owned(),
        "remote".to_owned(),
        "--remote-url".to_owned(),
        "http://remote".to_owned(),
        "--zid".to_owned(),
        "mismatch".to_owned(),
        "--hello".to_owned(),
        "mismatch:70001".to_owned(),
        "--add-pubkey-mismatch".to_owned(),
        "key mismatch".to_owned(),
    ]);
    let mismatch = json(&mismatch_args);
    assert_eq!(mismatch["status"], 409);
    assert_eq!(mismatch["error"], "key mismatch");

    let mut missing_args = base();
    missing_args.extend(["--remote-url".to_owned(), "http://remote".to_owned()]);
    let missing = json(&missing_args);
    assert_eq!(missing["status"], 400);
    assert_eq!(missing["error"], "missing_fields");
}

#[test]
fn pair_api_auto_plan_rejects_missing_config_and_bad_hello() {
    let missing = run_cli(&[
        "pair-api-auto".to_owned(),
        "--remote-node".to_owned(),
        "r".to_owned(),
    ]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --now value"),
        "{}",
        missing.stderr
    );

    let bad_hello = run_cli(&[
        "pair-api-auto".to_owned(),
        "--now".to_owned(),
        "70001".to_owned(),
        "--hello".to_owned(),
        "bad".to_owned(),
    ]);
    assert_eq!(bad_hello.code, 2);
    assert!(
        bad_hello.stderr.contains("--hello must be zid:seen_at_ms"),
        "{}",
        bad_hello.stderr
    );
}
