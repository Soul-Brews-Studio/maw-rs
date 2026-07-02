fn render_consent_pending_status_plan_json(
    id: &str,
    updated: bool,
    request: Option<&PendingRequest>,
    entries: &[PendingRequest],
) -> String {
    format!(
        "{{\"command\":\"consent-pending-status\",\"id\":{},\"updated\":{updated},\"request\":{},\"entries\":{}}}\n",
        json_string(id),
        render_pending_request_json(request),
        render_pending_requests_json(entries)
    )
}

fn consent_pending_status_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", consent_pending_status_usage()),
    }
}

fn consent_pending_status_usage() -> &'static str {
    "usage: maw-rs consent-pending-status [--request <id=...,from=...,to=...,action=...,summary=...,pin_hash=...,created_at=...,expires_at=...,status=...>]... --set-status <id:pending|approved|rejected|expired> [--plan-json]"
}

fn run_recent_hello_plan(argv: &[String]) -> CliOutput {
    if argv.first().is_some_and(|arg| arg == "constants") {
        return run_recent_hello_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut zid = None::<String>;
    let mut now_ms = None::<u64>;
    let mut store = RecentHelloStore::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--hello" => {
                let Some(value) = argv.get(index + 1) else {
                    return recent_hello_usage_error("recent-hello: missing --hello value");
                };
                let Ok((hello_zid, seen_at)) = parse_recent_hello_arg(value) else {
                    return recent_hello_usage_error("recent-hello: invalid hello timestamp");
                };
                store.record(&hello_zid, seen_at);
                index += 1;
            }
            "--zid" => {
                let Some(value) = argv.get(index + 1) else {
                    return recent_hello_usage_error("recent-hello: missing --zid value");
                };
                if value.is_empty() {
                    return recent_hello_usage_error("recent-hello: missing --zid value");
                }
                zid = Some(value.to_owned());
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return recent_hello_usage_error("recent-hello: missing --now value");
                };
                match value.parse::<u64>() {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(_) => return recent_hello_usage_error("recent-hello: invalid --now value"),
                }
                index += 1;
            }
            arg => {
                return recent_hello_usage_error(&format!("recent-hello: unknown argument {arg}"))
            }
        }
        index += 1;
    }

    let Some(zid) = zid else {
        return recent_hello_usage_error("recent-hello: missing --zid value");
    };
    let Some(now_ms) = now_ms else {
        return recent_hello_usage_error("recent-hello: missing --now value");
    };
    let recent = store.is_recent(&zid, now_ms);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_recent_hello_plan_json(&zid, now_ms, recent)
        } else {
            format!("recent-hello zid={zid} recent={recent}\n")
        },
        stderr: String::new(),
    }
}

fn run_recent_hello_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return recent_hello_constants_usage_error(&format!(
                    "recent-hello constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_recent_hello_constants_json()
        } else {
            "recent-hello windowMs=60000 threshold='now-minus-seen-at <= windowMs'\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn parse_recent_hello_arg(value: &str) -> Result<(String, u64), String> {
    let Some((zid, seen_at)) = value.split_once(':') else {
        return Err("recent-hello: --hello must be zid:seen_at_ms".to_owned());
    };
    if zid.is_empty() {
        return Err("recent-hello: --hello must be zid:seen_at_ms".to_owned());
    }
    let seen_at = seen_at
        .parse::<u64>()
        .map_err(|_| "recent-hello: invalid hello timestamp".to_owned())?;
    Ok((zid.to_owned(), seen_at))
}

fn render_recent_hello_plan_json(zid: &str, now_ms: u64, recent: bool) -> String {
    format!(
        "{{\"command\":\"recent-hello\",\"zid\":{},\"now\":{now_ms},\"windowMs\":60000,\"recent\":{recent}}}\n",
        json_string(zid)
    )
}

fn render_recent_hello_constants_json() -> String {
    "{\"command\":\"recent-hello\",\"kind\":\"constants\",\"windowMs\":60000,\"threshold\":\"now-minus-seen-at <= windowMs\"}\n".to_owned()
}

fn recent_hello_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", recent_hello_usage()),
    }
}

fn recent_hello_usage() -> &'static str {
    "usage: maw-rs recent-hello [--hello <zid:seen_at_ms>]... --zid <zid> --now <ms> [--plan-json]\n       maw-rs recent-hello constants [--plan-json]"
}

fn recent_hello_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", recent_hello_constants_usage()),
    }
}

fn recent_hello_constants_usage() -> &'static str {
    "usage: maw-rs recent-hello constants [--plan-json]"
}

fn run_pair_code_plan(argv: &[String]) -> CliOutput {
    if argv.first().is_some_and(|arg| arg == "constants") {
        return run_pair_code_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut code = None::<String>;
    let mut bytes = None::<Vec<u8>>;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--code" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_code_usage_error("pair-code: missing --code value");
                };
                code = Some(value.to_owned());
                index += 1;
            }
            "--bytes" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_code_usage_error("pair-code: missing --bytes value");
                };
                match parse_pair_code_bytes(value) {
                    Ok(parsed) => bytes = Some(parsed),
                    Err(message) => return pair_code_usage_error(&message),
                }
                index += 1;
            }
            arg => return pair_code_usage_error(&format!("pair-code: unknown argument {arg}")),
        }
        index += 1;
    }

    if code.is_some() && bytes.is_some() {
        return pair_code_usage_error("pair-code: expected exactly one of --code or --bytes");
    }
    let raw_code = if let Some(code) = code {
        code
    } else if let Some(bytes) = bytes {
        generate_pair_code_from_bytes(&bytes)
    } else {
        return pair_code_usage_error("pair-code: expected --code or --bytes");
    };
    let normalized = normalize_pair_code(&raw_code);
    let pretty = pretty_pair_code(&normalized);
    let redacted = redact_pair_code(&normalized);
    let valid = is_valid_pair_code_shape(&normalized);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_code_plan_json(&normalized, &pretty, &redacted, valid)
        } else {
            render_pair_code_plan_text(&pretty, &redacted, valid)
        },
        stderr: String::new(),
    }
}

