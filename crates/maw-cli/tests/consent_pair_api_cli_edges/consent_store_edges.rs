use maw_cli::{run_cli, CliOutput};
use serde_json::Value;

const TRUST_ENTRY: &str = "from=neo,to=mawjs,action=hey,approved_at=2026-01-02T00:00:00.000Z,approved_by=auto,request_id=req-1";
const PENDING_REQ: &str = "id=req-1,from=neo,to=mawjs,action=hey,summary=hello,pin_hash=hash,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending";
const PENDING_REQ_2: &str = "id=req-2,from=trinity,to=mawjs,action=plugin-install,summary=install,pin_hash=hash2,created_at=2026-01-02T00:00:01.000Z,expires_at=2026-01-02T00:01:01.000Z,status=approved";
const TOKEN: &str = "abababababababababababababababababababababababababababababababab";
const PUBKEY: &str = "pppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppppp";

fn run(args: &[&str]) -> CliOutput {
    run_cli(
        &args
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>(),
    )
}

fn json(args: &[&str]) -> Value {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
    serde_json::from_str(&output.stdout)
        .unwrap_or_else(|error| panic!("invalid json for {args:?}: {error}\n{}", output.stdout))
}

fn assert_usage(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
    assert!(
        output.stdout.is_empty(),
        "stdout for {args:?}: {}",
        output.stdout
    );
}

fn assert_text(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(expected),
        "stdout for {args:?} did not contain {expected:?}: {}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
}

#[test]
fn consent_approval_text_and_remaining_parser_edges_are_covered() {
    assert_text(
        &[
            "consent-approval",
            "approve",
            "--request-id",
            "req-1",
            "--from",
            "neo",
            "--to",
            "mawjs",
            "--action",
            "hey",
            "--summary",
            "hello",
            "--pin",
            "ABCDEF",
            "--created-at",
            "1767312000000",
            "--now",
            "1767312001000",
        ],
        "consent-approval mode=approve ok=true pendingStatus=approved trusted=true",
    );

    for (args, expected) in [
        (
            &["consent-approval", "approve", "--request-id"][..],
            "missing --request-id value",
        ),
        (
            &["consent-approval", "approve", "--from"][..],
            "missing --from value",
        ),
        (
            &["consent-approval", "approve", "--to"][..],
            "missing --to value",
        ),
        (
            &["consent-approval", "approve", "--action"][..],
            "missing --action value",
        ),
        (
            &["consent-approval", "approve", "--action", "bogus"][..],
            "invalid --action value",
        ),
        (
            &["consent-approval", "approve", "--summary"][..],
            "missing --summary value",
        ),
        (
            &["consent-approval", "approve", "--pin"][..],
            "missing --pin value",
        ),
        (
            &["consent-approval", "approve", "--created-at"][..],
            "missing --created-at value",
        ),
        (
            &["consent-approval", "approve", "--now"][..],
            "missing --now value",
        ),
        (
            &["consent-approval", "approve", "--unexpected"][..],
            "unknown argument --unexpected",
        ),
    ] {
        assert_usage(args, expected);
    }

    let required_prefix = [
        "consent-approval",
        "approve",
        "--request-id",
        "req-1",
        "--from",
        "neo",
        "--to",
        "mawjs",
        "--action",
        "hey",
        "--summary",
        "hello",
        "--pin",
        "ABCDEF",
        "--created-at",
        "1767312000000",
    ];
    assert_usage(&required_prefix, "missing --now value");
}

#[test]
fn consent_store_text_outputs_and_parse_failures_are_covered() {
    assert_text(
        &[
            "consent-store",
            "trust",
            "--entry",
            TRUST_ENTRY,
            "--check",
            "neo:mawjs:hey",
            "--key",
            "neo:mawjs:hey",
        ],
        "consent-store trust trusted=true trustKey=neo→mawjs:hey",
    );
    assert_text(
        &[
            "consent-store",
            "pending",
            "--request",
            PENDING_REQ,
            "--set-status",
            "req-1:expired",
        ],
        "consent-store pending updated=true",
    );

    for (args, expected) in [
        (&["consent-store"][..], "expected trust or pending"),
        (
            &["consent-store", "pending", "--request"][..],
            "missing --request value",
        ),
        (
            &["consent-store", "trust", "--entry", "not-fields"][..],
            "expected key=value fields",
        ),
        (
            &["consent-store", "trust", "--entry", "=v"][..],
            "expected non-empty field name",
        ),
        (
            &[
                "consent-store",
                "trust",
                "--entry",
                "from=a,to=b,action=bad,approved_at=t,approved_by=human",
            ][..],
            "invalid action",
        ),
        (
            &[
                "consent-store",
                "trust",
                "--entry",
                "from=a,to=b,action=hey,approved_at=t,approved_by=robot",
            ][..],
            "invalid approved_by",
        ),
        (
            &["consent-store", "trust", "--check"][..],
            "missing --check value",
        ),
        (
            &["consent-store", "trust", "--check", "a:b:c:d"][..],
            "key must use from:to:action",
        ),
        (
            &["consent-store", "trust", "--key"][..],
            "missing --key value",
        ),
        (
            &["consent-store", "pending", "--set-status"][..],
            "missing --set-status value",
        ),
        (
            &["consent-store", "pending", "--set-status", "req-1"][..],
            "--set-status must use id:status",
        ),
        (
            &["consent-store", "pending", "--set-status", ":pending"][..],
            "--set-status missing id",
        ),
        (
            &["consent-store", "pending", "--set-status", "req-1:bogus"][..],
            "invalid status",
        ),
        (
            &["consent-store", "pending", "--odd"][..],
            "unknown argument --odd",
        ),
    ] {
        assert_usage(args, expected);
    }
}

