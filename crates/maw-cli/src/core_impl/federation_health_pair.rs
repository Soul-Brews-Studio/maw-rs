fn parse_federation_health_peer(value: &str) -> Result<FederationPeerStatus, String> {
    let parts: Vec<&str> = value.split('|').collect();
    if parts.len() != 6 {
        return Err("federation-health: --peer must use <url|node|-|reachable|unreachable|latency|-|agents|ok|clock>".to_owned());
    }
    Ok(FederationPeerStatus {
        url: parts[0].to_owned(),
        node: optional_dash(parts[1]),
        reachable: parse_reachable(parts[2], "federation-health: --peer")?,
        latency: parse_optional_u64(parts[3], "federation-health: --peer latency must be u64")?,
        agents: parse_csv(parts[4]),
        clock_warning: match parts[5] {
            "ok" => false,
            "clock" => true,
            _ => return Err("federation-health: --peer clock flag must be ok or clock".to_owned()),
        },
    })
}

fn parse_federation_health_remote(
    value: &str,
) -> Result<(String, PeerFederationStatusResult), String> {
    let parts: Vec<&str> = value.split('|').collect();
    if parts.len() < 2 {
        return Err("federation-health: --remote must use <url|kind|...>".to_owned());
    }
    let url = parts[0];
    let kind = parts[1];
    let status = match kind {
        "missing-peers" if parts.len() == 2 => PeerFederationStatusResult::MissingPeers,
        "http" if parts.len() == 3 => PeerFederationStatusResult::HttpStatus(
            parts[2]
                .parse::<u16>()
                .map_err(|_| "federation-health: --remote http status must be u16".to_owned())?,
        ),
        "fetch-error" if parts.len() == 3 => {
            PeerFederationStatusResult::FetchError(parts[2].to_owned())
        }
        "peer" if parts.len() == 5 => PeerFederationStatusResult::Ok(PeerFederationStatus {
            peers: vec![FederationPeerView {
                url: optional_dash(parts[2]),
                node: optional_dash(parts[3]),
                reachable: Some(parse_reachable(parts[4], "federation-health: --remote peer")?),
            }],
        }),
        _ => {
            return Err(
                "federation-health: --remote must use <url|missing-peers>, <url|http|status>, <url|fetch-error|message>, or <url|peer|view-url|view-node|reachable>".to_owned(),
            )
        }
    };
    Ok((url.to_owned(), status))
}

fn parse_reachable(value: &str, prefix: &str) -> Result<bool, String> {
    match value {
        "reachable" => Ok(true),
        "unreachable" => Ok(false),
        _ => Err(format!(
            "{prefix} reachability must be reachable or unreachable"
        )),
    }
}

