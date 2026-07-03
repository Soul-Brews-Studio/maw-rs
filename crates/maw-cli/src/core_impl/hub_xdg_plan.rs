struct AuthFromSignPayloadRender<'a> {
    legacy: bool,
    from: &'a str,
    timestamp: Option<i64>,
    signed_at: Option<&'a str>,
    method: &'a str,
    path: &'a str,
    body_hash: &'a str,
    payload: &'a str,
}

fn render_auth_from_sign_payload_json(args: &AuthFromSignPayloadRender<'_>) -> String {
    let version = if args.legacy { "legacy" } else { "v3" };
    let timestamp = args
        .timestamp
        .map_or_else(|| "null".to_owned(), |timestamp| timestamp.to_string());
    let signed_at = args
        .signed_at
        .map_or_else(|| "null".to_owned(), json_string);
    format!(
        "{{\"command\":\"auth\",\"kind\":\"from-sign-payload\",\"version\":{},\"from\":{},\"timestamp\":{timestamp},\"signedAt\":{signed_at},\"method\":{},\"path\":{},\"bodyHash\":{},\"payload\":{}}}\n",
        json_string(version),
        json_string(args.from),
        json_string(args.method),
        json_string(args.path),
        json_string(args.body_hash),
        json_string(args.payload)
    )
}

fn render_auth_hmac_verify_json(
    payload: &str,
    signature: &str,
    valid: bool,
    reason: &str,
) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"hmac-verify\",\"payloadLength\":{},\"signatureLength\":{},\"valid\":{valid},\"reason\":{}}}\n",
        payload.len(),
        signature.len(),
        json_string(reason)
    )
}

fn render_auth_hmac_sign_json(payload: &str, signature: &str) -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"hmac-sign\",\"payloadLength\":{},\"signature\":{}}}\n",
        payload.len(),
        json_string(signature)
    )
}

fn render_auth_constants_json() -> String {
    format!(
        "{{\"command\":\"auth\",\"kind\":\"constants\",\"defaultOracle\":{},\"windowSec\":{WINDOW_SEC}}}\n",
        json_string(DEFAULT_ORACLE)
    )
}

fn render_auth_decision_fields(decision: &FromVerifyDecision) -> Vec<String> {
    let mut fields = vec![format!("\"kind\":{}", json_string(decision.kind()))];
    match decision {
        FromVerifyDecision::AcceptLegacy { reason }
        | FromVerifyDecision::RefuseMalformed { reason } => {
            fields.push(format!("\"reason\":{}", json_string(reason)));
        }
        FromVerifyDecision::AcceptTofuRecord { reason, from }
        | FromVerifyDecision::AcceptVerified { reason, from }
        | FromVerifyDecision::RefuseMismatch { reason, from } => {
            fields.push(format!("\"reason\":{}", json_string(reason)));
            fields.push(format!("\"from\":{}", json_string(from)));
        }
        FromVerifyDecision::RefuseUnsigned { reason, from } => {
            fields.push(format!("\"reason\":{}", json_string(reason)));
            if let Some(from) = from {
                fields.push(format!("\"from\":{}", json_string(from)));
            }
        }
        FromVerifyDecision::RefuseSkew {
            reason,
            from,
            delta,
        } => {
            fields.push(format!("\"reason\":{}", json_string(reason)));
            fields.push(format!("\"from\":{}", json_string(from)));
            fields.push(format!("\"delta\":{delta}"));
        }
    }
    fields
}

