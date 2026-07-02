fn parse_optional_bool(value: Option<&str>, default: bool, name: &str) -> Result<bool, String> {
    match value {
        None | Some("") => Ok(default),
        Some("true") => Ok(true),
        Some("false") => Ok(false),
        Some(_) => Err(format!("transport: invalid {name} boolean")),
    }
}

struct CliTransport {
    spec: CliTransportSpec,
    sent: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl Transport for CliTransport {
    fn name(&self) -> &str {
        &self.spec.name
    }

    fn connected(&self) -> bool {
        self.spec.connected
    }

    fn can_reach(&self, _target: &TransportTarget) -> bool {
        self.spec.can_reach
    }

    fn send(
        &mut self,
        _target: &TransportTarget,
        _message: &str,
        _from: &str,
    ) -> Result<bool, String> {
        self.sent.borrow_mut().push(self.spec.name.clone());
        match &self.spec.action {
            CliTransportAction::Ok => Ok(true),
            CliTransportAction::False => Ok(false),
            CliTransportAction::Throw(error) => Err(error.clone()),
        }
    }
}

fn render_transport_send_plan_json(result: &TransportResult, sent: &[String]) -> String {
    let mut fields = vec![
        "\"command\":\"transport\"".to_owned(),
        "\"kind\":\"send\"".to_owned(),
        format!("\"ok\":{}", result.ok),
        format!("\"via\":{}", json_string(&result.via)),
        format!("\"retryable\":{}", result.retryable),
        format!("\"sent\":{}", json_string_array(sent)),
    ];
    if let Some(reason) = result.reason {
        fields.push(format!("\"reason\":{}", json_string(reason.as_str())));
    }
    format!("{{{}}}\n", fields.join(","))
}

fn render_transport_send_plan_text(result: &TransportResult, sent: &[String]) -> String {
    let reason = result.reason.map_or("-", TransportFailureReason::as_str);
    format!(
        "transport send ok={} via={} reason={} retryable={} sent={}\n",
        result.ok,
        result.via,
        reason,
        result.retryable,
        sent.join(",")
    )
}

fn run_split_policy_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_split_policy_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut pane_current_command = None;
    let mut requested_policy = None;
    let mut no_attach = false;
    let mut force_split = false;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--pane-current-command" => {
                let Some(value) = argv.get(index + 1) else {
                    return split_policy_usage_error(
                        "split-policy: missing --pane-current-command value",
                    );
                };
                pane_current_command = Some(value.to_owned());
                index += 1;
            }
            "--requested-policy" | "--claude-pane-policy" => {
                let Some(value) = argv.get(index + 1) else {
                    return split_policy_usage_error(
                        "split-policy: missing --requested-policy value",
                    );
                };
                requested_policy = Some(value.to_owned());
                index += 1;
            }
            "--no-attach" => no_attach = true,
            "--force-split" => force_split = true,
            arg => {
                return split_policy_usage_error(&format!("split-policy: unknown argument {arg}"))
            }
        }
        index += 1;
    }

    let input = SplitPolicyInput {
        pane_current_command,
        no_attach,
        requested_policy,
        force_split,
    };

    match decide_split_policy(&input) {
        Ok(decision) => CliOutput {
            code: 0,
            stdout: if plan_json {
                render_split_policy_plan_json(decision)
            } else {
                render_split_policy_plan_text(decision)
            },
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("split-policy: {error}\n"),
        },
    }
}

fn run_split_policy_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return split_policy_constants_usage_error(&format!(
                    "split-policy constants: unknown argument {other}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_split_policy_constants_json()
        } else {
            "split-policy constants actions=split,background-tab,link-window,refuse\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_split_policy_constants_json() -> String {
    r#"{"command":"split-policy","kind":"constants","actions":["split","background-tab","link-window","refuse"],"reasons":["not-attaching","force-split","not-claude","claude-policy"],"defaultClaudePolicy":"background-tab","policyFlags":["--requested-policy","--claude-pane-policy"],"precedence":["no-attach","force-split","not-claude","claude-policy"],"claudeLikeCommands":["claude","version-like semver command"]}
"#
    .to_owned()
}

fn split_policy_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs split-policy [--pane-current-command <cmd>] [--requested-policy <policy>] [--no-attach] [--force-split] [--plan-json]\n       maw-rs split-policy constants [--plan-json]\n"
        ),
    }
}

fn split_policy_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs split-policy constants [--plan-json]\n"),
    }
}

fn render_split_policy_plan_json(decision: SplitPolicyDecision) -> String {
    format!(
        "{{\"command\":\"split-policy\",\"action\":{},\"reason\":{}}}\n",
        json_string(decision.action.as_str()),
        json_string(decision.reason.as_str())
    )
}

fn render_split_policy_plan_text(decision: SplitPolicyDecision) -> String {
    format!(
        "split-policy action={} reason={}\n",
        decision.action.as_str(),
        decision.reason.as_str()
    )
}

