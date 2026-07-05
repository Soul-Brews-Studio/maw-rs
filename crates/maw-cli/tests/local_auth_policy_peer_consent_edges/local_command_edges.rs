use maw_cli::{run_cli, CliOutput};

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn ok_contains(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(expected),
        "stdout for {args:?} did not contain {expected:?}: {}",
        output.stdout
    );
    assert_eq!(output.stderr, "");
}

fn err_contains(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
}

#[test]
fn auto_wake_remaining_parser_and_site_text_edges() {
    for site in ["api-send", "api-wake", "peek", "bud", "wake-cmd"] {
        ok_contains(
            &["auto-wake", "neo", "--site", site, "--plan-json"],
            &format!("\"site\":\"{site}\""),
        );
    }
    err_contains(&["auto-wake", "neo", "--site=nope"], "invalid --site value");
    err_contains(
        &["auto-wake", "neo", "--manifest-live=maybe"],
        "--manifest-live must be true or false",
    );
    err_contains(
        &["auto-wake", "neo", "--manifest-live"],
        "missing --manifest-live value",
    );
    err_contains(&["auto-wake", "neo", "--bogus"], "unknown argument --bogus");
}

#[test]
fn auth_text_and_unknown_argument_edges() {
    ok_contains(
        &[
            "auth",
            "verify-request",
            "--now",
            "123",
            "--header",
            "x-maw-from=mawjs:m5",
        ],
        "accept-legacy",
    );
    ok_contains(
        &[
            "auth",
            "verify-legacy-from",
            "--from",
            "mawjs:m5",
            "--signed-at",
            "old",
            "--signature",
            "bad",
            "--now",
            "123",
        ],
        "accept-tofu-record",
    );
    ok_contains(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            "mawjs:m5",
            "--timestamp",
            "123",
            "--signature-v3",
            "bad",
            "--now",
            "123",
        ],
        "accept-tofu-record",
    );
    ok_contains(
        &[
            "auth",
            "sign-v3",
            "--peer-key",
            "not-a-key",
            "--from",
            "mawjs:m5",
            "--now",
            "123",
        ],
        "c6efc646",
    );

    for (args, expected) in [
        (
            &["auth", "sign-v1", "--weird"][..],
            "auth sign-v1: unknown argument --weird",
        ),
        (
            &["auth", "verify-v1", "--weird"][..],
            "auth verify-v1: unknown argument --weird",
        ),
        (
            &["auth", "verify-legacy-from", "--weird"][..],
            "auth verify-legacy-from: unknown argument --weird",
        ),
        (
            &["auth", "verify-v3-from", "--weird"][..],
            "auth verify-v3-from: unknown argument --weird",
        ),
        (
            &["auth", "from-sign-payload", "--weird"][..],
            "auth from-sign-payload: unknown argument --weird",
        ),
        (
            &["auth", "hmac-verify", "--weird"][..],
            "auth hmac-verify: unknown argument --weird",
        ),
        (
            &["auth", "hmac-sign", "--weird"][..],
            "auth hmac-sign: unknown argument --weird",
        ),
        (
            &["auth", "sign-v3", "--weird"][..],
            "auth sign-v3: unknown argument --weird",
        ),
        (
            &["auth", "loopback", "--weird"][..],
            "auth loopback: unknown argument --weird",
        ),
        (
            &["auth", "from-address", "--weird"][..],
            "auth from-address: unknown argument --weird",
        ),
        (
            &["auth", "verify-request", "--weird"][..],
            "auth verify-request: unknown argument --weird",
        ),
    ] {
        err_contains(args, expected);
    }
}

