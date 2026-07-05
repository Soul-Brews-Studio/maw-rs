#[test]
fn pair_api_text_outputs_config_errors_and_seed_parsing_edges_are_covered() {
    assert_text(
        &[
            "pair-api",
            "generate",
            "--code",
            "ABC234",
            "--node",
            "node-a",
            "--oracle",
            "oracle-a",
            "--port",
            "4567",
            "--base-url",
            "http://localhost:4567",
            "--federation-token",
            TOKEN,
            "--pubkey",
            PUBKEY,
            "--now",
            "1000",
            "--ttl-ms",
            "5000",
        ],
        "pair-api generate status=201 code=ABC-234",
    );
    assert_text(
        &[
            "pair-api",
            "probe",
            "--code",
            "ABC234",
            "--seed-code",
            "ABC234:5000:1000",
            "--node",
            "node-a",
            "--oracle",
            "oracle-a",
            "--port",
            "4567",
            "--base-url",
            "http://localhost:4567",
            "--federation-token",
            TOKEN,
            "--pubkey",
            PUBKEY,
            "--now",
            "1000",
        ],
        "pair-api probe status=200 ok=true",
    );
    assert_text(
        &[
            "pair-api",
            "accept",
            "--code",
            "ABC234",
            "--seed-code",
            "ABC234:5000:1000",
            "--remote-node",
            "remote",
            "--remote-url",
            "http://remote",
            "--node",
            "node-a",
            "--oracle",
            "oracle-a",
            "--port",
            "4567",
            "--base-url",
            "http://localhost:4567",
            "--federation-token",
            TOKEN,
            "--pubkey",
            PUBKEY,
            "--now",
            "1000",
        ],
        "pair-api accept status=200 ok=true",
    );
    assert_text(
        &[
            "pair-api",
            "status",
            "--code",
            "ABC234",
            "--seed-code",
            "ABC234:5000:1000",
            "--seed-accepted",
            "remote=http://remote",
            "--node",
            "node-a",
            "--oracle",
            "oracle-a",
            "--port",
            "4567",
            "--base-url",
            "http://localhost:4567",
            "--federation-token",
            TOKEN,
            "--pubkey",
            PUBKEY,
            "--now",
            "1000",
        ],
        "pair-api status status=200 ok=true",
    );
    assert_text(
        &["pair-api", "constants"],
        "pair-api constants endpoints=generate,probe,accept,status",
    );

    let status = json(&[
        "pair-api",
        "status",
        "--code",
        "ABC234",
        "--seed-code",
        "ABC234:5000:1000",
        "--seed-accepted",
        "remote=http://remote",
        "--node",
        "node-a",
        "--oracle",
        "oracle-a",
        "--port",
        "4567",
        "--base-url",
        "http://localhost:4567",
        "--federation-token",
        TOKEN,
        "--pubkey",
        PUBKEY,
        "--now",
        "1000",
        "--plan-json",
    ]);
    assert_eq!(status["consumed"], true);
    assert_eq!(status["remoteNode"], "remote");

    for (args, expected) in [
        (&["pair-api", "probe", "--node"][..], "missing --node value"),
        (
            &["pair-api", "probe", "--oracle"][..],
            "missing --oracle value",
        ),
        (&["pair-api", "probe", "--port"][..], "missing --port value"),
        (
            &["pair-api", "probe", "--base-url"][..],
            "missing --base-url value",
        ),
        (
            &["pair-api", "probe", "--federation-token"][..],
            "missing --federation-token value",
        ),
        (
            &["pair-api", "probe", "--pubkey"][..],
            "missing --pubkey value",
        ),
        (&["pair-api", "probe", "--now"][..], "missing --now value"),
        (
            &["pair-api", "probe", "--now", "bad"][..],
            "--now must be a non-negative integer",
        ),
        (&["pair-api", "probe", "--code"][..], "missing --code value"),
        (
            &["pair-api", "generate", "--expires-sec"][..],
            "missing --expires-sec value",
        ),
        (
            &["pair-api", "generate", "--expires-sec", "bad"][..],
            "--expires-sec must be a non-negative integer",
        ),
        (
            &["pair-api", "generate", "--ttl-ms"][..],
            "missing --ttl-ms value",
        ),
        (
            &["pair-api", "generate", "--ttl-ms", "bad"][..],
            "--ttl-ms must be a non-negative integer",
        ),
        (
            &["pair-api", "probe", "--seed-code"][..],
            "missing --seed-code value",
        ),
        (
            &["pair-api", "probe", "--seed-code", ":1:2"][..],
            "--seed-code must be code:ttl_ms:created_at_ms",
        ),
        (
            &["pair-api", "probe", "--seed-code", "ABC234"][..],
            "--seed-code must be code:ttl_ms:created_at_ms",
        ),
        (
            &["pair-api", "probe", "--seed-code", "ABC234:1"][..],
            "--seed-code must be code:ttl_ms:created_at_ms",
        ),
        (
            &["pair-api", "probe", "--seed-code", "ABC234:1:2:3"][..],
            "--seed-code must be code:ttl_ms:created_at_ms",
        ),
        (
            &["pair-api", "probe", "--remote-node"][..],
            "missing --remote-node value",
        ),
        (
            &["pair-api", "probe", "--remote-url"][..],
            "missing --remote-url value",
        ),
        (
            &["pair-api", "probe", "--seed-accepted"][..],
            "missing --seed-accepted value",
        ),
        (
            &["pair-api", "probe", "--seed-accepted", "bad"][..],
            "--seed-accepted must be node=url",
        ),
        (
            &["pair-api", "probe", "--seed-accepted", "=url"][..],
            "--seed-accepted must be node=url",
        ),
        (
            &["pair-api", "probe", "--odd"][..],
            "unknown argument --odd",
        ),
        (&["pair-api", "probe"][..], "missing --code value"),
        (
            &["pair-api", "probe", "--code", "ABC234", "--now", "1"][..],
            "missing --node value",
        ),
        (
            &[
                "pair-api", "probe", "--code", "ABC234", "--now", "1", "--node", "node-a",
            ][..],
            "missing --oracle value",
        ),
        (
            &[
                "pair-api", "probe", "--code", "ABC234", "--now", "1", "--node", "node-a",
                "--oracle", "oracle-a",
            ][..],
            "missing --port value",
        ),
        (
            &[
                "pair-api", "probe", "--code", "ABC234", "--now", "1", "--node", "node-a",
                "--oracle", "oracle-a", "--port", "4567",
            ][..],
            "missing --base-url value",
        ),
        (
            &[
                "pair-api",
                "probe",
                "--code",
                "ABC234",
                "--now",
                "1",
                "--node",
                "node-a",
                "--oracle",
                "oracle-a",
                "--port",
                "4567",
                "--base-url",
                "http://localhost:4567",
            ][..],
            "missing --federation-token value",
        ),
        (
            &[
                "pair-api",
                "probe",
                "--code",
                "ABC234",
                "--now",
                "1",
                "--node",
                "node-a",
                "--oracle",
                "oracle-a",
                "--port",
                "4567",
                "--base-url",
                "http://localhost:4567",
                "--federation-token",
                TOKEN,
            ][..],
            "missing --pubkey value",
        ),
        (
            &["pair-api", "constants", "--odd"][..],
            "constants: unknown arg --odd",
        ),
    ] {
        assert_usage(args, expected);
    }
}

