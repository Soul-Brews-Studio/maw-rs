fn parse_seed_accepted(value: &str) -> Result<PairAcceptInput, String> {
    let Some((node, url)) = value.split_once('=') else {
        return Err("pair-api: --seed-accepted must be node=url".to_owned());
    };
    if node.is_empty() || url.is_empty() {
        return Err("pair-api: --seed-accepted must be node=url".to_owned());
    }
    Ok(PairAcceptInput {
        node: node.to_owned(),
        url: Some(url.to_owned()),
    })
}

fn build_pair_api_config(
    node: Option<String>,
    oracle: Option<String>,
    port: Option<u16>,
    base_url: Option<String>,
    federation_token: Option<String>,
    pubkey: Option<String>,
) -> Result<PairApiConfig, String> {
    Ok(PairApiConfig {
        node: node.ok_or_else(|| "pair-api: missing --node value".to_owned())?,
        oracle: oracle.ok_or_else(|| "pair-api: missing --oracle value".to_owned())?,
        port: port.ok_or_else(|| "pair-api: missing --port value".to_owned())?,
        base_url: base_url.ok_or_else(|| "pair-api: missing --base-url value".to_owned())?,
        federation_token: federation_token
            .ok_or_else(|| "pair-api: missing --federation-token value".to_owned())?,
        pubkey: pubkey.ok_or_else(|| "pair-api: missing --pubkey value".to_owned())?,
    })
}

fn render_pair_api_generate_json(result: &PairApiGenerateResult) -> String {
    format!(
        "{{\"command\":\"pair-api\",\"endpoint\":\"generate\",\"status\":{},\"ok\":{},\"code\":{},\"expiresAt\":{},\"ttlMs\":{},\"node\":{},\"port\":{},\"federationToken\":null}}\n",
        result.status,
        result.ok,
        json_string(&result.code),
        result.expires_at,
        result.ttl_ms,
        json_string(&result.node),
        result.port
    )
}

fn render_pair_api_probe_json(result: &PairApiProbeResult) -> String {
    format!(
        "{{\"command\":\"pair-api\",\"endpoint\":\"probe\",\"status\":{},\"ok\":{},\"error\":{},\"node\":{}}}\n",
        result.status,
        result.ok,
        json_optional_string(result.error.as_deref()),
        json_optional_string(result.node.as_deref())
    )
}

fn render_pair_api_accept_json(result: &PairApiAcceptResult) -> String {
    format!(
        "{{\"command\":\"pair-api\",\"endpoint\":\"accept\",\"status\":{},\"ok\":{},\"error\":{},\"node\":{},\"url\":{},\"federationToken\":null}}\n",
        result.status,
        result.ok,
        json_optional_string(result.error.as_deref()),
        json_optional_string(result.node.as_deref()),
        json_optional_string(result.url.as_deref())
    )
}

fn render_pair_api_status_json(result: &PairApiStatusResult) -> String {
    format!(
        "{{\"command\":\"pair-api\",\"endpoint\":\"status\",\"status\":{},\"ok\":{},\"error\":{},\"consumed\":{},\"remoteNode\":{},\"remoteUrl\":{}}}\n",
        result.status,
        result.ok,
        json_optional_string(result.error.as_deref()),
        json_optional_bool(result.consumed),
        json_optional_string(result.remote_node.as_deref()),
        json_optional_string(result.remote_url.as_deref())
    )
}

fn json_optional_bool(value: Option<bool>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn parse_u64_arg(value: &str, name: &str) -> Result<u64, String> {
    value
        .parse::<u64>()
        .map_err(|_| format!("{name} must be a non-negative integer"))
}

fn parse_u16_arg(value: &str, name: &str) -> Result<u16, String> {
    value
        .parse::<u16>()
        .map_err(|_| format!("{name} must be a u16"))
}

fn pair_api_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_api_usage()),
    }
}

