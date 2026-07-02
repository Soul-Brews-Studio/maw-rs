#[allow(clippy::too_many_lines)]
fn run_discover_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_discover_constants_plan(&argv[1..]);
    }

    let mut plan_json = false;
    let mut json = false;
    let mut tree = false;
    let mut awake = false;
    let mut peer_source_raw: Option<String> = None;
    let mut config = PeerConfig::default();
    let mut discovery_rows = Vec::new();
    let mut panes = Vec::new();
    let mut inventory_input = DiscoverInventoryInput::default();

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--json" => json = true,
            "--tree" => tree = true,
            "--awake" => awake = true,
            "--peers" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --peers value");
                };
                peer_source_raw = Some(value.to_owned());
                index += 1;
            }
            arg if arg.starts_with("--peers=") => {
                peer_source_raw = Some(arg["--peers=".len()..].to_owned());
            }
            "--peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --peer value");
                };
                config.peers.push(value.to_owned());
                index += 1;
            }
            "--named-peer" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --named-peer value");
                };
                match parse_key_value(value, "discover: --named-peer must use <name=url>") {
                    Ok((name, url)) => config.named_peers.push(NamedPeerConfig { name, url }),
                    Err(message) => return discover_usage_error(&message),
                }
                index += 1;
            }
            "--discovered" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --discovered value");
                };
                match parse_discovery_row(value) {
                    Ok(row) => discovery_rows.push(row),
                    Err(message) => {
                        return discover_usage_error(&message.replace("peer-sources", "discover"))
                    }
                }
                index += 1;
            }
            "--pane" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --pane value");
                };
                match parse_discover_pane(value) {
                    Ok(pane) => panes.push(pane),
                    Err(message) => return discover_usage_error(&message),
                }
                index += 1;
            }
            "--plugin" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --plugin value");
                };
                match parse_discover_plugin(value) {
                    Ok(plugin) => inventory_input.plugins.push(plugin),
                    Err(message) => return discover_usage_error(&message),
                }
                index += 1;
            }
            "--ghq" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --ghq value");
                };
                inventory_input.ghq_paths.push(value.to_owned());
                index += 1;
            }
            "--agent" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --agent value");
                };
                match parse_key_value(value, "discover: --agent must use <window=node>") {
                    Ok((window, node)) => {
                        inventory_input.agents.insert(window, node);
                    }
                    Err(message) => return discover_usage_error(&message),
                }
                index += 1;
            }
            "--fleet" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --fleet value");
                };
                match parse_discover_fleet(value) {
                    Ok(record) => inventory_input.fleet.push(record),
                    Err(message) => return discover_usage_error(&message),
                }
                index += 1;
            }
            "--oracle" => {
                let Some(value) = argv.get(index + 1) else {
                    return discover_usage_error("discover: missing --oracle value");
                };
                match parse_discover_oracle(value) {
                    Ok(record) => inventory_input.oracles.push(record),
                    Err(message) => return discover_usage_error(&message),
                }
                index += 1;
            }
            arg => return discover_usage_error(&format!("discover: unknown argument {arg}")),
        }
        index += 1;
    }

    let Some(mode) =
        maw_peer::parse_peer_source_mode(peer_source_raw.as_deref(), PeerSourceMode::Both)
    else {
        return render_discover_invalid_peer_source(plan_json);
    };
    let discoveries = (!discovery_rows.is_empty()).then_some(DiscoveryResult::Ok {
        peers: discovery_rows,
    });
    let result = resolve_peer_sources(&config, mode, discoveries.as_ref());
    let include_live = json || tree || awake;
    let live_probe_calls = usize::from(include_live);
    let live_state = if include_live {
        resolve_tmux_live_state(&result.peers, &panes)
    } else {
        TmuxLiveStateResult {
            source: "tmux".to_owned(),
            live: Vec::new(),
            warnings: Vec::new(),
        }
    };
    let peers_with_live = if include_live {
        mark_peer_targets_live(&result.peers, &live_state.live)
    } else {
        result
            .peers
            .iter()
            .map(peer_with_no_live)
            .collect::<Vec<_>>()
    };
    let visible_peers = if awake && !tree {
        peers_with_live
            .iter()
            .filter(|peer| peer.awake)
            .cloned()
            .collect::<Vec<_>>()
    } else {
        peers_with_live
    };
    let inventory = build_discover_inventory(inventory_input, &visible_peers, &live_state.live);

    CliOutput {
        code: 0,
        stdout: if plan_json || json {
            render_discover_plan_json(
                &result,
                &visible_peers,
                &live_state,
                &inventory,
                tree,
                awake,
                live_probe_calls,
            )
        } else if awake {
            render_discover_live_text(&live_state)
        } else if tree {
            render_discover_tree_text(&visible_peers, &live_state, &inventory)
        } else {
            render_discover_inventory_text(&result, &inventory)
        },
        stderr: String::new(),
    }
}

