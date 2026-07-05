const DISPATCH_320: &[DispatcherEntry] = &[
    DispatcherEntry { command: "pair-api", handler: Handler::Sync(run_pair_api_plan) },
];

enum PairCodeStorePlanResult {
    Register(PairEntry),
    Lookup(LookupResult),
}

fn parse_pair_code_store_seed(value: &str) -> Result<SeedPairCode, String> {
    parse_seed_pair_code(value)
        .map_err(|message| message.replace("pair-api: --seed-code", "pair-code-store: --seed-code"))
}

fn pair_code_store_result_state(result: &PairCodeStorePlanResult) -> &'static str {
    match result {
        PairCodeStorePlanResult::Register(_)
        | PairCodeStorePlanResult::Lookup(LookupResult::Live(_)) => "live",
        PairCodeStorePlanResult::Lookup(LookupResult::NotFound) => "not-found",
        PairCodeStorePlanResult::Lookup(LookupResult::Expired) => "expired",
        PairCodeStorePlanResult::Lookup(LookupResult::Consumed) => "consumed",
    }
}

fn pair_code_store_result_entry(result: &PairCodeStorePlanResult) -> String {
    match result {
        PairCodeStorePlanResult::Register(entry)
        | PairCodeStorePlanResult::Lookup(LookupResult::Live(entry)) => {
            render_pair_code_store_entry_json(entry)
        }
        PairCodeStorePlanResult::Lookup(
            LookupResult::NotFound | LookupResult::Expired | LookupResult::Consumed,
        ) => "null".to_owned(),
    }
}

fn render_pair_code_store_entry_json(entry: &PairEntry) -> String {
    format!(
        "{{\"code\":{},\"expiresAt\":{},\"createdAt\":{},\"consumed\":{}}}",
        json_string(&entry.code),
        entry.expires_at,
        entry.created_at,
        entry.consumed
    )
}

fn render_pair_code_store_plan_json(
    mode: &str,
    normalized: &str,
    result: &PairCodeStorePlanResult,
) -> String {
    format!(
        "{{\"command\":\"pair-code-store\",\"mode\":{},\"normalized\":{},\"state\":{},\"entry\":{}}}\n",
        json_string(mode),
        json_string(normalized),
        json_string(pair_code_store_result_state(result)),
        pair_code_store_result_entry(result)
    )
}

fn run_pair_code_store_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => {
                return pair_code_store_constants_usage_error(&format!(
                    "pair-code-store constants: unknown arg {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_code_store_constants_json()
        } else {
            "pair-code-store constants modes=register,lookup,consume states=live,not-found,expired,consumed seed=code:ttl_ms:created_at_ms\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_pair_code_store_constants_json() -> String {
    r#"{"command":"pair-code-store","action":"constants","modes":["register","lookup","consume"],"states":["live","not-found","expired","consumed"],"seedCodeShape":"code:ttl_ms:created_at_ms","entryFields":["code","expiresAt","createdAt","consumed"],"normalization":"normalize-pair-code","registerRequires":["ttl-ms"],"lookupRequires":["code","now"]}
"#
    .to_owned()
}

fn pair_code_store_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_code_store_constants_usage()),
    }
}

fn pair_code_store_constants_usage() -> &'static str {
    "usage: maw-rs pair-code-store constants [--plan-json]"
}

fn pair_code_store_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_code_store_usage()),
    }
}

fn pair_code_store_usage() -> &'static str {
    "usage: maw-rs pair-code-store <register|lookup|consume> --code <code> --now <ms> [--ttl-ms <ms>] [--seed-code <code:ttl_ms:created_at_ms>]... [--plan-json]
       maw-rs pair-code-store constants [--plan-json]"
}

