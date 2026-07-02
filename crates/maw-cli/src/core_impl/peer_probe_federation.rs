fn run_peer_probe_handshake_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return peer_probe_handshake_constants_usage_error(&format!(
                    "peer-probe handshake-constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_peer_probe_handshake_constants_json()
        } else {
            "peer-probe handshake validShapes=legacy-true,schema-object-non-empty invalidShapes=empty-object,other-truthy,missing,schema-object-empty\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn parse_probe_error_code(value: &str) -> Option<ProbeErrorCode> {
    match value {
        "DNS" => Some(ProbeErrorCode::Dns),
        "REFUSED" => Some(ProbeErrorCode::Refused),
        "TIMEOUT" => Some(ProbeErrorCode::Timeout),
        "HTTP_4XX" => Some(ProbeErrorCode::Http4xx),
        "HTTP_5XX" => Some(ProbeErrorCode::Http5xx),
        "TLS" => Some(ProbeErrorCode::Tls),
        "BAD_BODY" => Some(ProbeErrorCode::BadBody),
        "UNKNOWN" => Some(ProbeErrorCode::Unknown),
        _ => None,
    }
}

fn render_peer_probe_constants_json() -> String {
    "{\"command\":\"peer-probe\",\"action\":\"constants\",\"codes\":[\"DNS\",\"REFUSED\",\"TIMEOUT\",\"HTTP_4XX\",\"HTTP_5XX\",\"TLS\",\"BAD_BODY\",\"UNKNOWN\"],\"exitCodes\":{\"DNS\":3,\"REFUSED\":4,\"TIMEOUT\":5,\"HTTP_4XX\":6,\"HTTP_5XX\":6,\"TLS\":2,\"BAD_BODY\":2,\"UNKNOWN\":2}}\n".to_owned()
}

fn render_peer_probe_handshake_constants_json() -> String {
    "{\"command\":\"peer-probe\",\"action\":\"handshake-constants\",\"validShapes\":[\"legacy-true\",\"schema-object-non-empty\"],\"invalidShapes\":[\"empty-object\",\"other-truthy\",\"missing\",\"schema-object-empty\"]}\n".to_owned()
}

fn peer_probe_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", peer_probe_usage()),
    }
}

fn peer_probe_usage() -> &'static str {
    "usage: maw-rs peer-probe classify (--http-status <n>|--code <code>|--cause-code <code>|--name <name>|--non-object) [--plan-json]\n       maw-rs peer-probe constants [--plan-json]\n       maw-rs peer-probe format --code <code> --message <msg> --url <url> --alias <alias> [--at <ts>] [--plan-json]\n       maw-rs peer-probe handshake (--legacy-true|--schema <schema>|--empty-object|--other-truthy|--missing) [--plan-json]\n       maw-rs peer-probe handshake-constants [--plan-json]"
}

fn peer_probe_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", peer_probe_constants_usage()),
    }
}

fn peer_probe_constants_usage() -> &'static str {
    "usage: maw-rs peer-probe constants [--plan-json]"
}

fn peer_probe_handshake_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", peer_probe_handshake_constants_usage()),
    }
}

fn peer_probe_handshake_constants_usage() -> &'static str {
    "usage: maw-rs peer-probe handshake-constants [--plan-json]"
}

fn run_peer_sources_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_peer_sources_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut mode = PeerSourceMode::Both;
    let mut config = PeerConfig::default();
    let mut discoveries: Option<DiscoveryResult> = None;
    let mut discovery_rows = Vec::new();
    let mut discovery_error_hint = None;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--mode" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_sources_usage_error("peer-sources: missing --mode value");
                };
                let Some(parsed) = maw_peer::parse_peer_source_mode(Some(value), mode) else {
                    return peer_sources_usage_error("peer-sources: unknown --mode");
                };
                mode = parsed;
                index += 1;
            }
            "--peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_sources_usage_error("peer-sources: missing --peer value");
                };
                config.peers.push(value.to_owned());
                index += 1;
            }
            "--named-peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_sources_usage_error("peer-sources: missing --named-peer value");
                };
                match parse_key_value(value, "peer-sources: --named-peer must use <name=url>") {
                    Ok((name, url)) => config.named_peers.push(NamedPeerConfig { name, url }),
                    Err(message) => return peer_sources_usage_error(&message),
                }
                index += 1;
            }
            "--discovery-ok" => discoveries = Some(DiscoveryResult::Ok { peers: Vec::new() }),
            "--discovery-error" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_sources_usage_error(
                        "peer-sources: missing --discovery-error value",
                    );
                };
                discoveries = Some(DiscoveryResult::Err {
                    error: value.to_owned(),
                    hint: discovery_error_hint.clone(),
                });
                index += 1;
            }
            "--discovery-hint" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_sources_usage_error(
                        "peer-sources: missing --discovery-hint value",
                    );
                };
                discovery_error_hint = Some(value.to_owned());
                if let Some(DiscoveryResult::Err { hint, .. }) = &mut discoveries {
                    hint.clone_from(&discovery_error_hint);
                }
                index += 1;
            }
            "--discovered" => {
                let Some(value) = argv.get(index + 1) else {
                    return peer_sources_usage_error("peer-sources: missing --discovered value");
                };
                match parse_discovery_row(value) {
                    Ok(row) => discovery_rows.push(row),
                    Err(message) => return peer_sources_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return peer_sources_usage_error(&format!("peer-sources: unknown argument {arg}"))
            }
        }
        index += 1;
    }

    if !discovery_rows.is_empty() {
        discoveries = Some(DiscoveryResult::Ok {
            peers: discovery_rows,
        });
    }

    let result = resolve_peer_sources(&config, mode, discoveries.as_ref());
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_peer_sources_plan_json(&result)
        } else {
            render_peer_sources_plan_text(&result)
        },
        stderr: String::new(),
    }
}