#[test]
fn consent_expiry_cleanup_trust_and_pending_text_and_errors_are_covered() {
    assert_text(
        &[
            "consent-expiry",
            "--request",
            PENDING_REQ,
            "--now",
            "1767312120000",
        ],
        "consent-expiry id=req-1 status=expired expired=true",
    );
    assert_text(
        &[
            "consent-cleanup",
            "--request",
            PENDING_REQ,
            "--request",
            PENDING_REQ_2,
            "--delete",
            "req-1",
        ],
        "consent-cleanup deletedId=req-1 deleted=true",
    );
    assert_text(
        &[
            "consent-trust-revoke",
            "--entry",
            TRUST_ENTRY,
            "--revoke",
            "neo:mawjs:hey",
        ],
        "consent-trust-revoke revokedKey=neo→mawjs:hey revoked=true",
    );
    assert_text(
        &[
            "consent-trust-check",
            "--entry",
            TRUST_ENTRY,
            "--check",
            "neo:mawjs:hey",
        ],
        "consent-trust-check trustKey=neo→mawjs:hey trusted=true",
    );
    assert_text(
        &[
            "consent-pending-read",
            "--request",
            PENDING_REQ,
            "--id",
            "req-1",
        ],
        "consent-pending-read id=req-1 found=true",
    );
    assert_text(
        &[
            "consent-pending-status",
            "--request",
            PENDING_REQ,
            "--set-status",
            "req-1:approved",
        ],
        "consent-pending-status id=req-1 updated=true",
    );

    for (args, expected) in [
        (
            &["consent-expiry", "--request"][..],
            "missing --request value",
        ),
        (&["consent-expiry", "--now"][..], "missing --now value"),
        (
            &["consent-expiry", "--now", "bad"][..],
            "--now must be an integer",
        ),
        (&["consent-expiry", "--odd"][..], "unknown argument --odd"),
        (
            &["consent-expiry", "--request", PENDING_REQ][..],
            "missing --now value",
        ),
        (
            &["consent-cleanup", "--request"][..],
            "missing --request value",
        ),
        (
            &["consent-cleanup", "--delete"][..],
            "missing --delete value",
        ),
        (
            &["consent-cleanup", "--delete", ""][..],
            "missing --delete value",
        ),
        (&["consent-cleanup", "--odd"][..], "unknown argument --odd"),
        (
            &["consent-cleanup", "--request", PENDING_REQ][..],
            "missing --delete value",
        ),
        (
            &["consent-trust-revoke", "--entry"][..],
            "missing --entry value",
        ),
        (
            &["consent-trust-revoke", "--revoke"][..],
            "missing --revoke value",
        ),
        (
            &["consent-trust-revoke", "--revoke", "bad"][..],
            "key must use from:to:action",
        ),
        (
            &["consent-trust-revoke", "--odd"][..],
            "unknown argument --odd",
        ),
        (&["consent-trust-revoke"][..], "missing --revoke value"),
        (
            &["consent-trust-check", "--entry"][..],
            "missing --entry value",
        ),
        (
            &["consent-trust-check", "--check"][..],
            "missing --check value",
        ),
        (
            &["consent-trust-check", "--check", "bad"][..],
            "key must use from:to:action",
        ),
        (
            &["consent-trust-check", "--odd"][..],
            "unknown argument --odd",
        ),
        (&["consent-trust-check"][..], "missing --check value"),
        (
            &["consent-pending-read", "--request"][..],
            "missing --request value",
        ),
        (&["consent-pending-read", "--id"][..], "missing --id value"),
        (
            &["consent-pending-read", "--id", ""][..],
            "missing --id value",
        ),
        (
            &["consent-pending-read", "--odd"][..],
            "unknown argument --odd",
        ),
        (&["consent-pending-read"][..], "missing --id value"),
        (
            &["consent-pending-status", "--request"][..],
            "missing --request value",
        ),
        (
            &["consent-pending-status", "--set-status"][..],
            "missing --set-status value",
        ),
        (
            &["consent-pending-status", "--set-status", "bad"][..],
            "--set-status must use id:status",
        ),
        (
            &["consent-pending-status", "--odd"][..],
            "unknown argument --odd",
        ),
        (
            &["consent-pending-status"][..],
            "missing --set-status value",
        ),
    ] {
        assert_usage(args, expected);
    }
}