fn render_discover_invalid_peer_source(plan_json: bool) -> CliOutput {
    let body = "{\"command\":\"discover\",\"ok\":false,\"error\":\"invalid_peer_source\",\"output\":\"usage: maw discover [--peers config|scout|both] [--json] [--tree] [--awake]\",\"fetchCalls\":0,\"liveProbeCalls\":0}\n";
    CliOutput {
        code: if plan_json { 0 } else { 2 },
        stdout: if plan_json {
            body.to_owned()
        } else {
            String::new()
        },
        stderr: if plan_json {
            String::new()
        } else {
            format!("invalid_peer_source\n{}\n", discover_usage())
        },
    }
}

fn run_discover_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return discover_constants_usage_error(&format!(
                    "discover constants: unknown argument {other}"
                ));
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_discover_constants_json()
        } else {
            "discover peerSources=config,scout,both views=json,tree,awake inventorySources=fleet-config,oracle-manifest,plugin-registry,ghq,tmux paneShape=id|command|target|title|pid|cwd|last_activity\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_discover_constants_json() -> String {
    r#"{"command":"discover","action":"constants","peerSources":["config","scout","both"],"views":["json","tree","awake"],"inventorySources":["fleet-config","oracle-manifest","plugin-registry","ghq","tmux"],"paneShape":"id|command|target|title|pid|cwd|last_activity"}
"#
    .to_owned()
}

fn discover_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", discover_usage()),
    }
}

fn discover_usage() -> &'static str {
    "usage: maw-rs discover [--peers config|scout|both] [--peer <url>] [--named-peer <name=url>] [--discovered <node|host|oracle|locator[,locator]>]... [--pane <id|command|target|title|pid|cwd|last_activity>]... [--plugin <name|version|kind|tier|weight|disabled|dir|command|aliases|capabilities|dependencies>] [--ghq <path>] [--agent <window=node>] [--fleet <file|slot|group|session|window|repo>] [--oracle <name|sources|node|session|window|repo|local_path|has_psi|has_fleet_config>] [--json] [--tree] [--awake] [--plan-json]
       maw-rs discover constants [--plan-json]"
}

fn discover_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}
{}
",
            discover_constants_usage()
        ),
    }
}

fn discover_constants_usage() -> &'static str {
    "usage: maw-rs discover constants [--plan-json]"
}

#[derive(Debug, Clone)]
struct DiscoverPluginRecord {
    name: String,
    version: String,
    kind: String,
    tier: String,
    weight: i64,
    disabled: bool,
    dir: String,
    command: String,
    aliases: Vec<String>,
    capabilities: Vec<String>,
    dependencies: Vec<String>,
}

#[derive(Debug, Clone)]
struct GhqRepoRecord {
    path: String,
    name: String,
    owner: Option<String>,
    host: Option<String>,
    oracle_like: bool,
    worktree: bool,
}

