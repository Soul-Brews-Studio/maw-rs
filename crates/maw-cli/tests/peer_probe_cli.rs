// Plan CLI parity for maw-js peers/pair probe pure helpers.

use maw_cli::run_cli;
use serde_json::json;

fn json_output(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid json: {error}\n{}", output.stdout))
}

#[test]
fn peer_probe_classify_plan_matches_maw_js_buckets() {
    let output = json_output(&run_cli(&[
        "peer-probe".to_owned(),
        "classify".to_owned(),
        "--cause-code".to_owned(),
        "EAI_AGAIN".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(output["code"], "DNS");
    assert_eq!(output["exitCode"], 3);

    let http = json_output(&run_cli(&[
        "peer-probe".to_owned(),
        "classify".to_owned(),
        "--http-status".to_owned(),
        "503".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(http["code"], "HTTP_5XX");
    assert_eq!(http["exitCode"], 6);
}

#[test]
fn peer_probe_format_plan_matches_maw_js_host_and_hint_contract() {
    let output = json_output(&run_cli(&[
        "peer-probe".to_owned(),
        "format".to_owned(),
        "--code".to_owned(),
        "DNS".to_owned(),
        "--message".to_owned(),
        "query ENOTIMP white.local".to_owned(),
        "--at".to_owned(),
        "now".to_owned(),
        "--url".to_owned(),
        "http://white.local:3456/base".to_owned(),
        "--alias".to_owned(),
        "white".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(
        output["hint"],
        "install avahi-daemon (Linux) for mDNS, or add white.local to /etc/hosts"
    );
    assert_eq!(output["host"], "white.local:3456");
    assert!(output["formatted"]
        .as_str()
        .unwrap()
        .contains("retry: maw peers probe white"));
}

#[test]
fn peer_probe_handshake_plan_validates_maw_js_shapes() {
    let schema = json_output(&run_cli(&[
        "peer-probe".to_owned(),
        "handshake".to_owned(),
        "--schema".to_owned(),
        "1".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(
        schema,
        json!({"command":"peer-probe","action":"handshake","ok":true,"valid":true})
    );

    let empty = json_output(&run_cli(&[
        "peer-probe".to_owned(),
        "handshake".to_owned(),
        "--empty-object".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(empty["valid"], false);
}