#[test]
fn recent_hello_pair_code_and_pair_code_store_remaining_edges_are_covered() {
    assert_text(
        &[
            "recent-hello",
            "--hello",
            "zid-a:1000",
            "--zid",
            "zid-a",
            "--now",
            "61000",
        ],
        "recent-hello zid=zid-a recent=true",
    );
    assert_text(
        &["recent-hello", "constants"],
        "recent-hello windowMs=60000",
    );
    assert_text(
        &["pair-code", "--code", "abc234"],
        "pair-code ABC-234 valid=true",
    );
    assert_text(&["pair-code", "constants"], "pair-code alphabet=");
    assert_text(
        &[
            "pair-code-store",
            "lookup",
            "--seed-code",
            "ABC234:60000:1000",
            "--code",
            "ABC234",
            "--now",
            "61001",
        ],
        "pair-code-store mode=lookup code=ABC234 state=expired",
    );
    assert_text(
        &["pair-code-store", "constants"],
        "pair-code-store constants modes=register,lookup,consume",
    );

    let code_json = json(&["pair-code", "--bytes", "0,1,2,3", "--plan-json"]);
    assert_eq!(code_json["command"], "pair-code");
    assert!(code_json["normalized"].as_str().is_some());

    for (args, expected) in [
        (&["recent-hello", "--hello"][..], "missing --hello value"),
        (
            &["recent-hello", "--hello", "bad"][..],
            "invalid hello timestamp",
        ),
        (
            &["recent-hello", "--hello", ":1000"][..],
            "invalid hello timestamp",
        ),
        (&["recent-hello", "--zid"][..], "missing --zid value"),
        (&["recent-hello", "--zid", ""][..], "missing --zid value"),
        (&["recent-hello", "--now"][..], "missing --now value"),
        (&["recent-hello", "--now", "bad"][..], "invalid --now value"),
        (&["recent-hello", "--odd"][..], "unknown argument --odd"),
        (&["recent-hello"][..], "missing --zid value"),
        (
            &["recent-hello", "constants", "--odd"][..],
            "constants: unknown argument --odd",
        ),
        (&["pair-code", "--bytes"][..], "missing --bytes value"),
        (
            &["pair-code", "--bytes", "1,,2"][..],
            "--bytes must use comma-separated u8 values",
        ),
        (
            &["pair-code", "--bytes", "256"][..],
            "--bytes must use comma-separated u8 values",
        ),
        (&["pair-code", "--odd"][..], "unknown argument --odd"),
        (&["pair-code"][..], "expected --code or --bytes"),
        (
            &["pair-code-store"][..],
            "expected register, lookup, or consume",
        ),
        (
            &["pair-code-store", "bogus"][..],
            "expected register, lookup, or consume",
        ),
        (
            &["pair-code-store", "lookup", "--code"][..],
            "missing --code value",
        ),
        (
            &["pair-code-store", "lookup", "--code", ""][..],
            "missing --code value",
        ),
        (
            &["pair-code-store", "lookup", "--now"][..],
            "missing --now value",
        ),
        (
            &["pair-code-store", "lookup", "--now", "bad"][..],
            "--now must be a non-negative integer",
        ),
        (
            &["pair-code-store", "register", "--ttl-ms"][..],
            "missing --ttl-ms value",
        ),
        (
            &["pair-code-store", "register", "--ttl-ms", "bad"][..],
            "--ttl-ms must be a non-negative integer",
        ),
        (
            &["pair-code-store", "lookup", "--seed-code"][..],
            "missing --seed-code value",
        ),
        (
            &["pair-code-store", "lookup", "--odd"][..],
            "unknown argument --odd",
        ),
        (
            &["pair-code-store", "lookup", "--now", "1"][..],
            "missing --code value",
        ),
        (
            &[
                "pair-code-store",
                "register",
                "--code",
                "ABC234",
                "--now",
                "1",
            ][..],
            "missing --ttl-ms value",
        ),
        (
            &["pair-code-store", "constants", "--odd"][..],
            "constants: unknown arg --odd",
        ),
    ] {
        assert_usage(args, expected);
    }
}