fn run_peer_probe_plan(argv: &[String]) -> CliOutput {
    let Some(action) = argv.first().map(String::as_str) else {
        return peer_probe_usage_error("peer-probe: missing action");
    };
    match action {
        "classify" => run_peer_probe_classify_plan(&argv[1..]),
        "constants" => run_peer_probe_constants_plan(&argv[1..]),
        "format" => run_peer_probe_format_plan(&argv[1..]),
        "handshake" => run_peer_probe_handshake_plan(&argv[1..]),
        "handshake-constants" => run_peer_probe_handshake_constants_plan(&argv[1..]),
        _ => peer_probe_usage_error("peer-probe: invalid action"),
    }
}

fn run_peer_probe_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return peer_probe_constants_usage_error(&format!(
                    "peer-probe constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_peer_probe_constants_json()
        } else {
            "peer-probe codes=DNS,REFUSED,TIMEOUT,HTTP_4XX,HTTP_5XX,TLS,BAD_BODY,UNKNOWN exitCodes=DNS:3,REFUSED:4,TIMEOUT:5,HTTP_4XX:6,HTTP_5XX:6,TLS:2,BAD_BODY:2,UNKNOWN:2\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn run_peer_probe_classify_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut input = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--http-status" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error(
                        "peer-probe classify: missing --http-status value",
                    );
                };
                let Ok(status) = value.parse::<u16>() else {
                    return peer_probe_usage_error(
                        "peer-probe classify: --http-status must be an integer",
                    );
                };
                input = Some(ProbeFailureInput::Http { status, ok: false });
                index += 1;
            }
            "--code" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe classify: missing --code value");
                };
                input = Some(ProbeFailureInput::Code(value.to_owned()));
                index += 1;
            }
            "--cause-code" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error(
                        "peer-probe classify: missing --cause-code value",
                    );
                };
                input = Some(ProbeFailureInput::CauseCode(value.to_owned()));
                index += 1;
            }
            "--name" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe classify: missing --name value");
                };
                input = Some(ProbeFailureInput::Name(value.to_owned()));
                index += 1;
            }
            "--non-object" => input = Some(ProbeFailureInput::NonObject),
            arg => {
                return peer_probe_usage_error(&format!(
                    "peer-probe classify: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }
    let Some(input) = input else {
        return peer_probe_usage_error("peer-probe classify: missing input");
    };
    let code = classify_probe_error(&input);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"peer-probe\",\"action\":\"classify\",\"ok\":true,\"code\":{},\"exitCode\":{},\"hint\":{}}}\n",
                json_string(code.as_str()),
                probe_exit_code(code),
                json_string(maw_peer::probe_hint(code))
            )
        } else {
            format!("{}\n", code.as_str())
        },
        stderr: String::new(),
    }
}

fn run_peer_probe_format_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut code = None;
    let mut message = None;
    let mut at = "now".to_owned();
    let mut url = None;
    let mut alias = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--code" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe format: missing --code value");
                };
                code = parse_probe_error_code(value);
                if code.is_none() {
                    return peer_probe_usage_error("peer-probe format: invalid --code value");
                }
                index += 1;
            }
            "--message" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe format: missing --message value");
                };
                message = Some(value.to_owned());
                index += 1;
            }
            "--at" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe format: missing --at value");
                };
                value.clone_into(&mut at);
                index += 1;
            }
            "--url" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe format: missing --url value");
                };
                url = Some(value.to_owned());
                index += 1;
            }
            "--alias" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe format: missing --alias value");
                };
                alias = Some(value.to_owned());
                index += 1;
            }
            arg => {
                return peer_probe_usage_error(&format!(
                    "peer-probe format: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }
    let (Some(code), Some(message), Some(url), Some(alias)) = (code, message, url, alias) else {
        return peer_probe_usage_error("peer-probe format: missing required value");
    };
    let err = ProbeLastError { code, message, at };
    let formatted = format_probe_error(&err, &url, &alias);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"peer-probe\",\"action\":\"format\",\"ok\":true,\"code\":{},\"host\":{},\"hint\":{},\"formatted\":{}}}\n",
                json_string(code.as_str()),
                json_string(&safe_probe_host(&url)),
                json_string(pick_probe_hint(&err)),
                json_string(&formatted)
            )
        } else {
            formatted + "\n"
        },
        stderr: String::new(),
    }
}

fn run_peer_probe_handshake_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    let mut handshake = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--legacy-true" => handshake = Some(ProbeMawHandshake::LegacyTrue),
            "--schema" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_probe_usage_error("peer-probe handshake: missing --schema value");
                };
                handshake = Some(ProbeMawHandshake::SchemaObject(value.to_owned()));
                index += 1;
            }
            "--empty-object" => handshake = Some(ProbeMawHandshake::EmptyObject),
            "--other-truthy" => handshake = Some(ProbeMawHandshake::OtherTruthy),
            "--missing" => handshake = Some(ProbeMawHandshake::Missing),
            arg => {
                return peer_probe_usage_error(&format!(
                    "peer-probe handshake: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }
    let Some(handshake) = handshake else {
        return peer_probe_usage_error("peer-probe handshake: missing shape");
    };
    let valid = is_valid_maw_handshake(&handshake);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"peer-probe\",\"action\":\"handshake\",\"ok\":true,\"valid\":{valid}}}\n"
            )
        } else {
            format!("valid={valid}\n")
        },
        stderr: String::new(),
    }
}

