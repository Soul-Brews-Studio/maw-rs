use maw_cli::{run_cli, CliOutput};
use serde_json::Value;

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(ToString::to_string).collect::<Vec<_>>())
}

fn json(output: &CliOutput) -> Value {
    assert_eq!(output.code, 0, "{}", output.stderr);
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid json: {error}\n{}", output.stdout))
}

fn assert_usage_error(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "expected usage error for {args:?}");
    assert_eq!(output.stdout, "");
    assert!(
        output.stderr.contains(expected),
        "stderr did not contain {expected:?}: {}",
        output.stderr
    );
}

#[test]
fn transport_parser_errors_cover_missing_and_malformed_options() {
    assert_usage_error(&["transport"], "expected --classify-error or --send");
    assert_usage_error(
        &["transport", "--classify-error"],
        "missing --classify-error value",
    );
    assert_usage_error(
        &["transport", "--send", "--transport"],
        "missing --transport value",
    );
    assert_usage_error(
        &["transport", "--send", "--transport", ""],
        "requires a name",
    );
    assert_usage_error(
        &["transport", "--send", "--transport", "tmux:true:maybe:ok"],
        "invalid canReach boolean",
    );
    assert_usage_error(
        &["transport", "--send", "--transport", "tmux:true:true:nope"],
        "action must be ok, false, or throw=<error>",
    );
    assert_usage_error(&["transport", "--bad"], "unknown argument --bad");
    assert_usage_error(
        &["transport", "constants", "--bad"],
        "transport constants: unknown argument --bad",
    );
}

#[test]
fn transport_text_rendering_covers_classify_constants_and_send_failover() {
    let classified = run(&["transport", "--classify-empty"]);
    assert_eq!(classified.code, 0, "{}", classified.stderr);
    assert_eq!(
        classified.stdout,
        "transport classify reason=unknown retryable=false\n"
    );

    let constants = run(&["transport", "constants"]);
    assert_eq!(constants.code, 0, "{}", constants.stderr);
    assert!(constants.stdout.contains("reasons=timeout"));

    let send = run(&[
        "transport",
        "--send",
        "--transport",
        "offline:false:true:ok",
        "--transport",
        "blocked:true:false:ok",
        "--transport",
        "first:true:true:false",
        "--transport",
        "second:true:true:ok",
    ]);
    assert_eq!(send.code, 0, "{}", send.stderr);
    assert!(
        send.stdout.contains("transport send ok=true via=second"),
        "{}",
        send.stdout
    );
    assert!(send.stdout.contains("sent=first,second"), "{}", send.stdout);
}

#[test]
fn peer_probe_parser_errors_cover_missing_invalid_and_constants_options() {
    assert_usage_error(&["peer-probe"], "peer-probe: missing action");
    assert_usage_error(&["peer-probe", "unknown"], "peer-probe: invalid action");
    assert_usage_error(
        &["peer-probe", "classify"],
        "peer-probe classify: missing input",
    );
    assert_usage_error(
        &["peer-probe", "classify", "--http-status", "nope"],
        "--http-status must be an integer",
    );
    assert_usage_error(
        &["peer-probe", "classify", "--code"],
        "missing --code value",
    );
    assert_usage_error(
        &["peer-probe", "format", "--code", "NOPE"],
        "invalid --code value",
    );
    assert_usage_error(
        &["peer-probe", "format", "--code", "DNS"],
        "missing required value",
    );
    assert_usage_error(
        &["peer-probe", "handshake"],
        "peer-probe handshake: missing shape",
    );
    assert_usage_error(
        &["peer-probe", "handshake-constants", "--bad"],
        "handshake-constants: unknown argument --bad",
    );
}

#[test]
fn peer_probe_text_rendering_covers_classify_format_handshake_and_constants() {
    let classify = run(&["peer-probe", "classify", "--non-object"]);
    assert_eq!(classify.code, 0, "{}", classify.stderr);
    assert_eq!(classify.stdout, "UNKNOWN\n");

    let formatted = run(&[
        "peer-probe",
        "format",
        "--code",
        "REFUSED",
        "--message",
        "connection refused",
        "--url",
        "http://127.0.0.1:3456",
        "--alias",
        "local",
    ]);
    assert_eq!(formatted.code, 0, "{}", formatted.stderr);
    assert!(formatted.stdout.contains("local"), "{}", formatted.stdout);
    assert!(
        formatted.stdout.contains("127.0.0.1:3456"),
        "{}",
        formatted.stdout
    );

    let handshake = run(&["peer-probe", "handshake", "--missing"]);
    assert_eq!(handshake.code, 0, "{}", handshake.stderr);
    assert_eq!(handshake.stdout, "valid=false\n");

    let constants = run(&["peer-probe", "handshake-constants"]);
    assert_eq!(constants.code, 0, "{}", constants.stderr);
    assert!(constants.stdout.contains("validShapes=legacy-true"));
}