fn parse_csv(value: &str) -> Vec<String> {
    value
        .split(',')
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn optional_dash(value: &str) -> Option<String> {
    (!value.is_empty() && value != "-").then(|| value.to_owned())
}

fn render_federation_health_plan_json(status: &SymmetricFederationStatus) -> String {
    format!(
        "{{\"command\":\"federation-health\",\"localUrl\":{},\"localNode\":{},\"healthyPairs\":{},\"totalPairs\":{},\"pairs\":{}}}\n",
        json_string(&status.local_url),
        json_string(&status.local_node),
        status.healthy_pairs,
        status.total_pairs,
        render_pair_statuses_json(&status.pairs)
    )
}

fn render_pair_statuses_json(pairs: &[PairStatus]) -> String {
    format!(
        "[{}]",
        pairs
            .iter()
            .map(render_pair_status_json)
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_pair_status_json(pair: &PairStatus) -> String {
    let mut fields = vec![
        format!("\"url\":{}", json_string(&pair.url)),
        format!("\"pair\":{}", json_string(pair.pair.as_str())),
        format!("\"forward\":{}", pair.forward),
        format!("\"agents\":{}", json_string_array(&pair.agents)),
        format!("\"clockWarning\":{}", pair.clock_warning),
    ];
    push_json_opt(&mut fields, "node", pair.node.as_deref());
    match pair.reverse {
        Some(reverse) => fields.push(format!("\"reverse\":{reverse}")),
        None => fields.push("\"reverse\":null".to_owned()),
    }
    match pair.latency {
        Some(latency) => fields.push(format!("\"latency\":{latency}")),
        None => fields.push("\"latency\":null".to_owned()),
    }
    push_json_opt(&mut fields, "reason", pair.reason.as_deref());
    format!("{{{}}}", fields.join(","))
}

fn render_federation_health_plan_text(status: &SymmetricFederationStatus) -> String {
    format!(
        "federation-health healthyPairs={} totalPairs={}\n",
        status.healthy_pairs, status.total_pairs
    )
}

fn run_federation_health_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return federation_health_constants_usage_error(&format!(
                    "federation-health constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_federation_health_constants_json()
        } else {
            "federation-health pairHealth=healthy,half-up,down,unknown peerReachability=reachable,unreachable remoteKinds=missing-peers,http,fetch-error,peer clockFlags=ok,clock\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_federation_health_constants_json() -> String {
    r#"{"command":"federation-health","action":"constants","pairHealth":["healthy","half-up","down","unknown"],"peerReachability":["reachable","unreachable"],"remoteKinds":["missing-peers","http","fetch-error","peer"],"clockFlags":["ok","clock"]}
"#
    .to_owned()
}

fn federation_health_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", federation_health_usage()),
    }
}

fn federation_health_usage() -> &'static str {
    "usage: maw-rs federation-health [--node <name>] [--local-url <url>] [--peer <url|node|-|reachable|unreachable|latency|-|agents|ok|clock>]... [--remote <url|missing-peers|http|fetch-error|peer...>]... [--plan-json]
       maw-rs federation-health constants [--plan-json]"
}

fn federation_health_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}
{}
",
            federation_health_constants_usage()
        ),
    }
}

fn federation_health_constants_usage() -> &'static str {
    "usage: maw-rs federation-health constants [--plan-json]"
}

fn run_auto_pair_proof_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut node = None::<String>;
    let mut oracle = None::<String>;
    let mut url = None::<String>;
    let mut pubkey = None::<String>;
    let mut token = None::<String>;
    let mut proof = None::<String>;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return auto_pair_proof_usage_error("auto-pair-proof: missing --node value");
                };
                node = Some(value.to_owned());
                index += 1;
            }
            "--oracle" => {
                let Some(value) = argv.get(index + 1) else {
                    return auto_pair_proof_usage_error("auto-pair-proof: missing --oracle value");
                };
                oracle = Some(value.to_owned());
                index += 1;
            }
            "--url" => {
                let Some(value) = argv.get(index + 1) else {
                    return auto_pair_proof_usage_error("auto-pair-proof: missing --url value");
                };
                url = Some(value.to_owned());
                index += 1;
            }
            "--pubkey" => {
                let Some(value) = argv.get(index + 1) else {
                    return auto_pair_proof_usage_error("auto-pair-proof: missing --pubkey value");
                };
                pubkey = Some(value.to_owned());
                index += 1;
            }
            "--token" => {
                let Some(value) = argv.get(index + 1) else {
                    return auto_pair_proof_usage_error("auto-pair-proof: missing --token value");
                };
                token = Some(value.to_owned());
                index += 1;
            }
            "--proof" => {
                let Some(value) = argv.get(index + 1) else {
                    return auto_pair_proof_usage_error("auto-pair-proof: missing --proof value");
                };
                proof = Some(value.to_owned());
                index += 1;
            }
            arg => {
                return auto_pair_proof_usage_error(&format!(
                    "auto-pair-proof: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(node) = node else {
        return auto_pair_proof_usage_error("auto-pair-proof: missing --node value");
    };
    let Some(oracle) = oracle else {
        return auto_pair_proof_usage_error("auto-pair-proof: missing --oracle value");
    };
    let Some(url) = url else {
        return auto_pair_proof_usage_error("auto-pair-proof: missing --url value");
    };
    let Some(pubkey) = pubkey else {
        return auto_pair_proof_usage_error("auto-pair-proof: missing --pubkey value");
    };
    let Some(token) = token else {
        return auto_pair_proof_usage_error("auto-pair-proof: missing --token value");
    };

    let identity = AutoPairIdentity {
        node,
        oracle,
        url,
        pubkey,
    };
    let signed_proof = sign_auto_pair_proof(&identity, &token);
    let valid = proof
        .as_deref()
        .map(|proof| verify_auto_pair_proof(&identity, &token, proof));

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_auto_pair_proof_plan_json(&identity, &signed_proof, valid)
        } else {
            render_auto_pair_proof_plan_text(&signed_proof, valid)
        },
        stderr: String::new(),
    }
}