fn auth_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs auth sign-v1 --token <token> --now <sec> [--method <method>] [--path <path>] [--body-hash <sha256>] [--plan-json]
       maw-rs auth sign-headers --token <token> --now <sec> [--method <method>] [--path <path>] [--body <body>] [--plan-json]
       maw-rs auth verify-v1 --token <token> --signature <hex> --signed-at <sec> --now <sec> [--method <method>] [--path <path>] [--body-hash <sha256>] [--plan-json]
       maw-rs auth verify-legacy-from --from <oracle:node> --signed-at <iso> --signature <hex> --now <sec> [--cached-pubkey <key>] [--method <method>] [--path <path>] [--body <body>] [--plan-json]
       maw-rs auth verify-v3-from --from <oracle:node> --timestamp <sec> --signature-v3 <hex> --now <sec> [--cached-pubkey <key>] [--method <method>] [--path <path>] [--body <body>] [--plan-json]
       maw-rs auth from-sign-payload --from <oracle:node> (--timestamp <sec>|--legacy --signed-at <iso>) [--method <method>] [--path <path>] [--body-hash <sha256>] [--plan-json]
       maw-rs auth hmac-sign --secret <secret> --payload <payload> [--plan-json]
       maw-rs auth hmac-verify --secret <secret> --payload <payload> --signature <hex> [--plan-json]
       maw-rs auth constants [--plan-json]
       maw-rs auth sign-v3 --peer-key <key> --from <oracle:node> [--method <method>] [--path <path>] [--now <sec>] [--body <body>] [--plan-json]\n       maw-rs auth verify-request [--method <method>] [--path <path>] [--now <sec>] [--body <body>] [--cached-pubkey <key>] [--peer-ip <ip>] [--workspace-key-env <name>] [--header <key=value>]... [--plan-json]\n       maw-rs auth loopback --address <address> [--plan-json]\n       maw-rs auth from-address --node <node> [--oracle <oracle>] [--plan-json]\n       maw-rs auth hash-body [--body <body>] [--plan-json]\n"
        ),
    }
}

fn run_hub_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_hub_constants_plan(&argv[1..]);
    }

    let action = match parse_hub_plan_args(argv) {
        Ok(action) => action,
        Err(message) => return hub_usage_error(&message),
    };
    match action {
        HubPlanAction::ValidateWorkspace {
            plan_json,
            id,
            hub_url,
            token,
            shared_agents,
        } => {
            let raw = serde_json::json!({
                "id": id,
                "hubUrl": hub_url,
                "token": token,
                "sharedAgents": shared_agents,
            });
            let validation = validate_workspace_config(&raw);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_hub_validate_json(&raw, &validation)
                } else if validation.ok() {
                    "ok\n".to_owned()
                } else {
                    format!("invalid: {}\n", validation.reason().unwrap_or("unknown"))
                },
                stderr: String::new(),
            }
        }
        HubPlanAction::LoadWorkspaces {
            plan_json,
            config_dir,
        } => match load_workspace_configs(&config_dir) {
            Ok(report) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_hub_load_json(&report.configs, &report.warnings)
                } else {
                    format!(
                        "configs={} warnings={}\n",
                        report.configs.len(),
                        report.warnings.len()
                    )
                },
                stderr: String::new(),
            },
            Err(error) => CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("hub load-workspaces: {error}\n"),
            },
        },
    }
}

enum HubPlanAction {
    ValidateWorkspace {
        plan_json: bool,
        id: String,
        hub_url: String,
        token: String,
        shared_agents: Vec<String>,
    },
    LoadWorkspaces {
        plan_json: bool,
        config_dir: String,
    },
}

fn parse_hub_plan_args(argv: &[String]) -> Result<HubPlanAction, String> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err("hub: expected validate-workspace or load-workspaces".to_owned());
    };
    match kind {
        "validate-workspace" => parse_hub_validate_args(&argv[1..]),
        "load-workspaces" => parse_hub_load_args(&argv[1..]),
        other => Err(format!("hub: unknown subcommand {other}")),
    }
}

fn parse_hub_validate_args(argv: &[String]) -> Result<HubPlanAction, String> {
    let mut plan_json = false;
    let mut id = String::new();
    let mut hub_url = String::new();
    let mut token = String::new();
    let mut shared_agents = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--id" => {
                id = take_hub_value(argv, index, "--id")?;
                index += 1;
            }
            "--hub-url" => {
                hub_url = take_hub_value(argv, index, "--hub-url")?;
                index += 1;
            }
            "--token" => {
                token = take_hub_value(argv, index, "--token")?;
                index += 1;
            }
            "--shared-agent" => {
                shared_agents.push(take_hub_value(argv, index, "--shared-agent")?);
                index += 1;
            }
            other => return Err(format!("hub validate-workspace: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(HubPlanAction::ValidateWorkspace {
        plan_json,
        id,
        hub_url,
        token,
        shared_agents,
    })
}

fn parse_hub_load_args(argv: &[String]) -> Result<HubPlanAction, String> {
    let mut plan_json = false;
    let mut config_dir = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--config-dir" => {
                config_dir = Some(take_hub_value(argv, index, "--config-dir")?);
                index += 1;
            }
            other => return Err(format!("hub load-workspaces: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(HubPlanAction::LoadWorkspaces {
        plan_json,
        config_dir: config_dir
            .ok_or_else(|| "hub load-workspaces: --config-dir is required".to_owned())?,
    })
}