#[test]
fn peer_sources_parser_errors_and_text_warning_rendering_are_covered() {
    assert_usage_error(&["peer-sources", "--mode"], "missing --mode value");
    assert_usage_error(&["peer-sources", "--mode", "bogus"], "unknown --mode");
    assert_usage_error(&["peer-sources", "--peer"], "missing --peer value");
    assert_usage_error(
        &["peer-sources", "--named-peer"],
        "missing --named-peer value",
    );
    assert_usage_error(
        &["peer-sources", "--discovery-error"],
        "missing --discovery-error value",
    );
    assert_usage_error(
        &["peer-sources", "--discovery-hint"],
        "missing --discovery-hint value",
    );
    assert_usage_error(
        &["peer-sources", "--discovered", "bad"],
        "--discovered must use",
    );
    assert_usage_error(
        &["peer-sources", "constants", "--bad"],
        "constants: unknown argument --bad",
    );

    let text = run(&[
        "peer-sources",
        "--mode",
        "both",
        "--peer",
        "http://config:3456",
        "--discovery-error",
        "scout offline",
        "--discovery-hint",
        "retry later",
    ]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout.contains("peer-sources mode=both"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains("config - http://config:3456"),
        "{}",
        text.stdout
    );
    assert!(text.stdout.contains("warning:"), "{}", text.stdout);

    let constants = run(&["peer-sources", "constants"]);
    assert_eq!(constants.code, 0, "{}", constants.stderr);
    assert!(constants.stdout.contains("modes=config,scout,both"));
}

#[test]
fn federation_identity_and_sync_cover_text_and_parser_errors() {
    assert_usage_error(&["federation-identity", "--node"], "missing --node value");
    assert_usage_error(&["federation-identity", "--url"], "missing --url value");
    assert_usage_error(&["federation-identity", "--bad"], "unknown argument --bad");
    assert_usage_error(
        &["federation-identity", "constants", "--bad"],
        "constants: unknown argument --bad",
    );
    let identity = run(&[
        "federation-identity",
        "--node",
        "white",
        "--url",
        "http://white:3456",
        "--agent",
        "pulse=white",
    ]);
    assert_eq!(identity.code, 0, "{}", identity.stderr);
    assert_eq!(
        identity.stdout,
        "federation-identity node=white url=http://white:3456 agents=1\n"
    );

    assert_usage_error(&["federation-sync", "--node"], "missing --node value");
    assert_usage_error(&["federation-sync", "--agent"], "missing --agent value");
    assert_usage_error(
        &["federation-sync", "--identity", "p|u|n|a|maybe"],
        "reachability must be reachable or unreachable",
    );
    assert_usage_error(&["federation-sync", "--bad"], "unknown argument --bad");
    assert_usage_error(
        &["federation-sync", "constants", "--bad"],
        "constants: unknown argument --bad",
    );
    let sync = run(&[
        "federation-sync",
        "--dry-run",
        "--force",
        "--identity",
        "white|http://white:3456|white|pulse|reachable",
    ]);
    assert_eq!(sync.code, 0, "{}", sync.stderr);
    assert!(sync.stdout.contains("add=1"), "{}", sync.stdout);
    assert!(sync.stdout.contains("dryRun=true"), "{}", sync.stdout);
}

#[test]
fn federation_health_covers_text_constants_and_parser_error_branches() {
    assert_usage_error(&["federation-health", "--node"], "missing --node value");
    assert_usage_error(
        &["federation-health", "--local-url"],
        "missing --local-url value",
    );
    assert_usage_error(&["federation-health", "--peer"], "missing --peer value");
    assert_usage_error(
        &["federation-health", "--peer", "http://x|-|maybe|-||ok"],
        "reachability must be reachable or unreachable",
    );
    assert_usage_error(
        &[
            "federation-health",
            "--peer",
            "http://x|-|reachable|nan||ok",
        ],
        "latency must be u64",
    );
    assert_usage_error(
        &["federation-health", "--peer", "http://x|-|reachable|-||bad"],
        "clock flag must be ok or clock",
    );
    assert_usage_error(&["federation-health", "--remote"], "missing --remote value");
    assert_usage_error(
        &["federation-health", "--remote", "http://x|http|nan"],
        "http status must be u16",
    );
    assert_usage_error(
        &["federation-health", "--remote", "http://x|peer|-|-|maybe"],
        "reachability must be reachable or unreachable",
    );
    assert_usage_error(&["federation-health", "--bad"], "unknown argument --bad");
    assert_usage_error(
        &["federation-health", "constants", "--bad"],
        "constants: unknown argument --bad",
    );

    let text = run(&[
        "federation-health",
        "--peer",
        "http://alpha:3456|alpha|reachable|12|pulse|ok",
        "--remote",
        "http://alpha:3456|peer|http://localhost:3456|local|reachable",
    ]);
    assert_eq!(text.code, 0, "{}", text.stderr);
    assert_eq!(
        text.stdout,
        "federation-health healthyPairs=1 totalPairs=1\n"
    );

    let constants = run(&["federation-health", "constants"]);
    assert_eq!(constants.code, 0, "{}", constants.stderr);
    assert!(constants.stdout.contains("pairHealth=healthy"));
}

#[test]
fn discover_covers_non_json_invalid_peer_source_and_parser_errors() {
    assert_usage_error(&["discover", "--peers", "bogus"], "invalid_peer_source");
    assert_usage_error(&["discover", "--peers"], "missing --peers value");
    assert_usage_error(
        &["discover", "--named-peer", "bad"],
        "--named-peer must use",
    );
    assert_usage_error(
        &["discover", "--discovered", "bad"],
        "--discovered must use",
    );
    assert_usage_error(&["discover", "--pane", "bad"], "--pane must use");
    assert_usage_error(
        &["discover", "--pane", "%1|cmd|target|title|pid|/tmp|1"],
        "pane pid must be an integer",
    );
    assert_usage_error(&["discover", "--plugin", "bad"], "--plugin must use");
    assert_usage_error(
        &[
            "discover",
            "--plugin",
            "p|1|rs|standard|heavy|false|/p|p|||",
        ],
        "plugin weight must be an integer",
    );
    assert_usage_error(&["discover", "--fleet", "bad"], "--fleet must use");
    assert_usage_error(&["discover", "--oracle", "bad"], "--oracle must use");
    assert_usage_error(
        &[
            "discover",
            "--oracle",
            "neo|fleet|node|s|w|r|/tmp|maybe|false",
        ],
        "oracle has_psi must be true or false",
    );
    assert_usage_error(&["discover", "--agent", "bad"], "--agent must use");
    assert_usage_error(&["discover", "--bad"], "unknown argument --bad");
    assert_usage_error(
        &["discover", "constants", "--bad"],
        "constants: unknown argument --bad",
    );
}

#[test]
fn discover_text_rendering_covers_inventory_tree_awake_and_constants() {
    let inventory = run(&[
        "discover",
        "--peer",
        "http://config:3456",
        "--fleet",
        "fleet.json|slot|neo|session|window|/repo",
        "--oracle",
        "neo|fleet+psi|node|session|window|repo|/repo|true|true",
    ]);
    assert_eq!(inventory.code, 0, "{}", inventory.stderr);
    assert!(
        inventory.stdout.contains("http://config:3456"),
        "{}",
        inventory.stdout
    );
    assert!(inventory.stdout.contains("neo"), "{}", inventory.stdout);

    let tree = run(&[
        "discover",
        "--tree",
        "--peer",
        "http://config:3456",
        "--pane",
        "%1|claude|session:window|config|-|/repo|-",
        "--plugin",
        "buddy|1.0.0|rs|standard|1|false|/plugins/buddy|buddy|||",
        "--ghq",
        "/opt/Code/github.com/Soul-Brews-Studio/maw-rs",
    ]);
    assert_eq!(tree.code, 0, "{}", tree.stderr);
    assert!(tree.stdout.contains("buddy"), "{}", tree.stdout);
    assert!(tree.stdout.contains("maw-rs"), "{}", tree.stdout);

    let awake = run(&[
        "discover",
        "--awake",
        "--named-peer",
        "config=http://config:3456",
        "--pane",
        "%1|claude|session:window|config|-|/repo|-",
    ]);
    assert_eq!(awake.code, 0, "{}", awake.stderr);
    assert!(awake.stdout.contains("session:window"), "{}", awake.stdout);

    let constants_text = run(&["discover", "constants"]);
    assert_eq!(constants_text.code, 0, "{}", constants_text.stderr);
    assert!(constants_text
        .stdout
        .contains("peerSources=config,scout,both"));

    let constants_json = json(&run(&["discover", "constants", "--plan-json"]));
    assert_eq!(constants_json["action"], "constants");
}
