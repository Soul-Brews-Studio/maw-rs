const DISPATCH_300: &[DispatcherEntry] = &[
    DispatcherEntry { command: "xdg", handler: Handler::Sync(run_xdg_plan) },
];

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