fn take_hub_value(argv: &[String], index: usize, name: &str) -> Result<String, String> {
    argv.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("hub: missing {name} value"))
}

fn render_hub_validate_json(
    raw: &serde_json::Value,
    validation: &WorkspaceConfigValidation,
) -> String {
    let reason = validation.reason().map_or("null".to_owned(), json_string);
    format!(
        "{{\"command\":\"hub\",\"kind\":\"validate-workspace\",\"input\":{},\"ok\":{},\"reason\":{reason}}}\n",
        raw,
        validation.ok()
    )
}

fn render_hub_load_json(configs: &[WorkspaceConfig], warnings: &[String]) -> String {
    let configs = configs
        .iter()
        .map(render_workspace_config_json)
        .collect::<Vec<_>>()
        .join(",");
    let warnings = json_string_array(warnings);
    format!(
        "{{\"command\":\"hub\",\"kind\":\"load-workspaces\",\"configs\":[{configs}],\"warnings\":{warnings}}}\n"
    )
}

fn render_workspace_config_json(config: &WorkspaceConfig) -> String {
    format!(
        "{{\"id\":{},\"hubUrl\":{},\"token\":{},\"sharedAgents\":{}}}",
        json_string(&config.id),
        json_string(&config.hub_url),
        json_string(&config.token),
        json_string_array(&config.shared_agents)
    )
}

fn run_hub_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => return hub_constants_usage_error(&format!("hub constants: unknown arg {arg}")),
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_hub_constants_json()
        } else {
            format!(
                "hub constants heartbeat-ms={HEARTBEAT_MS} reconnect-base-ms={RECONNECT_BASE_MS} reconnect-max-ms={RECONNECT_MAX_MS}\n"
            )
        },
        stderr: String::new(),
    }
}

fn render_hub_constants_json() -> String {
    format!(
        r#"{{"command":"hub","action":"constants","actions":["validate-workspace","load-workspaces"],"requiredFields":["id","hubUrl","token","sharedAgents"],"validProtocols":["ws","wss"],"workspaceDirName":"workspaces","fileExtension":"json","heartbeatMs":{HEARTBEAT_MS},"reconnectBaseMs":{RECONNECT_BASE_MS},"reconnectMaxMs":{RECONNECT_MAX_MS},"validationReasons":["not an object","missing/empty id","missing/empty hubUrl","missing/empty token","sharedAgents must be array","hubUrl must be ws:|wss: (got <protocol>:)","hubUrl not a valid URL"],"warningPrefixes":["[hub] failed to parse workspace config","[hub] invalid workspace config"]}}
"#
    )
}

fn hub_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", hub_constants_usage()),
    }
}

fn hub_constants_usage() -> &'static str {
    "usage: maw-rs hub constants [--plan-json]"
}

fn hub_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs hub validate-workspace [--id <id>] [--hub-url <ws-url>] [--token <token>] [--shared-agent <agent>]... [--plan-json]\n       maw-rs hub load-workspaces --config-dir <dir> [--plan-json]\n       maw-rs hub constants [--plan-json]\n"
        ),
    }
}

fn run_xdg_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_xdg_constants_plan(&argv[1..]);
    }

    let action = match parse_xdg_plan_args(argv) {
        Ok(action) => action,
        Err(message) => return xdg_usage_error(&message),
    };
    match action {
        XdgPlanAction::Paths { plan_json, env } => {
            let paths = XdgResolvedPaths::from_env(&env);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_xdg_paths_json(&paths)
                } else {
                    format!("{}\n", paths.runtime_home)
                },
                stderr: String::new(),
            }
        }
        XdgPlanAction::CorePaths { plan_json, env } => match ensure_maw_core_paths(&env) {
            Ok(paths) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_xdg_core_paths_json(&paths)
                } else {
                    format!("{}\n", paths.runtime_home.display())
                },
                stderr: String::new(),
            },
            Err(error) => CliOutput {
                code: 1,
                stdout: String::new(),
                stderr: format!("xdg core-paths: {error}\n"),
            },
        },
        XdgPlanAction::ValidateInstance { plan_json, name } => {
            let valid = is_valid_instance_name(&name);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!(
                        "{{\"command\":\"xdg\",\"kind\":\"validate-instance\",\"name\":{},\"valid\":{valid}}}\n",
                        json_string(&name)
                    )
                } else {
                    format!("{valid}\n")
                },
                stderr: String::new(),
            }
        }
    }
}