fn run_pair_api_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => {
                return pair_api_constants_usage_error(&format!(
                    "pair-api constants: unknown arg {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_api_constants_json()
        } else {
            "pair-api constants endpoints=generate,probe,accept,status statuses=live,not_found,expired,consumed,invalid_shape redacted=federationToken\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_pair_api_constants_json() -> String {
    r#"{"command":"pair-api","action":"constants","endpoints":["generate","probe","accept","status"],"probeStatuses":["live","not_found","expired","consumed","invalid_shape"],"acceptErrors":["bad_request","not_found","expired","consumed","invalid_shape"],"statusStates":["live","consumed","not_found","expired","invalid_shape"],"httpStatuses":{"generateCreated":201,"ok":200,"badRequest":400,"notFound":404,"gone":410},"seedCodeShape":"code:ttl_ms:created_at_ms","seedAcceptedShape":"node=url","redactedFields":["federationToken"]}
"#
    .to_owned()
}

fn pair_api_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_api_constants_usage()),
    }
}

fn pair_api_constants_usage() -> &'static str {
    "usage: maw-rs pair-api constants [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_pair_api_plan(argv: &[String]) -> CliOutput {
    let Some(endpoint) = argv.first().map(String::as_str) else {
        return pair_api_usage_error("pair-api: expected generate, probe, accept, or status");
    };
    if endpoint == "constants" {
        return run_pair_api_constants_plan(&argv[1..]);
    }
    let Some(endpoint) = PairApiEndpoint::parse(endpoint) else {
        return pair_api_usage_error("pair-api: expected generate, probe, accept, or status");
    };

    let mut plan_json = false;
    let mut node = None::<String>;
    let mut oracle = None::<String>;
    let mut port = None::<u16>;
    let mut base_url = None::<String>;
    let mut federation_token = None::<String>;
    let mut pubkey = None::<String>;
    let mut now_ms = None::<u64>;
    let mut code = None::<String>;
    let mut expires_sec = None::<u64>;
    let mut ttl_ms = None::<u64>;
    let mut seed_codes = Vec::<SeedPairCode>::new();
    let mut remote_node = None::<String>;
    let mut remote_url = None::<String>;
    let mut seed_accepted = None::<PairAcceptInput>;

    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --node value");
                };
                node = Some(value.to_owned());
                index += 1;
            }
            "--oracle" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --oracle value");
                };
                oracle = Some(value.to_owned());
                index += 1;
            }
            "--port" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --port value");
                };
                match parse_u16_arg(value, "pair-api: --port") {
                    Ok(parsed) => port = Some(parsed),
                    Err(message) => return pair_api_usage_error(&message),
                }
                index += 1;
            }
            "--base-url" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --base-url value");
                };
                base_url = Some(value.to_owned());
                index += 1;
            }
            "--federation-token" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --federation-token value");
                };
                federation_token = Some(value.to_owned());
                index += 1;
            }
            "--pubkey" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --pubkey value");
                };
                pubkey = Some(value.to_owned());
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --now value");
                };
                match parse_u64_arg(value, "pair-api: --now") {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(message) => return pair_api_usage_error(&message),
                }
                index += 1;
            }
            "--code" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --code value");
                };
                code = Some(value.to_owned());
                index += 1;
            }
            "--expires-sec" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --expires-sec value");
                };
                match parse_u64_arg(value, "pair-api: --expires-sec") {
                    Ok(parsed) => expires_sec = Some(parsed),
                    Err(message) => return pair_api_usage_error(&message),
                }
                index += 1;
            }
            "--ttl-ms" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --ttl-ms value");
                };
                match parse_u64_arg(value, "pair-api: --ttl-ms") {
                    Ok(parsed) => ttl_ms = Some(parsed),
                    Err(message) => return pair_api_usage_error(&message),
                }
                index += 1;
            }
            "--seed-code" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --seed-code value");
                };
                match parse_seed_pair_code(value) {
                    Ok(seed) => seed_codes.push(seed),
                    Err(message) => return pair_api_usage_error(&message),
                }
                index += 1;
            }
            "--remote-node" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --remote-node value");
                };
                remote_node = Some(value.to_owned());
                index += 1;
            }
            "--remote-url" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --remote-url value");
                };
                remote_url = Some(value.to_owned());
                index += 1;
            }
            "--seed-accepted" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_usage_error("pair-api: missing --seed-accepted value");
                };
                match parse_seed_accepted(value) {
                    Ok(input) => seed_accepted = Some(input),
                    Err(message) => return pair_api_usage_error(&message),
                }
                index += 1;
            }
            arg => return pair_api_usage_error(&format!("pair-api: unknown argument {arg}")),
        }
        index += 1;
    }

    let Some(code) = code else {
        return pair_api_usage_error("pair-api: missing --code value");
    };
    let Some(now_ms) = now_ms else {
        return pair_api_usage_error("pair-api: missing --now value");
    };
    let config = match build_pair_api_config(node, oracle, port, base_url, federation_token, pubkey)
    {
        Ok(config) => config,
        Err(message) => return pair_api_usage_error(&message),
    };
    let mut store = PairCodeStore::default();
    for seed in seed_codes {
        let _ = store.register_at(&seed.code, seed.ttl_ms, seed.created_at_ms);
    }
    if let Some(input) = seed_accepted.clone() {
        let _ = pair_api_accept_plan(&mut store, &config, &code, Some(input), now_ms);
    }

    CliOutput {
        code: 0,
        stdout: match endpoint {
            PairApiEndpoint::Generate => {
                let result =
                    pair_api_generate_plan(&mut store, &config, &code, expires_sec, ttl_ms, now_ms);
                if plan_json {
                    render_pair_api_generate_json(&result)
                } else {
                    format!(
                        "pair-api generate status={} code={}\n",
                        result.status, result.code
                    )
                }
            }
            PairApiEndpoint::Probe => {
                let result = pair_api_probe_plan(&store, &config, &code, now_ms);
                if plan_json {
                    render_pair_api_probe_json(&result)
                } else {
                    format!("pair-api probe status={} ok={}\n", result.status, result.ok)
                }
            }
            PairApiEndpoint::Accept => {
                let input = remote_node.map(|node| PairAcceptInput {
                    node,
                    url: remote_url,
                });
                let result = pair_api_accept_plan(&mut store, &config, &code, input, now_ms);
                if plan_json {
                    render_pair_api_accept_json(&result)
                } else {
                    format!(
                        "pair-api accept status={} ok={}\n",
                        result.status, result.ok
                    )
                }
            }
            PairApiEndpoint::Status => {
                let result = pair_api_status_plan(&store, &code, now_ms);
                if plan_json {
                    render_pair_api_status_json(&result)
                } else {
                    format!(
                        "pair-api status status={} ok={}\n",
                        result.status, result.ok
                    )
                }
            }
        },
        stderr: String::new(),
    }
}