#[test]
fn pair_api_auto_text_constants_and_parser_edges_are_covered() {
    assert_text(
        &[
            "pair-api-auto",
            "--node",
            "node-a",
            "--oracle",
            "oracle-a",
            "--port",
            "4567",
            "--base-url",
            "http://localhost:4567",
            "--federation-token",
            TOKEN,
            "--pubkey",
            PUBKEY,
            "--now",
            "70001",
            "--remote-node",
            "remote",
            "--remote-url",
            "http://remote",
            "--zid",
            "zid-a",
            "--remote-oracle",
            "remote-oracle",
            "--remote-pubkey",
            PUBKEY,
            "--hello",
            "zid-a:70001",
            "--add-one-way",
        ],
        "pair-api-auto status=200 ok=true",
    );
    assert_text(
        &["pair-api-auto", "constants"],
        "pair-api-auto constants required=remote-node,remote-url,zid",
    );

    let add_error = json(&[
        "pair-api-auto",
        "--node",
        "node-a",
        "--oracle",
        "oracle-a",
        "--port",
        "4567",
        "--base-url",
        "http://localhost:4567",
        "--federation-token",
        TOKEN,
        "--pubkey",
        PUBKEY,
        "--now",
        "70001",
        "--remote-node",
        "remote",
        "--remote-url",
        "http://remote",
        "--zid",
        "zid-a",
        "--hello",
        "zid-a:70001",
        "--remote-pubkey",
        PUBKEY,
        "--add-error",
        "disk full",
        "--plan-json",
    ]);
    assert_eq!(add_error["status"], 400);
    assert_eq!(add_error["error"], "disk full");
    assert_eq!(add_error["add"], Value::Null);

    for (args, expected) in [
        (&["pair-api-auto", "--node"][..], "missing --node value"),
        (&["pair-api-auto", "--oracle"][..], "missing --oracle value"),
        (&["pair-api-auto", "--port"][..], "missing --port value"),
        (
            &["pair-api-auto", "--port", "bad"][..],
            "--port must be a u16",
        ),
        (
            &["pair-api-auto", "--base-url"][..],
            "missing --base-url value",
        ),
        (
            &["pair-api-auto", "--federation-token"][..],
            "missing --federation-token value",
        ),
        (&["pair-api-auto", "--pubkey"][..], "missing --pubkey value"),
        (&["pair-api-auto", "--now"][..], "missing --now value"),
        (
            &["pair-api-auto", "--now", "bad"][..],
            "--now must be a non-negative integer",
        ),
        (
            &["pair-api-auto", "--remote-node"][..],
            "missing --remote-node value",
        ),
        (
            &["pair-api-auto", "--remote-oracle"][..],
            "missing --remote-oracle value",
        ),
        (
            &["pair-api-auto", "--remote-url"][..],
            "missing --remote-url value",
        ),
        (&["pair-api-auto", "--zid"][..], "missing --zid value"),
        (
            &["pair-api-auto", "--remote-pubkey"][..],
            "missing --remote-pubkey value",
        ),
        (&["pair-api-auto", "--hello"][..], "missing --hello value"),
        (
            &["pair-api-auto", "--hello", "bad"][..],
            "--hello must be zid:seen_at_ms",
        ),
        (
            &["pair-api-auto", "--hello", ":1"][..],
            "--hello must be zid:seen_at_ms",
        ),
        (
            &["pair-api-auto", "--hello", "zid:bad"][..],
            "--hello seen_at_ms must be a non-negative integer",
        ),
        (
            &["pair-api-auto", "--add-pubkey-mismatch"][..],
            "missing --add-pubkey-mismatch value",
        ),
        (
            &["pair-api-auto", "--add-error"][..],
            "missing --add-error value",
        ),
        (&["pair-api-auto", "--odd"][..], "unknown argument --odd"),
        (&["pair-api-auto"][..], "missing --now value"),
        (&["pair-api-auto", "--now", "1"][..], "missing --node value"),
        (
            &["pair-api-auto", "constants", "--odd"][..],
            "constants: unknown arg --odd",
        ),
    ] {
        assert_usage(args, expected);
    }
}
