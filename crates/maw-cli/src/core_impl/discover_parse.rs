fn parse_discover_oracle(value: &str) -> Result<RegisteredOracleRecord, String> {
    let parts = value.splitn(9, '|').collect::<Vec<_>>();
    if parts.len() != 9 {
        return Err("discover: --oracle must use <name|sources|node|session|window|repo|local_path|has_psi|has_fleet_config>".to_owned());
    }
    Ok(RegisteredOracleRecord {
        name: parts[0].to_owned(),
        sources: parse_plus_list_field(parts[1]),
        node: optional_field(parts[2]),
        session: optional_field(parts[3]),
        window: optional_field(parts[4]),
        repo: optional_field(parts[5]),
        local_path: optional_field(parts[6]),
        has_psi: parse_bool(parts[7], "discover: oracle has_psi must be true or false")?,
        has_fleet_config: parse_bool(
            parts[8],
            "discover: oracle has_fleet_config must be true or false",
        )?,
        awake: false,
        ghq_path: None,
        worktree: false,
        fleet_matched: false,
        peer_urls: Vec::new(),
    })
}

fn parse_bool(value: &str, message: &str) -> Result<bool, String> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(message.to_owned()),
    }
}

fn parse_list_field(value: &str) -> Vec<String> {
    if value.is_empty() || value == "-" {
        Vec::new()
    } else {
        value.split(',').map(ToOwned::to_owned).collect()
    }
}

fn parse_plus_list_field(value: &str) -> Vec<String> {
    if value.is_empty() || value == "-" {
        Vec::new()
    } else {
        value.split('+').map(ToOwned::to_owned).collect()
    }
}

fn parse_optional_u32(value: &str, message: &str) -> Result<Option<u32>, String> {
    if value.is_empty() || value == "-" {
        return Ok(None);
    }
    value
        .parse::<u32>()
        .map(Some)
        .map_err(|_| message.to_owned())
}

fn parse_optional_u64(value: &str, message: &str) -> Result<Option<u64>, String> {
    if value.is_empty() || value == "-" {
        return Ok(None);
    }
    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_| message.to_owned())
}

fn peer_with_no_live(peer: &maw_peer::PeerTarget) -> PeerTargetWithLive {
    PeerTargetWithLive {
        name: peer.name.clone(),
        url: peer.url.clone(),
        source: peer.source,
        node: peer.node.clone(),
        oracle: peer.oracle.clone(),
        awake: false,
        live_targets: Vec::new(),
        live_sessions: Vec::new(),
    }
}

fn build_discover_inventory(
    mut input: DiscoverInventoryInput,
    peers: &[PeerTargetWithLive],
    live_panes: &[DiscoverLivePane],
) -> DiscoverInventory {
    let mut seen_paths = BTreeSet::new();
    let ghq = input
        .ghq_paths
        .iter()
        .map(|path| path.trim_end_matches('/').replace('\\', "/"))
        .filter(|path| seen_paths.insert(path.to_lowercase()))
        .map(|path| ghq_repo_record(&path))
        .collect::<Vec<_>>();

    let mut seen_fleet = BTreeSet::new();
    let fleet = input
        .fleet
        .drain(..)
        .filter_map(|mut record| {
            record.node = input
                .agents
                .get(&record.window)
                .cloned()
                .unwrap_or_else(|| "local".to_owned());
            if let Some(peer) = peers.iter().find(|peer| {
                peer_matches_name(peer, &record.node)
                    || peer_matches_name(peer, &record.name)
                    || peer_matches_name(peer, &record.window)
            }) {
                record.endpoint = Some(peer.url.clone());
                record.peer_matched = true;
            }
            let key = format!(
                "{}\0{}\0{}",
                record.node.to_lowercase(),
                record.name.to_lowercase(),
                record.repo.to_lowercase()
            );
            seen_fleet.insert(key).then_some(record)
        })
        .collect::<Vec<_>>();

    let mut seen_oracles = BTreeSet::new();
    let oracles = input
        .oracles
        .drain(..)
        .filter_map(|mut oracle| {
            let key = oracle.name.to_lowercase();
            if !seen_oracles.insert(key) {
                return None;
            }
            join_oracle_inventory(&mut oracle, &ghq, &fleet, peers, live_panes);
            Some(oracle)
        })
        .collect::<Vec<_>>();

    DiscoverInventory {
        plugins: input.plugins,
        ghq,
        fleet,
        oracles,
        warnings: Vec::new(),
    }
}

fn is_worktree_path(path: &str, parts: &[&str]) -> bool {
    path.contains(".wt-")
        || path.contains(".wt/")
        || path.contains(".wt.")
        || parts.windows(2).any(|window| window[0] == "agents" && !window[1].is_empty())
}