fn render_auto_pair_proof_plan_json(
    identity: &AutoPairIdentity,
    proof: &str,
    valid: Option<bool>,
) -> String {
    let valid = valid.map_or_else(|| "null".to_owned(), |value| value.to_string());
    format!(
        "{{\"command\":\"auto-pair-proof\",\"node\":{},\"oracle\":{},\"url\":{},\"pubkey\":{},\"token\":null,\"proof\":{},\"valid\":{valid}}}\n",
        json_string(&identity.node),
        json_string(&identity.oracle),
        json_string(&identity.url),
        json_string(&identity.pubkey),
        json_string(proof)
    )
}

fn render_auto_pair_proof_plan_text(proof: &str, valid: Option<bool>) -> String {
    match valid {
        Some(valid) => format!("auto-pair-proof proof={proof} valid={valid}\n"),
        None => format!("auto-pair-proof proof={proof}\n"),
    }
}

fn auto_pair_proof_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", auto_pair_proof_usage()),
    }
}

fn auto_pair_proof_usage() -> &'static str {
    "usage: maw-rs auto-pair-proof --node <node> --oracle <oracle> --url <url> --pubkey <pubkey> --token <token> [--proof <hex>] [--plan-json]"
}

fn run_consent_pin_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut pin = None::<String>;
    let mut expected_hash = None::<String>;
    let mut request_id_bytes = None::<Vec<u8>>;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--pin" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pin_usage_error("consent-pin: missing --pin value");
                };
                pin = Some(value.to_owned());
                index += 1;
            }
            "--expected-hash" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pin_usage_error("consent-pin: missing --expected-hash value");
                };
                expected_hash = Some(value.to_owned());
                index += 1;
            }
            "--request-id-bytes" => {
                let Some(value) = argv.get(index + 1) else {
                    return consent_pin_usage_error(
                        "consent-pin: missing --request-id-bytes value",
                    );
                };
                match parse_pair_code_bytes(value) {
                    Ok(parsed) => request_id_bytes = Some(parsed),
                    Err(_) => {
                        return consent_pin_usage_error(
                            "consent-pin: --request-id-bytes must use comma-separated u8 values",
                        )
                    }
                }
                index += 1;
            }
            arg => return consent_pin_usage_error(&format!("consent-pin: unknown argument {arg}")),
        }
        index += 1;
    }

    if pin.is_some() && request_id_bytes.is_some() {
        return consent_pin_usage_error(
            "consent-pin: expected exactly one of --pin or --request-id-bytes",
        );
    }
    let Some(pin) = pin else {
        let Some(bytes) = request_id_bytes else {
            return consent_pin_usage_error("consent-pin: expected --pin or --request-id-bytes");
        };
        let request_id = consent_request_id_from_bytes(&bytes);
        return CliOutput {
            code: 0,
            stdout: if plan_json {
                render_consent_pin_request_id_json(&request_id)
            } else {
                format!("consent-pin requestId={request_id}\n")
            },
            stderr: String::new(),
        };
    };

    let normalized = normalize_pair_code(&pin);
    let redacted = redact_pair_code(&normalized);
    let valid = is_valid_pair_code_shape(&normalized);
    let pin_hash = hash_consent_pin(&normalized);
    let verified = expected_hash
        .as_deref()
        .map(|expected| verify_consent_pin(&normalized, expected));

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_consent_pin_plan_json(&normalized, &redacted, valid, &pin_hash, verified)
        } else {
            render_consent_pin_plan_text(&redacted, valid, verified)
        },
        stderr: String::new(),
    }
}

