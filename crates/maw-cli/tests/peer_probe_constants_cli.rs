use maw_cli::run_cli;

fn run(args: &[&str]) -> maw_cli::CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

#[test]
fn peer_probe_constants_plan_reports_codes_and_exit_codes() {
    let output = run(&["peer-probe", "constants", "--plan-json"]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert_eq!(output.stderr, "");
    assert!(output.stdout.contains("\"command\":\"peer-probe\""));
    assert!(output.stdout.contains("\"action\":\"constants\""));
    assert!(output.stdout.contains("\"codes\":[\"DNS\",\"REFUSED\",\"TIMEOUT\",\"HTTP_4XX\",\"HTTP_5XX\",\"TLS\",\"BAD_BODY\",\"UNREACHABLE\",\"UNKNOWN\"]"));
    assert!(output.stdout.contains("\"exitCodes\":{\"DNS\":3,\"REFUSED\":4,\"TIMEOUT\":5,\"HTTP_4XX\":6,\"HTTP_5XX\":6,\"TLS\":2,\"BAD_BODY\":2,\"UNREACHABLE\":7,\"UNKNOWN\":2}"));
}

#[test]
fn peer_probe_constants_text_and_json_surfaces_agree_on_code_set() {
    // #733 review: the text and JSON constants surfaces drifted before (UNREACHABLE
    // reached JSON but not text). Pin them together so editing one without the
    // other turns red — parse the code tokens each emits and require equality.
    use std::collections::BTreeSet;

    let json = run(&["peer-probe", "constants", "--plan-json"]).stdout;
    let text = run(&["peer-probe", "constants"]).stdout;

    let json_codes: BTreeSet<String> = json
        .split("\"codes\":[")
        .nth(1)
        .and_then(|tail| tail.split(']').next())
        .map(|list| list.split(',').map(|token| token.trim_matches('"').to_owned()).collect())
        .unwrap_or_default();
    let text_codes: BTreeSet<String> = text
        .split("codes=")
        .nth(1)
        .and_then(|tail| tail.split_whitespace().next())
        .map(|list| list.split(',').map(str::to_owned).collect())
        .unwrap_or_default();

    assert!(!json_codes.is_empty() && !text_codes.is_empty(), "parsed both surfaces");
    assert_eq!(json_codes, text_codes, "text and JSON constants must list the same codes");
    assert!(json_codes.contains("UNREACHABLE"), "UNREACHABLE must be present on both surfaces");
}

#[test]
fn peer_probe_constants_rejects_unknown_arguments() {
    let output = run(&["peer-probe", "constants", "--bad"]);
    assert_eq!(output.code, 2);
    assert!(output
        .stderr
        .contains("peer-probe constants: unknown argument --bad"));
    assert!(output.stderr.contains("maw-rs peer-probe constants"));
}