#[derive(Debug, Clone, Copy)]
enum PairApiEndpoint {
    Generate,
    Probe,
    Accept,
    Status,
}

impl PairApiEndpoint {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "generate" => Some(Self::Generate),
            "probe" => Some(Self::Probe),
            "accept" => Some(Self::Accept),
            "status" => Some(Self::Status),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct SeedPairCode {
    code: String,
    ttl_ms: u64,
    created_at_ms: u64,
}

fn parse_seed_pair_code(value: &str) -> Result<SeedPairCode, String> {
    let mut parts = value.split(':');
    let Some(code) = parts.next().filter(|part| !part.is_empty()) else {
        return Err("pair-api: --seed-code must be code:ttl_ms:created_at_ms".to_owned());
    };
    let Some(ttl_ms) = parts.next() else {
        return Err("pair-api: --seed-code must be code:ttl_ms:created_at_ms".to_owned());
    };
    let Some(created_at_ms) = parts.next() else {
        return Err("pair-api: --seed-code must be code:ttl_ms:created_at_ms".to_owned());
    };
    if parts.next().is_some() {
        return Err("pair-api: --seed-code must be code:ttl_ms:created_at_ms".to_owned());
    }
    Ok(SeedPairCode {
        code: code.to_owned(),
        ttl_ms: parse_u64_arg(ttl_ms, "pair-api: --seed-code ttl_ms")?,
        created_at_ms: parse_u64_arg(created_at_ms, "pair-api: --seed-code created_at_ms")?,
    })
}