fn run_pair_code_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return pair_code_constants_usage_error(&format!(
                    "pair-code constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_code_constants_plan_json()
        } else {
            format!(
                "pair-code alphabet={PAIR_CODE_ALPHABET} codeLength=6 prettyGroupSize=3 separator=-\n"
            )
        },
        stderr: String::new(),
    }
}

fn parse_pair_code_bytes(value: &str) -> Result<Vec<u8>, String> {
    if value.is_empty() {
        return Err("pair-code: --bytes must use comma-separated u8 values".to_owned());
    }
    value
        .split(',')
        .map(|part| {
            if part.is_empty() {
                return Err("pair-code: --bytes must use comma-separated u8 values".to_owned());
            }
            part.parse::<u8>()
                .map_err(|_| "pair-code: --bytes must use comma-separated u8 values".to_owned())
        })
        .collect()
}

fn render_pair_code_plan_json(
    normalized: &str,
    pretty: &str,
    redacted: &str,
    valid: bool,
) -> String {
    format!(
        "{{\"command\":\"pair-code\",\"normalized\":{},\"pretty\":{},\"redacted\":{},\"valid\":{valid}}}\n",
        json_string(normalized),
        json_string(pretty),
        json_string(redacted)
    )
}

fn render_pair_code_constants_plan_json() -> String {
    format!(
        "{{\"command\":\"pair-code\",\"kind\":\"constants\",\"alphabet\":{},\"codeLength\":6,\"prettyGroupSize\":3,\"separator\":\"-\"}}\n",
        json_string(PAIR_CODE_ALPHABET)
    )
}

fn render_pair_code_plan_text(pretty: &str, redacted: &str, valid: bool) -> String {
    format!("pair-code {pretty} valid={valid} redacted={redacted}\n")
}

fn pair_code_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_code_usage()),
    }
}

fn pair_code_usage() -> &'static str {
    "usage: maw-rs pair-code (--code <code>|--bytes <b0,b1,...>) [--plan-json]\n       maw-rs pair-code constants [--plan-json]"
}

fn pair_code_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_code_constants_usage()),
    }
}

fn pair_code_constants_usage() -> &'static str {
    "usage: maw-rs pair-code constants [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_pair_code_store_plan(argv: &[String]) -> CliOutput {
    let Some(mode) = argv.first().map(String::as_str) else {
        return pair_code_store_usage_error(
            "pair-code-store: expected register, lookup, or consume",
        );
    };
    if mode == "constants" {
        return run_pair_code_store_constants_plan(&argv[1..]);
    }
    if !matches!(mode, "register" | "lookup" | "consume") {
        return pair_code_store_usage_error(
            "pair-code-store: expected register, lookup, or consume",
        );
    }

    let mut plan_json = false;
    let mut code = None::<String>;
    let mut now_ms = None::<u64>;
    let mut ttl_ms = None::<u64>;
    let mut seed_codes = Vec::<SeedPairCode>::new();

    let mut index = 1;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--code" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_code_store_usage_error("pair-code-store: missing --code value");
                };
                if value.is_empty() {
                    return pair_code_store_usage_error("pair-code-store: missing --code value");
                }
                code = Some(value.to_owned());
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_code_store_usage_error("pair-code-store: missing --now value");
                };
                match parse_u64_arg(value, "pair-code-store: --now") {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(message) => return pair_code_store_usage_error(&message),
                }
                index += 1;
            }
            "--ttl-ms" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_code_store_usage_error("pair-code-store: missing --ttl-ms value");
                };
                match parse_u64_arg(value, "pair-code-store: --ttl-ms") {
                    Ok(parsed) => ttl_ms = Some(parsed),
                    Err(message) => return pair_code_store_usage_error(&message),
                }
                index += 1;
            }
            "--seed-code" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_code_store_usage_error(
                        "pair-code-store: missing --seed-code value",
                    );
                };
                match parse_pair_code_store_seed(value) {
                    Ok(seed) => seed_codes.push(seed),
                    Err(message) => return pair_code_store_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return pair_code_store_usage_error(&format!(
                    "pair-code-store: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let Some(code) = code else {
        return pair_code_store_usage_error("pair-code-store: missing --code value");
    };
    let Some(now_ms) = now_ms else {
        return pair_code_store_usage_error("pair-code-store: missing --now value");
    };
    let mut store = PairCodeStore::default();
    for seed in seed_codes {
        let _ = store.register_at(&seed.code, seed.ttl_ms, seed.created_at_ms);
    }

    let normalized = normalize_pair_code(&code);
    let result = if mode == "register" {
        let Some(ttl_ms) = ttl_ms else {
            return pair_code_store_usage_error("pair-code-store: missing --ttl-ms value");
        };
        PairCodeStorePlanResult::Register(store.register_at(&code, ttl_ms, now_ms))
    } else if mode == "lookup" {
        PairCodeStorePlanResult::Lookup(store.lookup_at(&code, now_ms))
    } else {
        PairCodeStorePlanResult::Lookup(store.consume_at(&code, now_ms))
    };

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_code_store_plan_json(mode, &normalized, &result)
        } else {
            format!(
                "pair-code-store mode={mode} code={normalized} state={}\n",
                pair_code_store_result_state(&result)
            )
        },
        stderr: String::new(),
    }
}