fn pair_api_usage() -> &'static str {
    "usage: maw-rs pair-api <generate|probe|accept|status> --code <code> --node <node> --oracle <oracle> --port <port> --base-url <url> --federation-token <token> --pubkey <pubkey> --now <ms> [--expires-sec <sec>|--ttl-ms <ms>] [--seed-code <code:ttl_ms:created_at_ms>]... [--remote-node <node> --remote-url <url>] [--seed-accepted <node=url>] [--plan-json]
       maw-rs pair-api constants [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_pair_api_auto_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_pair_api_auto_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut node = None::<String>;
    let mut oracle = None::<String>;
    let mut port = None::<u16>;
    let mut base_url = None::<String>;
    let mut federation_token = None::<String>;
    let mut pubkey = None::<String>;
    let mut now_ms = None::<u64>;
    let mut remote_node = None::<String>;
    let mut remote_oracle = None::<String>;
    let mut remote_url = None::<String>;
    let mut zid = None::<String>;
    let mut remote_pubkey = None::<String>;
    let mut hellos = RecentHelloStore::default();
    let mut add_outcome = AutoPairAddOutcome::Ok { one_way: false };

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --node value");
                };
                node = Some(value.to_owned());
                index += 1;
            }
            "--oracle" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --oracle value");
                };
                oracle = Some(value.to_owned());
                index += 1;
            }
            "--port" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --port value");
                };
                match parse_u16_arg(value, "pair-api-auto: --port") {
                    Ok(parsed) => port = Some(parsed),
                    Err(message) => return pair_api_auto_usage_error(&message),
                }
                index += 1;
            }
            "--base-url" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --base-url value");
                };
                base_url = Some(value.to_owned());
                index += 1;
            }
            "--federation-token" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error(
                        "pair-api-auto: missing --federation-token value",
                    );
                };
                federation_token = Some(value.to_owned());
                index += 1;
            }
            "--pubkey" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --pubkey value");
                };
                pubkey = Some(value.to_owned());
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --now value");
                };
                match parse_u64_arg(value, "pair-api-auto: --now") {
                    Ok(parsed) => now_ms = Some(parsed),
                    Err(message) => return pair_api_auto_usage_error(&message),
                }
                index += 1;
            }
            "--remote-node" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --remote-node value");
                };
                remote_node = Some(value.to_owned());
                index += 1;
            }
            "--remote-oracle" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error(
                        "pair-api-auto: missing --remote-oracle value",
                    );
                };
                remote_oracle = Some(value.to_owned());
                index += 1;
            }
            "--remote-url" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --remote-url value");
                };
                remote_url = Some(value.to_owned());
                index += 1;
            }
            "--zid" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --zid value");
                };
                zid = Some(value.to_owned());
                index += 1;
            }
            "--remote-pubkey" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error(
                        "pair-api-auto: missing --remote-pubkey value",
                    );
                };
                remote_pubkey = Some(value.to_owned());
                index += 1;
            }
            "--hello" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --hello value");
                };
                match parse_recent_hello(value) {
                    Ok((zid, seen_at)) => hellos.record(&zid, seen_at),
                    Err(message) => return pair_api_auto_usage_error(&message),
                }
                index += 1;
            }
            "--add-ok" => add_outcome = AutoPairAddOutcome::Ok { one_way: false },
            "--add-one-way" => add_outcome = AutoPairAddOutcome::Ok { one_way: true },
            "--add-pubkey-mismatch" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error(
                        "pair-api-auto: missing --add-pubkey-mismatch value",
                    );
                };
                add_outcome = AutoPairAddOutcome::PubkeyMismatch(value.to_owned());
                index += 1;
            }
            "--add-error" => {
                let Some(value) = argv.get(index + 1) else {
                    return pair_api_auto_usage_error("pair-api-auto: missing --add-error value");
                };
                add_outcome = AutoPairAddOutcome::Error(value.to_owned());
                index += 1;
            }
            arg => {
                return pair_api_auto_usage_error(&format!("pair-api-auto: unknown argument {arg}"))
            }
        }
        index += 1;
    }

    let Some(now_ms) = now_ms else {
        return pair_api_auto_usage_error("pair-api-auto: missing --now value");
    };
    let config = match build_pair_api_config(node, oracle, port, base_url, federation_token, pubkey)
    {
        Ok(config) => config,
        Err(message) => {
            return pair_api_auto_usage_error(&message.replace("pair-api", "pair-api-auto"))
        }
    };
    let input = match (remote_node, remote_url, zid) {
        (Some(node), Some(url), Some(zid)) => Some(AutoPairInput {
            node,
            oracle: remote_oracle,
            url,
            zid,
            pubkey: remote_pubkey,
        }),
        _ => None,
    };
    let result = pair_api_auto_plan(&config, &hellos, input, add_outcome, now_ms);

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_api_auto_json(&result)
        } else {
            format!("pair-api-auto status={} ok={}\n", result.status, result.ok)
        },
        stderr: String::new(),
    }
}

