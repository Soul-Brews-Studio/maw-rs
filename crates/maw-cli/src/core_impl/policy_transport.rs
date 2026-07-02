fn render_identity_node_plan_json(host: &str, user: Option<&str>, canonical: &str) -> String {
    let mut input_fields = vec![format!("\"host\":{}", json_string(host))];
    if let Some(user) = user {
        input_fields.push(format!("\"user\":{}", json_string(user)));
    }
    format!(
        "{{\"command\":\"identity\",\"kind\":\"nodeIdentity\",\"input\":{{{}}},\"canonical\":{}}}\n",
        input_fields.join(","),
        json_string(canonical)
    )
}

fn run_policy_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_policy_constants_subcommand_plan(&argv[1..]);
    }

    let (plan_json, action) = match parse_policy_plan_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return policy_usage_error(&message),
    };
    render_policy_plan(action, plan_json)
}

fn parse_policy_plan_args(argv: &[String]) -> Result<(bool, PolicyPlanAction), String> {
    let mut plan_json = false;
    let mut action = PolicyPlanAction::Constants;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--constants" => action = PolicyPlanAction::Constants,
            "--weight" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("policy: missing --weight value".to_owned());
                };
                let Ok(weight) = value.parse::<i32>() else {
                    return Err("policy: --weight must be an integer".to_owned());
                };
                action = PolicyPlanAction::WeightToTier(weight);
                index += 1;
            }
            "--default-active" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("policy: missing --default-active value".to_owned());
                };
                action = PolicyPlanAction::DefaultActiveGroup(value.to_owned());
                index += 1;
            }
            "--includes" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("policy: missing --includes value".to_owned());
                };
                action = match action {
                    PolicyPlanAction::DefaultActiveGroup(key) => {
                        PolicyPlanAction::DefaultActiveIncludes {
                            key,
                            plugin: value.to_owned(),
                        }
                    }
                    _ => {
                        return Err("policy: --includes requires --default-active <key>".to_owned())
                    }
                };
                index += 1;
            }
            arg => return Err(format!("policy: unknown argument {arg}")),
        }
        index += 1;
    }
    Ok((plan_json, action))
}

fn render_policy_plan(action: PolicyPlanAction, plan_json: bool) -> CliOutput {
    match action {
        PolicyPlanAction::Constants => CliOutput {
            code: 0,
            stdout: if plan_json {
                render_policy_constants_json()
            } else {
                format!(
                    "policy constants default-tier={} known-tiers={}\n",
                    DEFAULT_TIER.as_str(),
                    KNOWN_TIERS
                        .iter()
                        .map(|tier| tier.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            },
            stderr: String::new(),
        },
        PolicyPlanAction::WeightToTier(weight) => {
            let tier = weight_to_tier(weight);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!(
                        "{{\"command\":\"policy\",\"kind\":\"weightToTier\",\"weight\":{weight},\"tier\":{}}}\n",
                        json_string(tier.as_str())
                    )
                } else {
                    format!("policy weight {weight}: {}\n", tier.as_str())
                },
                stderr: String::new(),
            }
        }
        PolicyPlanAction::DefaultActiveGroup(key) => {
            let Some(group) = default_active_group(&key) else {
                return policy_usage_error("policy: unknown --default-active key");
            };
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_policy_default_active_json(&key, group)
                } else {
                    format!(
                        "policy default-active {key}: migration={} plugins={}\n",
                        group.migration,
                        group.plugins.join(",")
                    )
                },
                stderr: String::new(),
            }
        }
        PolicyPlanAction::DefaultActiveIncludes { key, plugin } => {
            let Some(group) = default_active_group(&key) else {
                return policy_usage_error("policy: unknown --default-active key");
            };
            let included = (group.includes)(&plugin);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!(
                        "{{\"command\":\"policy\",\"kind\":\"defaultActiveIncludes\",\"key\":{},\"plugin\":{},\"included\":{included}}}\n",
                        json_string(&key),
                        json_string(&plugin)
                    )
                } else {
                    format!("policy default-active {key} includes {plugin}: {included}\n")
                },
                stderr: String::new(),
            }
        }
    }
}

fn run_policy_constants_subcommand_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return policy_constants_usage_error(&format!(
                    "policy constants: unknown argument {other}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_policy_constants_json()
        } else {
            format!(
                "policy constants default-tier={} known-tiers={}\n",
                DEFAULT_TIER.as_str(),
                KNOWN_TIERS
                    .iter()
                    .map(|tier| tier.as_str())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        },
        stderr: String::new(),
    }
}

enum PolicyPlanAction {
    Constants,
    WeightToTier(i32),
    DefaultActiveGroup(String),
    DefaultActiveIncludes { key: String, plugin: String },
}

fn policy_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs policy [--constants|--weight <i32>|--default-active <key> [--includes <plugin>]] [--plan-json]\n       maw-rs policy constants [--plan-json]\n"
        ),
    }
}

fn policy_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs policy constants [--plan-json]\n"),
    }
}

fn render_policy_constants_json() -> String {
    let tiers: Vec<&str> = KNOWN_TIERS.iter().map(|tier| tier.as_str()).collect();
    format!(
        "{{\"command\":\"policy\",\"kind\":\"constants\",\"knownTiers\":{},\"defaultTier\":{},\"weightThresholds\":{{\"core\":\"weight < 10\",\"standard\":\"10 <= weight < 50\",\"extra\":\"weight >= 50\"}},\"defaultActiveKeys\":[\"1500\",\"1514\",\"1523\",\"1524\",\"1531\"],\"defaultActiveMigrations\":[\"defaultActivePlugins1500\",\"defaultActivePlugins1514\",\"defaultActivePlugins1523\",\"defaultActivePlugins1524\",\"defaultActivePlugins1531\"],\"aliases\":[\"policy\",\"plugin-policy\"]}}\n",
        json_str_array(&tiers),
        json_string(DEFAULT_TIER.as_str())
    )
}

