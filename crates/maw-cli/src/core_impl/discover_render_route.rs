fn render_discover_inventory_text(
    result: &PeerSourceResult,
    inventory: &DiscoverInventory,
) -> String {
    let mut output = render_peer_sources_plan_text(result);
    if !inventory.oracles.is_empty() {
        output.push_str("registered oracles\n");
        for oracle in &inventory.oracles {
            let status = if oracle.awake { "awake" } else { "offline" };
            let _ = writeln!(output, "{} {}", oracle.name, status);
        }
    }
    if !inventory.fleet.is_empty() {
        output.push_str("fleet config\n");
        for record in &inventory.fleet {
            let _ = writeln!(output, "{} {} {}", record.name, record.node, record.repo);
        }
    }
    if !inventory.plugins.is_empty() {
        output.push_str("plugin registry\n");
        for plugin in &inventory.plugins {
            let status = if plugin.disabled {
                "disabled"
            } else {
                "enabled"
            };
            let _ = writeln!(output, "{} {} {}", plugin.name, plugin.version, status);
        }
    }
    if !inventory.ghq.is_empty() {
        output.push_str("ghq repos\n");
        for repo in &inventory.ghq {
            let _ = writeln!(output, "{} {}", repo.name, repo.path);
        }
    }
    output
}

fn render_discover_tree_text(
    peers: &[PeerTargetWithLive],
    live_state: &TmuxLiveStateResult,
    inventory: &DiscoverInventory,
) -> String {
    let mut output = String::new();
    let _ = writeln!(output, "discover tree");
    let _ = writeln!(output, "live ({} sessions)", live_state.live.len());
    let _ = writeln!(output, "peers ({} configured)", peers.len());
    let _ = writeln!(
        output,
        "fleet config ({} configured)",
        inventory.fleet.len()
    );
    let _ = writeln!(output, "registered oracles ({})", inventory.oracles.len());
    let _ = writeln!(output, "plugins ({} registered)", inventory.plugins.len());
    for plugin in &inventory.plugins {
        let _ = writeln!(output, "  - {}", plugin.name);
    }
    let _ = writeln!(output, "ghq ({} repos)", inventory.ghq.len());
    for repo in &inventory.ghq {
        let _ = writeln!(output, "  - {}", repo.path);
    }
    output
}

