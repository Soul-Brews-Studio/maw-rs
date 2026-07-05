
/// Port of maw-js `cmdProbeAll` over deterministic store/probe inputs.
#[must_use]
pub fn probe_all_from_plan(plan: &ProbeAllPlan) -> ProbeAllResult {
    let mut peers_after: BTreeMap<String, PeerRecord> = plan.peers.iter().cloned().collect();
    let probe_results: BTreeMap<String, (ProbePeerResult, u64)> = plan
        .probe_results
        .iter()
        .map(|(url, result, ms)| (url.clone(), (result.clone(), *ms)))
        .collect();
    let mut entries = plan.peers.clone();
    entries.sort_by(|(left, _), (right, _)| left.cmp(right));

    let mut probe_calls = Vec::with_capacity(entries.len());
    let mut rows = Vec::with_capacity(entries.len());
    for (alias, peer) in entries {
        probe_calls.push((peer.url.clone(), plan.timeout_ms));
        let (probe, ms) = probe_results
            .get(&peer.url)
            .cloned()
            .unwrap_or_else(|| (probe_failure_without_error(), 0));
        let error = probe.error.clone();
        rows.push(ProbeAllRow {
            alias,
            url: peer.url,
            node: probe.node.or(peer.node),
            last_seen: peer.last_seen,
            ok: error.is_none(),
            ms,
            error,
        });
    }

    let mutate_calls = usize::from(!rows.is_empty());
    if mutate_calls == 1 {
        for alias in &plan.removed_before_mutate {
            peers_after.remove(alias);
        }
        for row in &mut rows {
            let Some(peer) = peers_after.get_mut(&row.alias) else {
                continue;
            };
            if row.ok {
                peer.last_error = None;
                peer.last_seen = Some(plan.now.clone());
                row.last_seen = Some(plan.now.clone());
                if let Some(node) = &row.node {
                    peer.node = Some(node.clone());
                }
            } else if let Some(error) = &row.error {
                peer.last_error = Some(error.clone());
            }
        }
    }

    let ok_count = rows.iter().filter(|row| row.ok).count();
    let fail_count = rows.len() - ok_count;
    let worst_exit_code = rows
        .iter()
        .filter_map(|row| row.error.as_ref())
        .map(|err| probe_exit_code(err.code))
        .max()
        .unwrap_or(0);

    ProbeAllResult {
        rows,
        ok_count,
        fail_count,
        worst_exit_code,
        probe_calls,
        mutate_calls,
        peers_after,
    }
}

fn probe_failure_without_error() -> ProbePeerResult {
    ProbePeerResult {
        node: None,
        nickname: None,
        pubkey: None,
        identity: None,
        error: None,
    }
}

#[cfg(test)]
mod part03_coverage_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("maw-peer-part03-{name}-{nonce}"))
    }

    fn peer_record(url: &str) -> PeerRecord {
        PeerRecord {
            url: url.to_owned(),
            node: None,
            added_at: "2026-05-21T00:00:00Z".to_owned(),
            last_seen: None,
            last_error: None,
            nickname: None,
            pubkey: None,
            pubkey_first_seen: None,
            identity: None,
            one_way: None,
            last_symmetric_check: None,
        }
    }

    fn successful_probe(pubkey: Option<&str>) -> ProbePeerResult {
        ProbePeerResult {
            node: None,
            nickname: None,
            pubkey: pubkey.map(str::to_owned),
            identity: None,
            error: None,
        }
    }

    #[test]
    fn peer_add_existing_empty_pin_preserves_existing_metadata_edges() {
        let mut existing = peer_record("http://old:3456");
        existing.last_symmetric_check = Some("2026-05-20T00:00:00Z".to_owned());
        existing.one_way = Some(true);
        let plan = PeerAddPlan {
            alias: "white".to_owned(),
            url: "http://white:3456".to_owned(),
            node: Some("plan-node".to_owned()),
            authenticated_pubkey: None,
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::from([("white".to_owned(), existing)]),
            probe: successful_probe(Some("observed-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add succeeds");

        assert_eq!(result.peer.pubkey.as_deref(), Some("observed-key"));
        assert_eq!(
            result.peer.last_symmetric_check.as_deref(),
            Some("2026-05-20T00:00:00Z")
        );
        assert_eq!(result.peer.one_way, Some(true));
    }

    #[test]
    fn peer_store_error_paths_propagate_from_public_mutators() {
        let blocked_parent = temp_dir("blocked-parent");
        fs::write(&blocked_parent, "not a directory").expect("write blocked parent");
        let blocked_env = PeerStoreEnv::with_vars(
            temp_dir("blocked-home"),
            [(
                "PEERS_FILE",
                blocked_parent.join("peers.json").display().to_string(),
            )],
        );
        assert!(forget_peer_pubkey(&blocked_env, "white").is_err());
        let bootstrap = TofuDecision {
            kind: TofuDecisionKind::TofuBootstrap,
            alias: "white".to_owned(),
            cached: None,
            observed: Some("observed-key".to_owned()),
            message: String::new(),
        };
        assert!(apply_tofu_decision(&blocked_env, &bootstrap, "2026-05-21T00:00:00Z").is_err());
        let _ = fs::remove_file(&blocked_parent);

        let path = temp_dir("tmp-dir-peer-file");
        let tmp = tmp_peer_store_path(&path);
        fs::create_dir_all(&tmp).expect("create tmp directory");
        let env = PeerStoreEnv::with_vars(
            temp_dir("tmp-dir-home"),
            [("PEERS_FILE", path.display().to_string())],
        );
        assert!(save_peer_store(&env, &empty_peer_store()).is_err());
        let _ = fs::remove_dir_all(tmp);

        let stale_home = temp_dir("stale-one");
        let stale_env = PeerStoreEnv::new(&stale_home);
        let mut store = empty_peer_store();
        store
            .peers
            .insert("old".to_owned(), peer_record("http://old"));
        save_peer_store(&stale_env, &store).expect("save stale peer");
        let stale_path = peer_store_path(&stale_env);
        fs::create_dir_all(tmp_peer_store_path(&stale_path)).expect("block tmp write");
        assert!(remove_stale_peers(&stale_env, u64::MAX).is_err());
        let _ = fs::remove_dir_all(tmp_peer_store_path(&stale_path));
        let _ = remove_stale_peers(&stale_env, u64::MAX).expect("remove stale peer");
        let _ = fs::remove_dir_all(stale_home);
    }
}