fn render_policy_default_active_json(key: &str, group: maw_policy::DefaultActiveGroup) -> String {
    format!(
        "{{\"command\":\"policy\",\"kind\":\"defaultActiveGroup\",\"key\":{},\"migration\":{},\"plugins\":{}}}\n",
        json_string(key),
        json_string(group.migration),
        json_str_array(group.plugins)
    )
}

fn run_transport_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_transport_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut classify = None;
    let mut should_send = false;
    let mut transport_specs = Vec::new();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--classify-error" => {
                let Some(value) = argv.get(index + 1) else {
                    return transport_usage_error("transport: missing --classify-error value");
                };
                classify = Some(value.to_owned());
                index += 1;
            }
            "--classify-empty" => classify = Some(String::new()),
            "--send" => should_send = true,
            "--transport" => {
                let Some(value) = argv.get(index + 1) else {
                    return transport_usage_error("transport: missing --transport value");
                };
                match parse_transport_spec(value) {
                    Ok(transport) => transport_specs.push(transport),
                    Err(message) => return transport_usage_error(&message),
                }
                index += 1;
            }
            arg => return transport_usage_error(&format!("transport: unknown argument {arg}")),
        }
        index += 1;
    }

    if let Some(error) = classify {
        let classified = if error.is_empty() {
            classify_error(None)
        } else {
            classify_error(Some(&error))
        };
        return CliOutput {
            code: 0,
            stdout: if plan_json {
                format!(
                    "{{\"command\":\"transport\",\"kind\":\"classifyError\",\"reason\":{},\"retryable\":{}}}\n",
                    json_string(classified.reason.as_str()),
                    classified.retryable
                )
            } else {
                format!(
                    "transport classify reason={} retryable={}\n",
                    classified.reason.as_str(),
                    classified.retryable
                )
            },
            stderr: String::new(),
        };
    }

    if !should_send {
        return transport_usage_error("transport: expected --classify-error or --send");
    }

    let sent_order = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut router = TransportRouter::new();
    for spec in transport_specs {
        router.register(CliTransport {
            spec,
            sent: std::rc::Rc::clone(&sent_order),
        });
    }
    let target = TransportTarget {
        oracle: "neo".to_owned(),
        host: None,
        tmux_target: Some("neo:1".to_owned()),
    };
    let result = router.send(&target, "hello", "codex");
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_transport_send_plan_json(&result, &sent_order.borrow())
        } else {
            render_transport_send_plan_text(&result, &sent_order.borrow())
        },
        stderr: String::new(),
    }
}

fn run_transport_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return transport_constants_usage_error(&format!(
                    "transport constants: unknown argument {other}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_transport_constants_json()
        } else {
            "transport constants reasons=timeout,unreachable,auth,rate_limit,rejected,parse_error,unknown\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_transport_constants_json() -> String {
    r#"{"command":"transport","kind":"constants","actions":["classify-error","classify-empty","send"],"failureReasons":["timeout","unreachable","auth","rate_limit","rejected","parse_error","unknown"],"retryableReasons":["timeout","unreachable","rate_limit"],"fatalReasons":["auth","rejected","parse_error","unknown"],"sendFailover":["skip disconnected","skip unreachable","fall through false","fall through retryable throw","stop on fatal throw","first ok wins"],"transportSpec":{"shape":"name[:connected][:canReach][:ok|false|throw=err]","booleanValues":["true","false"],"defaultConnected":true,"defaultCanReach":true,"defaultAction":"ok"},"defaultTarget":{"oracle":"neo","host":null,"tmuxTarget":"neo:1","message":"hello","from":"codex"}}
"#
    .to_owned()
}

fn transport_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs transport --classify-error <error>|--classify-empty|--send [--transport <name[:connected][:canReach][:ok|false|throw=err]>]... [--plan-json]\n       maw-rs transport constants [--plan-json]\n"
        ),
    }
}

fn transport_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs transport constants [--plan-json]\n"),
    }
}

#[derive(Debug, Clone)]
struct CliTransportSpec {
    name: String,
    connected: bool,
    can_reach: bool,
    action: CliTransportAction,
}

#[derive(Debug, Clone)]
enum CliTransportAction {
    Ok,
    False,
    Throw(String),
}

fn parse_transport_spec(value: &str) -> Result<CliTransportSpec, String> {
    let mut parts = value.splitn(4, ':');
    let name = parts.next().unwrap_or_default();
    if name.is_empty() {
        return Err("transport: --transport requires a name".to_owned());
    }
    let connected = parse_optional_bool(parts.next(), true, "connected")?;
    let can_reach = parse_optional_bool(parts.next(), true, "canReach")?;
    let action = match parts.next() {
        None | Some("" | "ok") => CliTransportAction::Ok,
        Some("false") => CliTransportAction::False,
        Some(value) => {
            let Some(error) = value.strip_prefix("throw=") else {
                return Err("transport: action must be ok, false, or throw=<error>".to_owned());
            };
            CliTransportAction::Throw(error.to_owned())
        }
    };
    Ok(CliTransportSpec {
        name: name.to_owned(),
        connected,
        can_reach,
        action,
    })
}