fn parse_recent_hello(value: &str) -> Result<(String, u64), String> {
    let Some((zid, seen_at)) = value.split_once(':') else {
        return Err("pair-api-auto: --hello must be zid:seen_at_ms".to_owned());
    };
    if zid.is_empty() || seen_at.is_empty() {
        return Err("pair-api-auto: --hello must be zid:seen_at_ms".to_owned());
    }
    Ok((
        zid.to_owned(),
        parse_u64_arg(seen_at, "pair-api-auto: --hello seen_at_ms")?,
    ))
}

fn render_pair_api_auto_json(result: &PairApiAutoResult) -> String {
    format!(
        "{{\"command\":\"pair-api-auto\",\"status\":{},\"ok\":{},\"error\":{},\"node\":{},\"oracle\":{},\"url\":{},\"pubkey\":{},\"proof\":{},\"federationToken\":null,\"oneWay\":{},\"add\":{},\"markSymmetricCheck\":{}}}\n",
        result.status,
        result.ok,
        json_optional_string(result.error.as_deref()),
        json_optional_string(result.node.as_deref()),
        json_optional_string(result.oracle.as_deref()),
        json_optional_string(result.url.as_deref()),
        json_optional_string(result.pubkey.as_deref()),
        json_optional_string(result.proof.as_deref()),
        json_optional_bool(result.one_way),
        render_pair_api_auto_add_json(result),
        result.mark_symmetric_check
    )
}

fn render_pair_api_auto_add_json(result: &PairApiAutoResult) -> String {
    if result.add_alias.is_none()
        && result.add_url.is_none()
        && result.add_node.is_none()
        && result.add_pubkey.is_none()
        && result.add_identity_oracle.is_none()
        && result.add_identity_node.is_none()
    {
        return "null".to_owned();
    }
    format!(
        "{{\"alias\":{},\"url\":{},\"node\":{},\"pubkey\":{},\"identityOracle\":{},\"identityNode\":{}}}",
        json_optional_string(result.add_alias.as_deref()),
        json_optional_string(result.add_url.as_deref()),
        json_optional_string(result.add_node.as_deref()),
        json_optional_string(result.add_pubkey.as_deref()),
        json_optional_string(result.add_identity_oracle.as_deref()),
        json_optional_string(result.add_identity_node.as_deref())
    )
}

fn run_pair_api_auto_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => {
                return pair_api_auto_constants_usage_error(&format!(
                    "pair-api-auto constants: unknown arg {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_pair_api_auto_constants_json()
        } else {
            "pair-api-auto constants required=remote-node,remote-url,zid add=ok,one-way,pubkey-mismatch,error redacted=federationToken\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_pair_api_auto_constants_json() -> String {
    r#"{"command":"pair-api-auto","action":"constants","requiredInput":["remote-node","remote-url","zid"],"helloShape":"zid:seen_at_ms","addOutcomes":["ok","one-way","pubkey-mismatch","error"],"errorCodes":["missing_fields","no_recent_hello","pubkey_mismatch","add_error"],"httpStatuses":{"ok":200,"badRequest":400,"forbidden":403,"conflict":409},"redactedFields":["federationToken"],"markSymmetricCheckOnSuccess":true}
"#
    .to_owned()
}

fn pair_api_auto_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_api_auto_constants_usage()),
    }
}

fn pair_api_auto_constants_usage() -> &'static str {
    "usage: maw-rs pair-api-auto constants [--plan-json]"
}

fn pair_api_auto_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", pair_api_auto_usage()),
    }
}

fn pair_api_auto_usage() -> &'static str {
    "usage: maw-rs pair-api-auto --node <node> --oracle <oracle> --port <port> --base-url <url> --federation-token <token> --pubkey <pubkey> --now <ms> [--remote-node <node> --remote-url <url> --zid <zid>] [--remote-oracle <oracle>] [--remote-pubkey <pubkey>] [--hello <zid:seen_at_ms>]... [--add-ok|--add-one-way|--add-pubkey-mismatch <message>|--add-error <message>] [--plan-json]
       maw-rs pair-api-auto constants [--plan-json]"
}

