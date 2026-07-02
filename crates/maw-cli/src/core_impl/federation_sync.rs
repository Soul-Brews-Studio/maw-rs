fn parse_sync_identity(value: &str) -> Result<SyncPeerIdentity, String> {
    let parts: Vec<&str> = value.split('|').collect();
    if !(parts.len() == 5 || parts.len() == 6) {
        return Err(
            "federation-sync: --identity must use <peer|url|node|agents|reachable|unreachable[,error]>"
                .to_owned(),
        );
    }
    let reachable = match parts[4] {
        "reachable" => true,
        "unreachable" => false,
        _ => {
            return Err(
                "federation-sync: --identity reachability must be reachable or unreachable"
                    .to_owned(),
            )
        }
    };
    Ok(SyncPeerIdentity {
        peer_name: parts[0].to_owned(),
        url: parts[1].to_owned(),
        node: parts[2].to_owned(),
        agents: parts[3]
            .split(',')
            .filter(|agent| !agent.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        reachable,
        error: parts
            .get(5)
            .and_then(|error| (!error.is_empty()).then(|| (*error).to_owned())),
    })
}

fn sync_diff_is_dirty(diff: &SyncDiff) -> bool {
    !(diff.add.is_empty() && diff.stale.is_empty() && diff.conflict.is_empty())
}

fn render_federation_sync_plan_json(
    node: &str,
    flags: FederationSyncFlags,
    dirty: bool,
    diff: &SyncDiff,
    result: &SyncApplyResult,
) -> String {
    format!(
        "{{\"command\":\"federation-sync\",\"node\":{},\"dryRun\":{},\"check\":{},\"force\":{},\"prune\":{},\"dirty\":{dirty},\"diff\":{},\"applied\":{},\"agents\":{}}}\n",
        json_string(node),
        flags.dry_run,
        flags.check,
        flags.force,
        flags.prune,
        render_sync_diff_json(diff),
        json_string_array(&result.applied),
        render_agents_json(&result.agents)
    )
}

fn render_sync_diff_json(diff: &SyncDiff) -> String {
    format!(
        "{{\"add\":{},\"stale\":{},\"conflict\":{},\"unreachable\":{}}}",
        render_sync_adds_json(diff),
        render_sync_stale_json(diff),
        render_sync_conflicts_json(diff),
        render_sync_unreachable_json(diff)
    )
}

fn render_sync_adds_json(diff: &SyncDiff) -> String {
    format!(
        "[{}]",
        diff.add
            .iter()
            .map(|add| {
                format!(
                    "{{\"oracle\":{},\"peerNode\":{},\"fromPeer\":{}}}",
                    json_string(&add.oracle),
                    json_string(&add.peer_node),
                    json_string(&add.from_peer)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_sync_stale_json(diff: &SyncDiff) -> String {
    format!(
        "[{}]",
        diff.stale
            .iter()
            .map(|stale| {
                format!(
                    "{{\"oracle\":{},\"peerNode\":{}}}",
                    json_string(&stale.oracle),
                    json_string(&stale.peer_node)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_sync_conflicts_json(diff: &SyncDiff) -> String {
    format!(
        "[{}]",
        diff.conflict
            .iter()
            .map(|conflict| {
                format!(
                    "{{\"oracle\":{},\"current\":{},\"proposed\":{},\"fromPeer\":{}}}",
                    json_string(&conflict.oracle),
                    json_string(&conflict.current),
                    json_string(&conflict.proposed),
                    json_string(&conflict.from_peer)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_sync_unreachable_json(diff: &SyncDiff) -> String {
    format!(
        "[{}]",
        diff.unreachable
            .iter()
            .map(|peer| {
                let mut fields = vec![
                    format!("\"peerName\":{}", json_string(&peer.peer_name)),
                    format!("\"url\":{}", json_string(&peer.url)),
                ];
                push_json_opt(&mut fields, "error", peer.error.as_deref());
                format!("{{{}}}", fields.join(","))
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_agents_json(agents: &HashMap<String, String>) -> String {
    let sorted = agents
        .iter()
        .map(|(oracle, node)| (oracle.as_str(), node.as_str()))
        .collect::<BTreeMap<_, _>>();
    format!(
        "{{{}}}",
        sorted
            .iter()
            .map(|(oracle, node)| format!("{}:{}", json_string(oracle), json_string(node)))
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_federation_sync_plan_text(
    flags: FederationSyncFlags,
    diff: &SyncDiff,
    result: &SyncApplyResult,
) -> String {
    format!(
        "federation-sync add={} conflict={} stale={} unreachable={} applied={} dryRun={} check={} force={} prune={}\n",
        diff.add.len(),
        diff.conflict.len(),
        diff.stale.len(),
        diff.unreachable.len(),
        result.applied.len(),
        flags.dry_run,
        flags.check,
        flags.force,
        flags.prune
    )
}

fn run_federation_sync_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return federation_sync_constants_usage_error(&format!(
                    "federation-sync constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_federation_sync_constants_json()
        } else {
            "federation-sync diffBuckets=add,stale,conflict,unreachable flags=dry-run,check,force,prune identityReachability=reachable,unreachable checkExitCodes=clean:0,dirty:1\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_federation_sync_constants_json() -> String {
    r#"{"command":"federation-sync","action":"constants","diffBuckets":["add","stale","conflict","unreachable"],"flags":["dry-run","check","force","prune"],"identityReachability":["reachable","unreachable"],"checkExitCodes":{"clean":0,"dirty":1}}
"#
    .to_owned()
}

fn federation_sync_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", federation_sync_usage()),
    }
}

fn federation_sync_usage() -> &'static str {
    "usage: maw-rs federation-sync [--node <name>] [--agent <oracle=node>]... [--identity <peer|url|node|agents|reachable|unreachable[,error]>]... [--dry-run] [--check] [--force] [--prune] [--plan-json]
       maw-rs federation-sync constants [--plan-json]"
}

fn federation_sync_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}
{}
",
            federation_sync_constants_usage()
        ),
    }
}

fn federation_sync_constants_usage() -> &'static str {
    "usage: maw-rs federation-sync constants [--plan-json]"
}

fn run_federation_identity_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_federation_identity_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut node = "local".to_owned();
    let mut url = String::new();
    let mut agents = HashMap::<String, String>::new();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_identity_usage_error(
                        "federation-identity: missing --node value",
                    );
                };
                value.clone_into(&mut node);
                index += 1;
            }
            "--url" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_identity_usage_error(
                        "federation-identity: missing --url value",
                    );
                };
                value.clone_into(&mut url);
                index += 1;
            }
            "--agent" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_identity_usage_error(
                        "federation-identity: missing --agent value",
                    );
                };
                match parse_key_value(value, "federation-identity: --agent must use <oracle=node>")
                {
                    Ok((oracle, route_node)) => {
                        agents.insert(oracle, route_node);
                    }
                    Err(message) => return federation_identity_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return federation_identity_usage_error(&format!(
                    "federation-identity: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let mut hosted = hosted_agents(&agents, &node);
    hosted.sort();
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_federation_identity_plan_json(&node, &url, &hosted, &agents)
        } else {
            render_federation_identity_plan_text(&node, &url, &hosted)
        },
        stderr: String::new(),
    }
}

fn render_federation_identity_plan_json(
    node: &str,
    url: &str,
    hosted: &[String],
    routes: &HashMap<String, String>,
) -> String {
    format!(
        "{{\"command\":\"federation-identity\",\"node\":{},\"url\":{},\"agents\":{},\"routes\":{}}}\n",
        json_string(node),
        json_string(url),
        json_string_array(hosted),
        render_agents_json(routes)
    )
}

fn render_federation_identity_plan_text(node: &str, url: &str, hosted: &[String]) -> String {
    format!(
        "federation-identity node={} url={} agents={}\n",
        node,
        url,
        hosted.len()
    )
}

fn run_federation_identity_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return federation_identity_constants_usage_error(&format!(
                    "federation-identity constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_federation_identity_constants_json()
        } else {
            "federation-identity defaultNode=local defaultUrl= agentShape=oracle=node hostedRule=route-node-equals-local-node routesShape=oracle-to-node-map\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_federation_identity_constants_json() -> String {
    r#"{"command":"federation-identity","action":"constants","defaultNode":"local","defaultUrl":"","agentShape":"oracle=node","hostedRule":"route-node-equals-local-node","routesShape":"oracle-to-node-map"}
"#
    .to_owned()
}

fn federation_identity_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", federation_identity_usage()),
    }
}

fn federation_identity_usage() -> &'static str {
    "usage: maw-rs federation-identity [--node <name>] [--url <url>] [--agent <oracle=node>]... [--plan-json]
       maw-rs federation-identity constants [--plan-json]"
}

fn federation_identity_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}
{}
",
            federation_identity_constants_usage()
        ),
    }
}

fn federation_identity_constants_usage() -> &'static str {
    "usage: maw-rs federation-identity constants [--plan-json]"
}

#[allow(clippy::too_many_lines)]
fn run_federation_health_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_federation_health_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut local_url = "http://localhost:3456".to_owned();
    let mut node = "local".to_owned();
    let mut peers = Vec::<FederationPeerStatus>::new();
    let mut remote_statuses = Vec::<(String, PeerFederationStatusResult)>::new();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_health_usage_error(
                        "federation-health: missing --node value",
                    );
                };
                value.clone_into(&mut node);
                index += 1;
            }
            "--local-url" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_health_usage_error(
                        "federation-health: missing --local-url value",
                    );
                };
                value.clone_into(&mut local_url);
                index += 1;
            }
            "--peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_health_usage_error(
                        "federation-health: missing --peer value",
                    );
                };
                match parse_federation_health_peer(value) {
                    Ok(peer) => peers.push(peer),
                    Err(message) => return federation_health_usage_error(&message),
                }
                index += 1;
            }
            "--remote" => {
                let Some(value) = argv.get(index + 1) else {
                    return federation_health_usage_error(
                        "federation-health: missing --remote value",
                    );
                };
                match parse_federation_health_remote(value) {
                    Ok(remote) => remote_statuses.push(remote),
                    Err(message) => return federation_health_usage_error(&message),
                }
                index += 1;
            }
            arg => {
                return federation_health_usage_error(&format!(
                    "federation-health: unknown argument {arg}"
                ))
            }
        }
        index += 1;
    }

    let base = FederationStatus { local_url, peers };
    let status = classify_symmetric_federation_status(&base, &remote_statuses, &node);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_federation_health_plan_json(&status)
        } else {
            render_federation_health_plan_text(&status)
        },
        stderr: String::new(),
    }
}

