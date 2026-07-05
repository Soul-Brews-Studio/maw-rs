use maw_auth::{sign_headers_v3_at, sign_request_v3};
use maw_cli::run_cli;

const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const NOW: i64 = 1_700_000_000;

fn json(output: &maw_cli::CliOutput) -> serde_json::Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
        panic!("invalid json: {error}\n{}", output.stdout);
    })
}

fn signed_header_args(body: &str, timestamp: i64) -> Vec<String> {
    let headers = sign_headers_v3_at(
        PEER_KEY,
        FROM,
        "POST",
        "/api/send",
        Some(body.as_bytes()),
        timestamp,
    )
    .expect("fixture signs");
    vec![
        "--header".to_owned(),
        format!("X-Maw-From={}", headers.get("X-Maw-From").expect("from")),
        "--header".to_owned(),
        format!(
            "X-Maw-Signature-V3={}",
            headers.get("X-Maw-Signature-V3").expect("sig")
        ),
        "--header".to_owned(),
        format!(
            "X-Maw-Timestamp={}",
            headers.get("X-Maw-Timestamp").expect("timestamp")
        ),
    ]
}

#[test]
fn auth_sign_v3_plan_cli_matches_maw_js_payload_contract() {
    let output = json(&run_cli(&[
        "auth".to_owned(),
        "sign-v3".to_owned(),
        "--peer-key".to_owned(),
        PEER_KEY.to_owned(),
        "--from".to_owned(),
        FROM.to_owned(),
        "--method".to_owned(),
        "post".to_owned(),
        "--path".to_owned(),
        "/api/send".to_owned(),
        "--now".to_owned(),
        NOW.to_string(),
        "--body".to_owned(),
        "body".to_owned(),
        "--plan-json".to_owned(),
    ]));
    let expected = sign_request_v3(PEER_KEY, FROM, "post", "/api/send", NOW, Some(b"body"))
        .expect("fixture signs");
    assert_eq!(output["command"], "auth");
    assert_eq!(output["kind"], "sign-v3");
    assert_eq!(output["signature"], expected.signature);
    assert_eq!(output["bodyHash"], expected.body_hash);
    assert_eq!(output["headers"]["X-Maw-From"], FROM);
    assert_eq!(output["headers"]["X-Maw-Auth-Version"], "v3");
}

#[test]
fn auth_verify_plan_cli_matches_maw_js_o6_decisions() {
    let legacy = json(&run_cli(&[
        "auth".to_owned(),
        "verify-request".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
        "--path".to_owned(),
        "/api/send".to_owned(),
        "--now".to_owned(),
        NOW.to_string(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(legacy["decision"]["kind"], "accept-legacy");
    assert_eq!(legacy["decision"]["reason"], "no-cache-no-sig");

    let mut tofu = vec![
        "auth".to_owned(),
        "verify-request".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
        "--path".to_owned(),
        "/api/send".to_owned(),
        "--now".to_owned(),
        NOW.to_string(),
        "--body".to_owned(),
        "body".to_owned(),
        "--plan-json".to_owned(),
    ];
    tofu.extend(signed_header_args("body", NOW));
    let tofu = json(&run_cli(&tofu));
    assert_eq!(tofu["decision"]["kind"], "accept-tofu-record");
    assert_eq!(tofu["decision"]["from"], FROM);

    let mut verified = vec![
        "auth".to_owned(),
        "verify-request".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
        "--path".to_owned(),
        "/api/send".to_owned(),
        "--now".to_owned(),
        NOW.to_string(),
        "--body".to_owned(),
        "body".to_owned(),
        "--cached-pubkey".to_owned(),
        PEER_KEY.to_owned(),
        "--plan-json".to_owned(),
    ];
    verified.extend(signed_header_args("body", NOW));
    let verified = json(&run_cli(&verified));
    assert_eq!(verified["decision"]["kind"], "accept-verified");

    let mut mismatch = vec![
        "auth".to_owned(),
        "verify-request".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
        "--path".to_owned(),
        "/api/send".to_owned(),
        "--now".to_owned(),
        NOW.to_string(),
        "--body".to_owned(),
        "tampered".to_owned(),
        "--cached-pubkey".to_owned(),
        PEER_KEY.to_owned(),
        "--plan-json".to_owned(),
    ];
    mismatch.extend(signed_header_args("body", NOW));
    let mismatch = json(&run_cli(&mismatch));
    assert_eq!(mismatch["decision"]["kind"], "refuse-mismatch");

    let malformed = json(&run_cli(&[
        "auth".to_owned(),
        "verify-request".to_owned(),
        "--method".to_owned(),
        "POST".to_owned(),
        "--path".to_owned(),
        "/api/send".to_owned(),
        "--now".to_owned(),
        NOW.to_string(),
        "--cached-pubkey".to_owned(),
        PEER_KEY.to_owned(),
        "--header".to_owned(),
        format!("x-maw-from={FROM}"),
        "--header".to_owned(),
        format!("x-maw-signature-v3={}", "0".repeat(64)),
        "--header".to_owned(),
        "x-maw-timestamp=nope".to_owned(),
        "--plan-json".to_owned(),
    ]));
    assert_eq!(malformed["decision"]["kind"], "refuse-malformed");
    assert_eq!(malformed["decision"]["reason"], "invalid-timestamp");
}

#[test]
fn auth_plan_rejects_missing_required_inputs() {
    let output = run_cli(&[
        "auth".to_owned(),
        "sign-v3".to_owned(),
        "--plan-json".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("--peer-key is required"));

    let output = run_cli(&[
        "auth".to_owned(),
        "verify-request".to_owned(),
        "--header".to_owned(),
        "not-a-header".to_owned(),
    ]);
    assert_eq!(output.code, 2);
    assert!(output.stderr.contains("--header must be key=value"));
}
