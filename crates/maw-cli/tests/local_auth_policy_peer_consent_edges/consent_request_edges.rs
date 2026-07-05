#[test]
fn consent_request_remaining_parser_edges() {
    for (args, expected) in [
        (&["consent-request", "--from"][..], "missing --from value"),
        (&["consent-request", "--to"][..], "missing --to value"),
        (
            &["consent-request", "--action"][..],
            "missing --action value",
        ),
        (
            &["consent-request", "--action", "bad"][..],
            "invalid --action value",
        ),
        (
            &["consent-request", "--summary"][..],
            "missing --summary value",
        ),
        (
            &["consent-request", "--peer-url"][..],
            "missing --peer-url value",
        ),
        (
            &["consent-request", "--request-id"][..],
            "missing --request-id value",
        ),
        (&["consent-request", "--pin"][..], "missing --pin value"),
        (&["consent-request", "--now"][..], "missing --now value"),
        (
            &["consent-request", "--now", "bad"][..],
            "--now must be an integer",
        ),
        (
            &["consent-request", "--peer-http-status"][..],
            "missing --peer-http-status value",
        ),
        (
            &["consent-request", "--peer-http-status", "bad"][..],
            "must be u16",
        ),
        (
            &["consent-request", "--peer-network-error"][..],
            "missing --peer-network-error value",
        ),
        (&["consent-request", "--bad"][..], "unknown argument --bad"),
    ] {
        err_contains(args, expected);
    }
    err_contains(&["consent-request"], "missing --from value");
    err_contains(&["consent-request", "--from", "a"], "missing --to value");
    err_contains(
        &["consent-request", "--from", "a", "--to", "b"],
        "missing --action value",
    );
    err_contains(
        &[
            "consent-request",
            "--from",
            "a",
            "--to",
            "b",
            "--action",
            "hey",
        ],
        "missing --summary value",
    );
    err_contains(
        &[
            "consent-request",
            "--from",
            "a",
            "--to",
            "b",
            "--action",
            "hey",
            "--summary",
            "s",
        ],
        "missing --request-id value",
    );
    err_contains(
        &[
            "consent-request",
            "--from",
            "a",
            "--to",
            "b",
            "--action",
            "hey",
            "--summary",
            "s",
            "--request-id",
            "r",
        ],
        "missing --pin value",
    );
    err_contains(
        &[
            "consent-request",
            "--from",
            "a",
            "--to",
            "b",
            "--action",
            "hey",
            "--summary",
            "s",
            "--request-id",
            "r",
            "--pin",
            "ABCDEF",
        ],
        "missing --now value",
    );
    ok_contains(
        &[
            "consent-request",
            "--from",
            "a",
            "--to",
            "b",
            "--action",
            "team-invite",
            "--summary",
            "s",
            "--request-id",
            "r",
            "--pin",
            "ABCDEF",
            "--now",
            "1000",
            "--peer-ok",
        ],
        "consent-request ok=true requestId=r",
    );
    ok_contains(
        &[
            "consent-request",
            "--from",
            "a",
            "--to",
            "b",
            "--action",
            "plugin-install",
            "--summary",
            "s",
            "--request-id",
            "r",
            "--pin",
            "ABCDEF",
            "--now",
            "1000",
            "--peer-network-error",
            "boom",
        ],
        "ok=false",
    );
}
