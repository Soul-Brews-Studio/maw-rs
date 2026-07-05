use maw_cli::run_cli;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum Fixture {
    #[serde(rename = "classifyError")]
    ClassifyError {
        name: String,
        error: Option<String>,
        expected: ExpectedClassifiedError,
    },
    Send {
        name: String,
        transports: Vec<FixtureTransport>,
        expected: ExpectedSend,
    },
}

#[derive(Debug, Deserialize)]
struct ExpectedClassifiedError {
    reason: String,
    retryable: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FixtureTransport {
    name: String,
    connected: Option<bool>,
    can_reach: Option<bool>,
    send: Option<SendAction>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
enum SendAction {
    Ok,
    False,
    Throw { error: String },
}

#[derive(Debug, Deserialize)]
struct ExpectedSend {
    result: ExpectedTransportResult,
    sent: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ExpectedTransportResult {
    ok: bool,
    via: String,
    reason: Option<String>,
    retryable: bool,
}

#[test]
fn transport_plan_cli_matches_maw_js_fixtures() {
    let fixtures: Vec<Fixture> = serde_json::from_str(include_str!(
        "../../maw-transport/tests/fixtures/transport-router.fixtures.json"
    ))
    .expect("valid transport fixtures");

    for fixture in fixtures {
        match fixture {
            Fixture::ClassifyError {
                name,
                error,
                expected,
            } => {
                let mut argv = vec!["transport".to_owned(), "--plan-json".to_owned()];
                if let Some(error) = error {
                    argv.push("--classify-error".to_owned());
                    argv.push(error);
                } else {
                    argv.push("--classify-empty".to_owned());
                }
                let output = run_cli(&argv);
                assert_eq!(output.code, 0, "{name}: {}", output.stderr);
                let json: serde_json::Value =
                    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                        panic!("{name} invalid json: {error}\n{}", output.stdout)
                    });
                assert_eq!(json["command"], "transport", "{name}");
                assert_eq!(json["kind"], "classifyError", "{name}");
                assert_eq!(json["reason"], expected.reason, "{name}");
                assert_eq!(json["retryable"], expected.retryable, "{name}");
            }
            Fixture::Send {
                name,
                transports,
                expected,
            } => {
                let mut argv = vec![
                    "transport".to_owned(),
                    "--plan-json".to_owned(),
                    "--send".to_owned(),
                ];
                for transport in transports {
                    argv.push("--transport".to_owned());
                    argv.push(format_transport_spec(transport));
                }
                let output = run_cli(&argv);
                assert_eq!(output.code, 0, "{name}: {}", output.stderr);
                let json: serde_json::Value =
                    serde_json::from_str(&output.stdout).unwrap_or_else(|error| {
                        panic!("{name} invalid json: {error}\n{}", output.stdout)
                    });
                assert_eq!(json["command"], "transport", "{name}");
                assert_eq!(json["kind"], "send", "{name}");
                assert_eq!(json["ok"], expected.result.ok, "{name}");
                assert_eq!(json["via"], expected.result.via, "{name}");
                assert_eq!(json["retryable"], expected.result.retryable, "{name}");
                match expected.result.reason {
                    Some(reason) => assert_eq!(json["reason"], reason, "{name}"),
                    None => assert!(json.get("reason").is_none(), "{name}"),
                }
                let sent: Vec<String> = json["sent"]
                    .as_array()
                    .expect("sent array")
                    .iter()
                    .map(|value| value.as_str().expect("sent string").to_owned())
                    .collect();
                assert_eq!(sent, expected.sent, "{name}");
            }
        }
    }
}

fn format_transport_spec(transport: FixtureTransport) -> String {
    let connected = transport.connected.unwrap_or(true);
    let can_reach = transport.can_reach.unwrap_or(true);
    let action = match transport.send.unwrap_or(SendAction::Ok) {
        SendAction::Ok => "ok".to_owned(),
        SendAction::False => "false".to_owned(),
        SendAction::Throw { error } => format!("throw={error}"),
    };
    format!("{}:{connected}:{can_reach}:{action}", transport.name)
}

#[test]
fn transport_plan_rejects_bad_transport_boolean() {
    let argv = vec![
        "transport".to_owned(),
        "--send".to_owned(),
        "--transport".to_owned(),
        "tmux:maybe:true:ok".to_owned(),
    ];

    let output = run_cli(&argv);
    assert_eq!(output.code, 2);
    assert!(
        output.stderr.contains("invalid connected boolean"),
        "{}",
        output.stderr
    );
}