fn ghq_repo_record(path: &str) -> GhqRepoRecord {
    let parts = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let name = parts.last().copied().unwrap_or(path).to_owned();
    let host_index = parts.iter().position(|part| part.contains('.'));
    let host = host_index.map(|index| parts[index].to_owned());
    let owner = host_index
        .and_then(|index| parts.get(index + 1))
        .or_else(|| parts.get(parts.len().saturating_sub(2)))
        .map(|owner| (*owner).to_owned());
    GhqRepoRecord {
        path: path.to_owned(),
        oracle_like: is_oracle_like(&name),
        worktree: is_worktree_path(path, &parts),
        name,
        owner,
        host,
    }
}

fn is_oracle_like(name: &str) -> bool {
    name.contains("oracle")
}

fn join_oracle_inventory(
    oracle: &mut RegisteredOracleRecord,
    ghq: &[GhqRepoRecord],
    fleet: &[FleetConfigRecord],
    peers: &[PeerTargetWithLive],
    live_panes: &[DiscoverLivePane],
) {
    if let Some(repo) = ghq.iter().find(|repo| ghq_matches_oracle(repo, oracle)) {
        oracle.ghq_path = Some(repo.path.clone());
        oracle.worktree = repo.worktree;
    }
    oracle.fleet_matched = fleet.iter().any(|record| {
        names_match(&record.name, &oracle.name) || names_match(&record.window, &oracle.name)
    });
    oracle.peer_urls = peers
        .iter()
        .filter(|peer| {
            peer_matches_name(peer, &oracle.name)
                || oracle
                    .node
                    .as_deref()
                    .is_some_and(|node| peer_matches_name(peer, node))
        })
        .map(|peer| peer.url.clone())
        .collect();
    oracle.awake = live_panes
        .iter()
        .any(|pane| pane_matches_oracle(pane, oracle));
}

fn ghq_matches_oracle(repo: &GhqRepoRecord, oracle: &RegisteredOracleRecord) -> bool {
    if oracle.local_path.as_deref() == Some(repo.path.as_str()) {
        return true;
    }
    if oracle.repo.as_deref().is_some_and(|slug| {
        slug.rsplit('/').next() == Some(repo.name.as_str()) || slug.ends_with(&repo.name)
    }) {
        return true;
    }
    names_match(&repo.name, &oracle.name)
}

fn names_match(candidate: &str, name: &str) -> bool {
    let candidate = candidate.to_lowercase();
    let name = name.to_lowercase();
    candidate == name
        || candidate == format!("{name}-oracle")
        || candidate.ends_with(&format!("-{name}"))
}

fn peer_matches_name(peer: &PeerTargetWithLive, name: &str) -> bool {
    peer.name
        .as_deref()
        .is_some_and(|candidate| names_match(candidate, name))
        || peer
            .node
            .as_deref()
            .is_some_and(|candidate| names_match(candidate, name))
        || peer
            .oracle
            .as_deref()
            .is_some_and(|candidate| names_match(candidate, name))
}

fn pane_matches_oracle(pane: &DiscoverLivePane, oracle: &RegisteredOracleRecord) -> bool {
    oracle.session.as_deref() == Some(pane.session.as_str())
        || oracle.window.as_deref() == Some(pane.window.as_str())
        || names_match(&pane.window, &oracle.name)
        || pane
            .matches
            .iter()
            .any(|matched| names_match(matched, &oracle.name))
}