fn render_live_peer_targets_json(peers: &[PeerTargetWithLive]) -> String {
    format!(
        "[{}]",
        peers
            .iter()
            .map(|peer| {
                let mut fields = vec![
                    format!("\"url\":{}", json_string(&peer.url)),
                    format!("\"source\":{}", json_string(peer.source.as_str())),
                ];
                push_json_opt(&mut fields, "name", peer.name.as_deref());
                push_json_opt(&mut fields, "node", peer.node.as_deref());
                push_json_opt(&mut fields, "oracle", peer.oracle.as_deref());
                fields.push(format!("\"awake\":{}", peer.awake));
                fields.push(format!(
                    "\"liveTargets\":{}",
                    json_string_array(&peer.live_targets)
                ));
                fields.push(format!(
                    "\"liveSessions\":{}",
                    json_string_array(&peer.live_sessions)
                ));
                format!("{{{}}}", fields.join(","))
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_live_state_json(live_state: &TmuxLiveStateResult) -> String {
    format!(
        "{{\"source\":{},\"total\":{},\"panes\":{},\"sessions\":{}}}",
        json_string(&live_state.source),
        live_state.live.len(),
        render_live_panes_json(&live_state.live),
        render_live_sessions_json(&live_state.live)
    )
}

fn render_live_panes_json(panes: &[DiscoverLivePane]) -> String {
    format!(
        "[{}]",
        panes
            .iter()
            .map(|pane| {
                let mut fields = vec![
                    format!("\"source\":{}", json_string(&pane.source)),
                    format!("\"id\":{}", json_string(&pane.id)),
                    format!("\"target\":{}", json_string(&pane.target)),
                    format!("\"session\":{}", json_string(&pane.session)),
                    format!("\"window\":{}", json_string(&pane.window)),
                    format!("\"pane\":{}", json_string(&pane.pane)),
                    format!("\"awake\":{}", pane.awake),
                    format!("\"matches\":{}", json_string_array(&pane.matches)),
                ];
                push_json_opt(&mut fields, "command", pane.command.as_deref());
                push_json_opt(&mut fields, "title", pane.title.as_deref());
                if let Some(pid) = pane.pid {
                    fields.push(format!("\"pid\":{pid}"));
                }
                push_json_opt(&mut fields, "cwd", pane.cwd.as_deref());
                if let Some(last_activity) = pane.last_activity {
                    fields.push(format!("\"lastActivity\":{last_activity}"));
                }
                format!("{{{}}}", fields.join(","))
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_live_sessions_json(panes: &[DiscoverLivePane]) -> String {
    let mut sessions: BTreeMap<&str, BTreeMap<&str, Vec<&DiscoverLivePane>>> = BTreeMap::new();
    for pane in panes {
        sessions
            .entry(&pane.session)
            .or_default()
            .entry(&pane.window)
            .or_default()
            .push(pane);
    }
    format!(
        "[{}]",
        sessions
            .into_iter()
            .map(|(name, windows)| {
                let pane_count = windows.values().map(Vec::len).sum::<usize>();
                let windows_json = windows
                    .into_iter()
                    .map(|(window_name, window_panes)| {
                        let cloned = window_panes.into_iter().cloned().collect::<Vec<_>>();
                        format!(
                            "{{\"name\":{},\"paneCount\":{},\"panes\":{}}}",
                            json_string(window_name),
                            cloned.len(),
                            render_live_panes_json(&cloned)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!(
                    "{{\"source\":\"tmux\",\"name\":{},\"awake\":true,\"paneCount\":{},\"windows\":[{}]}}",
                    json_string(name),
                    pane_count,
                    windows_json
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_discover_live_text(live_state: &TmuxLiveStateResult) -> String {
    if live_state.live.is_empty() {
        return "no live tmux sessions/windows found\n".to_owned();
    }
    live_state
        .live
        .iter()
        .map(|pane| {
            format!(
                "tmux {} {}",
                pane.target,
                pane.command.as_deref().unwrap_or("-")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

#[allow(clippy::too_many_lines)]
fn run_route_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_route_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut query = None;
    let mut config = RouteConfig::default();
    let mut sessions: Vec<RouteSession> = Vec::new();
    let mut current_session: Option<RouteSession> = None;

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--query" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --query value");
                };
                query = Some(value.to_owned());
                index += 1;
            }
            "--node" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --node value");
                };
                config.node = Some(value.to_owned());
                index += 1;
            }
            "--named-peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --named-peer value");
                };
                match parse_key_value(value, "route: --named-peer must use <name=url>") {
                    Ok((name, url)) => config.named_peers.push(RouteNamedPeer { name, url }),
                    Err(message) => return route_usage_error(&message),
                }
                index += 1;
            }
            "--peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --peer value");
                };
                config.peers.push(value.to_owned());
                index += 1;
            }
            "--agent" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --agent value");
                };
                match parse_key_value(value, "route: --agent must use <agent=node>") {
                    Ok((agent, node)) => {
                        config.agents.insert(agent, node);
                    }
                    Err(message) => return route_usage_error(&message),
                }
                index += 1;
            }
            "--session" => {
                if let Some(session) = current_session.take() {
                    sessions.push(session);
                }
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --session value");
                };
                current_session = Some(RouteSession {
                    name: value.to_owned(),
                    windows: Vec::new(),
                    source: None,
                });
                index += 1;
            }
            "--source" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --source value");
                };
                let Some(session) = &mut current_session else {
                    return route_usage_error("route: --source must follow a --session");
                };
                session.source = Some(value.to_owned());
                index += 1;
            }
            "--window" => {
                let Some(value) = argv.get(index + 1) else {
                    return route_usage_error("route: missing --window value");
                };
                let Some(session) = &mut current_session else {
                    return route_usage_error("route: --window must follow a --session");
                };
                match parse_route_window(value) {
                    Ok(window) => session.windows.push(window),
                    Err(message) => return route_usage_error(&message),
                }
                index += 1;
            }
            arg => return route_usage_error(&format!("route: unknown argument {arg}")),
        }
        index += 1;
    }
    if let Some(session) = current_session.take() {
        sessions.push(session);
    }

    let Some(query) = query else {
        return route_usage_error("route: expected --query <target>");
    };
    let result = resolve_route_target(&query, &config, &sessions);
    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_route_plan_json(&query, &result)
        } else {
            render_route_plan_text(&query, &result)
        },
        stderr: String::new(),
    }
}