#[test]
fn policy_and_split_text_and_error_edges() {
    ok_contains(&["policy", "--constants"], "policy constants default-tier");
    ok_contains(&["policy", "--weight", "75"], "policy weight 75:");
    ok_contains(&["policy", "--default-active", "1500"], "plugins=");
    ok_contains(
        &["policy", "--default-active", "1500", "--includes", "tmux"],
        "includes tmux:",
    );
    ok_contains(&["policy", "constants"], "policy constants default-tier");
    err_contains(&["policy", "--weight"], "missing --weight value");
    err_contains(
        &["policy", "--default-active"],
        "missing --default-active value",
    );
    err_contains(&["policy", "--includes"], "missing --includes value");
    err_contains(&["policy", "--includes", "x"], "requires --default-active");
    err_contains(&["policy", "--bad"], "unknown argument --bad");

    ok_contains(
        &["split-policy", "--pane-current-command", "zsh"],
        "split-policy action=",
    );
    ok_contains(
        &["split-policy", "constants"],
        "split-policy constants actions=",
    );
    err_contains(
        &["split-policy", "--pane-current-command"],
        "missing --pane-current-command value",
    );
    err_contains(
        &["split-policy", "--requested-policy"],
        "missing --requested-policy value",
    );
    err_contains(&["split-policy", "--bad"], "unknown argument --bad");
}

#[test]
fn peer_probe_and_sources_remaining_edges() {
    err_contains(&["peer-probe"], "missing action");
    err_contains(&["peer-probe", "nope"], "invalid action");
    ok_contains(&["peer-probe", "constants"], "peer-probe codes=DNS");
    ok_contains(&["peer-probe", "classify", "--code", "ENOTFOUND"], "DNS");
    ok_contains(
        &["peer-probe", "classify", "--cause-code", "ECONNREFUSED"],
        "REFUSED",
    );
    ok_contains(
        &["peer-probe", "classify", "--name", "AbortError"],
        "TIMEOUT",
    );
    ok_contains(&["peer-probe", "classify", "--non-object"], "UNKNOWN");
    err_contains(
        &["peer-probe", "classify", "--http-status"],
        "missing --http-status value",
    );
    err_contains(
        &["peer-probe", "classify", "--code"],
        "missing --code value",
    );
    err_contains(
        &["peer-probe", "classify", "--cause-code"],
        "missing --cause-code value",
    );
    err_contains(
        &["peer-probe", "classify", "--name"],
        "missing --name value",
    );
    err_contains(
        &["peer-probe", "classify", "--bad"],
        "unknown argument --bad",
    );

    err_contains(&["peer-probe", "format", "--code"], "missing --code value");
    err_contains(
        &["peer-probe", "format", "--message"],
        "missing --message value",
    );
    err_contains(&["peer-probe", "format", "--at"], "missing --at value");
    err_contains(&["peer-probe", "format", "--url"], "missing --url value");
    err_contains(
        &["peer-probe", "format", "--alias"],
        "missing --alias value",
    );
    err_contains(&["peer-probe", "format", "--bad"], "unknown argument --bad");

    ok_contains(&["peer-probe", "handshake", "--legacy-true"], "true");
    ok_contains(&["peer-probe", "handshake", "--empty-object"], "false");
    ok_contains(&["peer-probe", "handshake", "--other-truthy"], "false");
    ok_contains(&["peer-probe", "handshake", "--missing"], "false");
    err_contains(
        &["peer-probe", "handshake", "--schema"],
        "missing --schema value",
    );
    err_contains(
        &["peer-probe", "handshake", "--bad"],
        "unknown argument --bad",
    );

    err_contains(
        &["peer-sources", "--discovery-hint"],
        "missing --discovery-hint value",
    );
    err_contains(
        &["peer-sources", "--discovered"],
        "missing --discovered value",
    );
    err_contains(&["peer-sources", "--bad"], "unknown argument --bad");
    ok_contains(
        &[
            "peer-sources",
            "--discovered",
            "-|-|-|http://one,http://two",
        ],
        "peer-sources mode=both",
    );
}

#[test]
fn federation_sync_identity_and_health_error_edges() {
    ok_contains(
        &["federation-sync", "constants"],
        "federation-sync diffBuckets=",
    );
    err_contains(&["federation-sync", "--node"], "missing --node value");
    err_contains(&["federation-sync", "--agent"], "missing --agent value");
    err_contains(&["federation-sync", "--agent", "bad"], "--agent must use");
    err_contains(
        &["federation-sync", "--identity"],
        "missing --identity value",
    );
    err_contains(&["federation-sync", "--bad"], "unknown argument --bad");
    err_contains(
        &["federation-sync", "constants", "--bad"],
        "constants: unknown argument --bad",
    );

    ok_contains(
        &["federation-identity", "constants"],
        "federation-identity defaultNode=local",
    );
    err_contains(&["federation-identity", "--node"], "missing --node value");
    err_contains(&["federation-identity", "--url"], "missing --url value");
    err_contains(&["federation-identity", "--agent"], "missing --agent value");
    err_contains(
        &["federation-identity", "--agent", "bad"],
        "--agent must use",
    );
    err_contains(
        &["federation-identity", "constants", "--bad"],
        "constants: unknown argument --bad",
    );

    err_contains(
        &["federation-health", "--remote", "url"],
        "--remote must use",
    );
    err_contains(
        &["federation-health", "--remote", "url|http|nope"],
        "http status must be u16",
    );
    err_contains(
        &["federation-health", "--remote", "url|peer|-|-|maybe"],
        "reachability must be reachable or unreachable",
    );
    err_contains(
        &["federation-health", "--remote", "url|bad"],
        "--remote must use",
    );
}