fn render_discover_plan_json(
    result: &PeerSourceResult,
    peers: &[PeerTargetWithLive],
    live_state: &TmuxLiveStateResult,
    inventory: &DiscoverInventory,
    tree: bool,
    awake: bool,
    live_probe_calls: usize,
) -> String {
    let warnings = result
        .warnings
        .iter()
        .chain(live_state.warnings.iter())
        .chain(inventory.warnings.iter())
        .cloned()
        .collect::<Vec<_>>();
    let total = if tree {
        peers.len()
            + live_state.live.len()
            + inventory.fleet.len()
            + inventory.oracles.len()
            + inventory.plugins.len()
            + inventory.ghq.len()
    } else {
        peers.len()
    };
    let tree_field = if tree {
        format!(
            ",\"tree\":{{\"live\":{},\"peers\":{},\"fleet\":{},\"oracles\":{},\"plugins\":{},\"ghq\":{}}}",
            render_live_sessions_json(&live_state.live),
            render_live_peer_targets_json(peers),
            render_fleet_records_json(&inventory.fleet),
            render_oracle_records_json(&inventory.oracles),
            render_plugin_records_json(&inventory.plugins),
            render_ghq_records_json(&inventory.ghq)
        )
    } else {
        String::new()
    };
    format!(
        "{{\"command\":\"discover\",\"ok\":true,\"mode\":{},\"total\":{},\"awake\":{},\"awakeOnly\":{},\"peers\":{},\"fleet\":{{\"source\":\"fleet-config\",\"total\":{},\"records\":{}}},\"oracles\":{{\"source\":\"oracle-manifest\",\"total\":{},\"records\":{}}},\"plugins\":{{\"source\":\"plugin-registry\",\"total\":{},\"records\":{}}},\"ghq\":{{\"source\":\"ghq\",\"total\":{},\"repos\":{}}},\"liveTotal\":{},\"live\":{}{},\"warnings\":{},\"fetchCalls\":{},\"liveProbeCalls\":{}}}\n",
        json_string(result.mode.as_str()),
        total,
        awake,
        awake,
        render_live_peer_targets_json(peers),
        inventory.fleet.len(),
        render_fleet_records_json(&inventory.fleet),
        inventory.oracles.len(),
        render_oracle_records_json(&inventory.oracles),
        inventory.plugins.len(),
        render_plugin_records_json(&inventory.plugins),
        inventory.ghq.len(),
        render_ghq_records_json(&inventory.ghq),
        live_state.live.len(),
        render_live_state_json(live_state),
        tree_field,
        json_string_array(&warnings),
        result.fetch_calls,
        live_probe_calls
    )
}

fn render_plugin_records_json(records: &[DiscoverPluginRecord]) -> String {
    format!(
        "[{}]",
        records
            .iter()
            .map(|record| {
                format!(
                    "{{\"source\":\"plugin-registry\",\"type\":\"plugin\",\"name\":{},\"version\":{},\"kind\":{},\"tier\":{},\"weight\":{},\"disabled\":{},\"dir\":{},\"command\":{},\"aliases\":{},\"capabilities\":{},\"dependencies\":{}}}",
                    json_string(&record.name),
                    json_string(&record.version),
                    json_string(&record.kind),
                    json_string(&record.tier),
                    record.weight,
                    record.disabled,
                    json_string(&record.dir),
                    json_string(&record.command),
                    json_string_array(&record.aliases),
                    json_string_array(&record.capabilities),
                    json_string_array(&record.dependencies)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_ghq_records_json(records: &[GhqRepoRecord]) -> String {
    format!(
        "[{}]",
        records
            .iter()
            .map(|record| {
                format!(
                    "{{\"source\":\"ghq\",\"type\":\"repo\",\"path\":{},\"name\":{},\"owner\":{},\"host\":{},\"oracleLike\":{},\"worktree\":{}}}",
                    json_string(&record.path),
                    json_string(&record.name),
                    json_opt_string(record.owner.as_deref()),
                    json_opt_string(record.host.as_deref()),
                    record.oracle_like,
                    record.worktree
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_fleet_records_json(records: &[FleetConfigRecord]) -> String {
    format!(
        "[{}]",
        records
            .iter()
            .map(|record| {
                format!(
                    "{{\"source\":\"fleet-config\",\"type\":\"workspace\",\"file\":{},\"slot\":{},\"name\":{},\"session\":{},\"window\":{},\"repo\":{},\"node\":{},\"endpoint\":{},\"peerMatched\":{}}}",
                    json_string(&record.file),
                    json_string(&record.slot),
                    json_string(&record.name),
                    json_string(&record.session),
                    json_string(&record.window),
                    json_string(&record.repo),
                    json_string(&record.node),
                    json_opt_string(record.endpoint.as_deref()),
                    record.peer_matched
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

fn render_oracle_records_json(records: &[RegisteredOracleRecord]) -> String {
    format!(
        "[{}]",
        records
            .iter()
            .map(|record| {
                format!(
                    "{{\"source\":\"oracle-manifest\",\"type\":\"oracle\",\"name\":{},\"sources\":{},\"node\":{},\"session\":{},\"window\":{},\"repo\":{},\"localPath\":{},\"hasPsi\":{},\"hasFleetConfig\":{},\"awake\":{},\"ghqPath\":{},\"worktree\":{},\"fleetMatched\":{},\"peerUrls\":{}}}",
                    json_string(&record.name),
                    json_string_array(&record.sources),
                    json_opt_string(record.node.as_deref()),
                    json_opt_string(record.session.as_deref()),
                    json_opt_string(record.window.as_deref()),
                    json_opt_string(record.repo.as_deref()),
                    json_opt_string(record.local_path.as_deref()),
                    record.has_psi,
                    record.has_fleet_config,
                    record.awake,
                    json_opt_string(record.ghq_path.as_deref()),
                    record.worktree,
                    record.fleet_matched,
                    json_string_array(&record.peer_urls)
                )
            })
            .collect::<Vec<_>>()
            .join(",")
    )
}