#[derive(Debug, Clone)]
struct FleetConfigRecord {
    file: String,
    slot: String,
    name: String,
    session: String,
    window: String,
    repo: String,
    node: String,
    endpoint: Option<String>,
    peer_matched: bool,
}

#[derive(Debug, Clone)]
#[allow(clippy::struct_excessive_bools)]
struct RegisteredOracleRecord {
    name: String,
    sources: Vec<String>,
    node: Option<String>,
    session: Option<String>,
    window: Option<String>,
    repo: Option<String>,
    local_path: Option<String>,
    has_psi: bool,
    has_fleet_config: bool,
    awake: bool,
    ghq_path: Option<String>,
    worktree: bool,
    fleet_matched: bool,
    peer_urls: Vec<String>,
}

#[derive(Debug, Default, Clone)]
struct DiscoverInventoryInput {
    plugins: Vec<DiscoverPluginRecord>,
    ghq_paths: Vec<String>,
    agents: BTreeMap<String, String>,
    fleet: Vec<FleetConfigRecord>,
    oracles: Vec<RegisteredOracleRecord>,
}

#[derive(Debug, Default, Clone)]
struct DiscoverInventory {
    plugins: Vec<DiscoverPluginRecord>,
    ghq: Vec<GhqRepoRecord>,
    fleet: Vec<FleetConfigRecord>,
    oracles: Vec<RegisteredOracleRecord>,
    warnings: Vec<String>,
}

fn parse_discover_pane(value: &str) -> Result<TmuxPane, String> {
    let parts = value.splitn(7, '|').collect::<Vec<_>>();
    if parts.len() != 7 {
        return Err(
            "discover: --pane must use <id|command|target|title|pid|cwd|last_activity>".to_owned(),
        );
    }
    Ok(TmuxPane {
        id: parts[0].to_owned(),
        command: parts[1].to_owned(),
        target: parts[2].to_owned(),
        title: parts[3].to_owned(),
        pid: parse_optional_u32(parts[4], "discover: pane pid must be an integer")?,
        cwd: optional_field(parts[5]),
        last_activity: parse_optional_u64(
            parts[6],
            "discover: pane last_activity must be an integer",
        )?,
    })
}

fn parse_discover_plugin(value: &str) -> Result<DiscoverPluginRecord, String> {
    let parts = value.splitn(11, '|').collect::<Vec<_>>();
    if parts.len() != 11 {
        return Err("discover: --plugin must use <name|version|kind|tier|weight|disabled|dir|command|aliases|capabilities|dependencies>".to_owned());
    }
    Ok(DiscoverPluginRecord {
        name: parts[0].to_owned(),
        version: parts[1].to_owned(),
        kind: parts[2].to_owned(),
        tier: parts[3].to_owned(),
        weight: parts[4]
            .parse::<i64>()
            .map_err(|_| "discover: plugin weight must be an integer".to_owned())?,
        disabled: parse_bool(parts[5], "discover: plugin disabled must be true or false")?,
        dir: parts[6].to_owned(),
        command: parts[7].to_owned(),
        aliases: parse_list_field(parts[8]),
        capabilities: parse_list_field(parts[9]),
        dependencies: parse_list_field(parts[10]),
    })
}

fn parse_discover_fleet(value: &str) -> Result<FleetConfigRecord, String> {
    let parts = value.splitn(6, '|').collect::<Vec<_>>();
    if parts.len() != 6 {
        return Err("discover: --fleet must use <file|slot|group|session|window|repo>".to_owned());
    }
    Ok(FleetConfigRecord {
        file: parts[0].to_owned(),
        slot: parts[1].to_owned(),
        name: parts[2].to_owned(),
        session: parts[3].to_owned(),
        window: parts[4].to_owned(),
        repo: parts[5].to_owned(),
        node: "local".to_owned(),
        endpoint: None,
        peer_matched: false,
    })
}