fn run_peer_sources_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return peer_sources_constants_usage_error(&format!(
                    "peer-sources constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_peer_sources_constants_json()
        } else {
            "peer-sources modes=config,scout,both configShapes=peer-url,named-peer discoveryStates=ok,error,hint discoveredShape=node|host|oracle|locator[,locator]\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_peer_sources_constants_json() -> String {
    r#"{"command":"peer-sources","action":"constants","modes":["config","scout","both"],"configShapes":["peer-url","named-peer"],"discoveryStates":["ok","error","hint"],"discoveredShape":"node|host|oracle|locator[,locator]"}
"#
    .to_owned()
}

fn peer_sources_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs peer-sources --mode <config|scout|both> [--peer <url>] [--named-peer <name=url>] [--discovery-ok|--discovery-error <error>] [--discovery-hint <hint>] [--discovered <node|host|oracle|locator[,locator]>]... [--plan-json]\n"
        ),
    }
}

fn peer_sources_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}
{}
",
            peer_sources_constants_usage()
        ),
    }
}

fn peer_sources_constants_usage() -> &'static str {
    "usage: maw-rs peer-sources constants [--plan-json]"
}

fn parse_discovery_row(value: &str) -> Result<DiscoveryRow, String> {
    let parts: Vec<&str> = value.splitn(4, '|').collect();
    if parts.len() != 4 {
        return Err(
            "peer-sources: --discovered must use <node|host|oracle|locator[,locator]>".to_owned(),
        );
    }
    Ok(DiscoveryRow {
        node: optional_field(parts[0]),
        host: optional_field(parts[1]),
        oracle: optional_field(parts[2]),
        locators: parts[3]
            .split(',')
            .filter(|locator| !locator.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
    })
}

fn optional_field(value: &str) -> Option<String> {
    if value.is_empty() || value == "-" {
        None
    } else {
        Some(value.to_owned())
    }
}

fn render_peer_sources_plan_json(result: &PeerSourceResult) -> String {
    format!(
        "{{\"command\":\"peer-sources\",\"mode\":{},\"peers\":{},\"warnings\":{},\"fetchCalls\":{}}}\n",
        json_string(result.mode.as_str()),
        render_peer_targets_json(result),
        json_string_array(&result.warnings),
        result.fetch_calls
    )
}

fn render_peer_targets_json(result: &PeerSourceResult) -> String {
    format!(
        "[{}]",
        result
            .peers
            .iter()
            .map(|peer| {
                let mut fields = vec![
                    format!("\"url\":{}", json_string(&peer.url)),
                    format!("\"source\":{}", json_string(peer.source.as_str())),
                ];
                push_json_opt(&mut fields, "name", peer.name.as_deref());
                push_json_opt(&mut fields, "node", peer.node.as_deref());
                push_json_opt(&mut fields, "oracle", peer.oracle.as_deref());
                format!("{{{}}}", fields.join(","))
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_peer_sources_plan_text(result: &PeerSourceResult) -> String {
    let mut lines = vec![format!(
        "peer-sources mode={} fetchCalls={}",
        result.mode.as_str(),
        result.fetch_calls
    )];
    for peer in &result.peers {
        lines.push(format!(
            "{} {} {}",
            peer.source.as_str(),
            peer.name.as_deref().unwrap_or("-"),
            peer.url
        ));
    }
    for warning in &result.warnings {
        lines.push(format!("warning: {warning}"));
    }
    lines.join("\n") + "\n"
}

#[allow(clippy::too_many_lines)]
fn run_federation_sync_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_federation_sync_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut flags = FederationSyncFlags::default();
    let mut node = "local".to_owned();
    let mut agents = HashMap::<String, String>::new();
    let mut identities = Vec::<SyncPeerIdentity>::new();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--dry-run" => flags.dry_run = true,
            "--check" => flags.check = true,
            "--force" => flags.force = true,
            "--prune" => flags.prune = true,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_sync_usage_error("federation-sync: missing --node value");
                };
                value.clone_into(&mut node);
                index += 1;
            }
            "--agent" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_sync_usage_error("federation-sync: missing --agent value");
                };
                match parse_key_value(value, "federation-sync: --agent must use <oracle=node>") {
                    Ok((oracle, node)) => {
                        agents.insert(oracle, node);
                    }
                    Err(message) => return federation_sync_usage_error(&message),
                }
                index += 1;
            }
            "--identity" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_sync_usage_error(
                        "federation-sync: missing --identity value",
                    );
                };
                match parse_sync_identity(value) {
                    Ok(identity) => identities.push(identity),
                    Err(message) => return federation_sync_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return federation_sync_usage_error(&format!(
                    "federation-sync: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let diff = compute_sync_diff(&agents, &identities, &node);
    let dirty = sync_diff_is_dirty(&diff);
    let result = if flags.check || flags.dry_run {
        SyncApplyResult {
            agents: agents.clone(),
            applied: Vec::new(),
        }
    } else {
        apply_sync_diff(
            &agents,
            &diff,
            SyncApplyOptions {
                force: flags.force,
                prune: flags.prune,
            },
        )
    };
    let code = i32::from(flags.check && dirty);

    CliOutput {
        code,
        stdout: if plan_json {
            render_federation_sync_plan_json(&node, flags, dirty, &diff, &result)
        } else {
            render_federation_sync_plan_text(flags, &diff, &result)
        },
        stderr: String::new(),
    }
}