#[test]
fn auto_pair_and_consent_pin_remaining_edges() {
    for (args, expected) in [
        (&["auto-pair-proof", "--node"][..], "missing --node value"),
        (
            &["auto-pair-proof", "--oracle"][..],
            "missing --oracle value",
        ),
        (&["auto-pair-proof", "--url"][..], "missing --url value"),
        (
            &["auto-pair-proof", "--pubkey"][..],
            "missing --pubkey value",
        ),
        (&["auto-pair-proof", "--token"][..], "missing --token value"),
        (&["auto-pair-proof", "--proof"][..], "missing --proof value"),
        (&["auto-pair-proof", "--bad"][..], "unknown argument --bad"),
    ] {
        err_contains(args, expected);
    }
    err_contains(&["auto-pair-proof"], "missing --node value");
    err_contains(
        &["auto-pair-proof", "--node", "n"],
        "missing --oracle value",
    );
    err_contains(
        &["auto-pair-proof", "--node", "n", "--oracle", "o"],
        "missing --url value",
    );
    err_contains(
        &[
            "auto-pair-proof",
            "--node",
            "n",
            "--oracle",
            "o",
            "--url",
            "u",
        ],
        "missing --pubkey value",
    );
    err_contains(
        &[
            "auto-pair-proof",
            "--node",
            "n",
            "--oracle",
            "o",
            "--url",
            "u",
            "--pubkey",
            "p",
        ],
        "missing --token value",
    );
    ok_contains(
        &[
            "auto-pair-proof",
            "--node",
            "n",
            "--oracle",
            "o",
            "--url",
            "u",
            "--pubkey",
            "p",
            "--token",
            "t",
        ],
        "auto-pair-proof proof=",
    );
    ok_contains(
        &[
            "auto-pair-proof",
            "--node",
            "n",
            "--oracle",
            "o",
            "--url",
            "u",
            "--pubkey",
            "p",
            "--token",
            "t",
            "--proof",
            "bad",
        ],
        "valid=false",
    );

    for (args, expected) in [
        (&["consent-pin", "--pin"][..], "missing --pin value"),
        (
            &["consent-pin", "--expected-hash"][..],
            "missing --expected-hash value",
        ),
        (
            &["consent-pin", "--request-id-bytes"][..],
            "missing --request-id-bytes value",
        ),
        (
            &["consent-pin", "--request-id-bytes", "bad"][..],
            "must use comma-separated u8 values",
        ),
        (&["consent-pin", "--bad"][..], "unknown argument --bad"),
        (
            &[
                "consent-pin",
                "--pin",
                "ABCDEF",
                "--request-id-bytes",
                "1,2",
            ][..],
            "expected exactly one",
        ),
        (&["consent-pin"][..], "expected --pin or --request-id-bytes"),
    ] {
        err_contains(args, expected);
    }
    ok_contains(
        &[
            "consent-pin",
            "--request-id-bytes",
            "0,1,2,3,4,5,6,7,8,9,10,11",
        ],
        "consent-pin requestId=000102030405060708090a0b",
    );
    ok_contains(&["consent-pin", "--pin", "abc-def"], "valid=true");
    ok_contains(
        &["consent-pin", "--pin", "abc-def", "--expected-hash", "bad"],
        "verified=false",
    );
    ok_contains(&["consent-constants"], "consent-constants actions=");
    err_contains(&["consent-constants", "--bad"], "unknown argument --bad");
}

