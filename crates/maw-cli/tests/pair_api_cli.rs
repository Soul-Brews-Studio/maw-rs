use maw_cli::run_cli;
use serde_json::Value;

fn json(argv: &[String]) -> Value {
    let output = run_cli(argv);
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).expect("json output")
}

fn base(subcommand: &str) -> Vec<String> {
    vec![
        "pair-api".to_owned(),
        subcommand.to_owned(),
        "--node".to_owned(),
        "node-a".to_owned(),
        "--oracle".to_owned(),
        "oracle-a".to_owned(),
        "--port".to_owned(),
        "4567".to_owned(),
        "--base-url".to_owned(),
        "http://localhost:4567".to_owned(),
        "--federation-token".to_owned(),
        "abababababababababababababababababababababababababababababababab".to_owned(),
        "--pubkey".to_owned(),
        "pppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppp".to_owned(),
        "--now".to_owned(),
        "1000000".to_owned(),
        "--plan-json".to_owned(),
    ]
}

#[test]
fn pair_api_generate_plan_returns_ttl_and_redacts_token() {
    let mut args = base("generate");
    args.extend([
        "--code".to_owned(),
        "ABC234".to_owned(),
        "--expires-sec".to_owned(),
        "5".to_owned(),
    ]);

    let json = json(&args);

    assert_eq!(json["command"], "pair-api");
    assert_eq!(json["endpoint"], "generate");
    assert_eq!(json["status"], 201);
    assert_eq!(json["ok"], true);
    assert_eq!(json["code"], "ABC-234");
    assert_eq!(json["ttlMs"], 5000);
    assert_eq!(json["expiresAt"], 1_005_000);
    assert_eq!(json["node"], "node-a");
    assert_eq!(json["port"], 4567);
    assert_eq!(json["federationToken"], Value::Null);
}

#[test]
fn pair_api_probe_plan_reports_live_missing_and_expired_codes() {
    let mut live_args = base("probe");
    live_args.extend([
        "--code".to_owned(),
        "ABC234".to_owned(),
        "--seed-code".to_owned(),
        "ABC234:120000:1000000".to_owned(),
    ]);
    let live = json(&live_args);
    assert_eq!(live["status"], 200);
    assert_eq!(live["ok"], true);
    assert_eq!(live["node"], "node-a");
    assert_eq!(live["error"], Value::Null);

    let mut missing_args = base("probe");
    missing_args.extend(["--code".to_owned(), "ZZZ999".to_owned()]);
    let missing = json(&missing_args);
    assert_eq!(missing["status"], 404);
    assert_eq!(missing["ok"], false);
    assert_eq!(missing["error"], "not_found");

    let mut expired_args = base("probe");
    expired_args.extend([
        "--code".to_owned(),
        "DEF456".to_owned(),
        "--seed-code".to_owned(),
        "DEF456:1:0".to_owned(),
    ]);
    let expired = json(&expired_args);
    assert_eq!(expired["status"], 410);
    assert_eq!(expired["error"], "expired");
}

#[test]
fn pair_api_accept_and_status_plan_consume_code_and_return_remote() {
    let mut accept_args = base("accept");
    accept_args.extend([
        "--code".to_owned(),
        "ABC234".to_owned(),
        "--remote-node".to_owned(),
        "remote".to_owned(),
        "--remote-url".to_owned(),
        "http://remote".to_owned(),
        "--seed-code".to_owned(),
        "ABC234:1000000:1000000".to_owned(),
    ]);
    let accepted = json(&accept_args);
    assert_eq!(accepted["status"], 200);
    assert_eq!(accepted["ok"], true);
    assert_eq!(accepted["node"], "node-a");
    assert_eq!(accepted["url"], "http://localhost:4567");
    assert_eq!(accepted["federationToken"], Value::Null);

    let mut status_args = base("status");
    status_args.extend([
        "--code".to_owned(),
        "ABC234".to_owned(),
        "--seed-code".to_owned(),
        "ABC234:1000000:1000000".to_owned(),
        "--seed-accepted".to_owned(),
        "remote=http://remote".to_owned(),
    ]);
    let status = json(&status_args);
    assert_eq!(status["status"], 200);
    assert_eq!(status["ok"], true);
    assert_eq!(status["consumed"], true);
    assert_eq!(status["remoteNode"], "remote");
    assert_eq!(status["remoteUrl"], "http://remote");
}

#[test]
fn pair_api_plan_rejects_bad_endpoint_and_missing_code() {
    let bad_endpoint = run_cli(&["pair-api".to_owned(), "unknown".to_owned()]);
    assert_eq!(bad_endpoint.code, 2);
    assert!(
        bad_endpoint
            .stderr
            .contains("expected generate, probe, accept, or status"),
        "{}",
        bad_endpoint.stderr
    );

    let missing = run_cli(&["pair-api".to_owned(), "probe".to_owned()]);
    assert_eq!(missing.code, 2);
    assert!(
        missing.stderr.contains("missing --code value"),
        "{}",
        missing.stderr
    );
}