fn route_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs route --query <target> [--node <name>] [--named-peer <name=url>] [--peer <url>] [--agent <agent=node>] [--session <name>] [--source <source>] [--window <index:name:active>]... [--plan-json]\nusage: maw-rs route constants [--plan-json]\n"
        ),
    }
}

fn run_route_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            _ => {
                return route_constants_usage_error(&format!(
                    "route constants: unknown argument {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_route_constants_json()
        } else {
            "route constants result-types=local,peer,self-node,error window-shape=index:name:active\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_route_constants_json() -> String {
    concat!(
        "{\"command\":\"route\",\"kind\":\"constants\",",
        "\"resultTypes\":[\"local\",\"peer\",\"self-node\",\"error\"],",
        "\"inputs\":[\"query\",\"node\",\"named-peer\",\"peer\",\"agent\",\"session\",\"source\",\"window\"],",
        "\"windowShape\":\"index:name:active\",\"keyValueShapes\":{\"namedPeer\":\"name=url\",\"agent\":\"agent=node\"},",
        "\"precedence\":[\"empty-query-error\",\"filter-writable-local-sessions\",\"bare-session-alias-window\",\"direct-local-window\",\"node-agent-prefix\",\"agents-map\",\"not-found\"],",
        "\"localFilters\":{\"ignoreViewSessions\":true,\"localSourceOnly\":true},",
        "\"nodeRouting\":{\"selfAliases\":[\"configured-node\",\"local\"],\"peerSources\":[\"namedPeers exact name\",\"legacy peers URL contains node\"],\"slashDisablesNodeRouting\":true,\"multipleColonsKeepAgentSuffix\":true},",
        "\"aliasRules\":[\"skip queries ending -oracle\",\"strip numeric fleet session prefix\",\"prefer oracle-named window\",\"single-window session fallback\",\"refuse ambiguous session aliases\",\"refuse first-window fallback for multi-window alias miss\"],",
        "\"errorReasons\":[\"empty_query\",\"self_not_running\",\"unknown_node\",\"no_peer_url\",\"not_found\",\"session_alias_ambiguous\",\"session_window_not_found\"],",
        "\"fixtureCounts\":{\"total\":20,\"local\":7,\"peer\":4,\"self-node\":1,\"error\":8}}\n"
    )
    .to_owned()
}

fn route_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\nusage: maw-rs route constants [--plan-json]\n"),
    }
}

fn parse_key_value(value: &str, message: &str) -> Result<(String, String), String> {
    let Some((key, value)) = value.split_once('=') else {
        return Err(message.to_owned());
    };
    if key.is_empty() || value.is_empty() {
        return Err(message.to_owned());
    }
    Ok((key.to_owned(), value.to_owned()))
}

fn parse_route_window(value: &str) -> Result<RouteWindow, String> {
    let mut parts = value.splitn(3, ':');
    let index = parts
        .next()
        .unwrap_or_default()
        .parse::<u32>()
        .map_err(|_| "route: invalid window index".to_owned())?;
    let Some(name) = parts.next() else {
        return Err("route: window must use <index:name:active>".to_owned());
    };
    let active = match parts.next() {
        Some("true") => true,
        Some("false") => false,
        _ => return Err("route: window active must be true or false".to_owned()),
    };
    Ok(RouteWindow {
        index,
        name: name.to_owned(),
        active,
    })
}

