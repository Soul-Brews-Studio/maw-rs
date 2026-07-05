
#[cfg(test)]
mod federation_sync_tests {
    use super::*;

    fn peer(peer_name: &str, node: &str, agents: &[&str], reachable: bool) -> PeerIdentity {
        PeerIdentity {
            peer_name: peer_name.to_owned(),
            url: format!("http://{peer_name}:3456"),
            node: node.to_owned(),
            agents: agents.iter().map(ToString::to_string).collect(),
            reachable,
            error: (!reachable).then(|| "stub".to_owned()),
        }
    }

    #[test]
    fn hosted_agents_includes_explicit_node_and_local_entries() {
        let mut agents = HashMap::new();
        agents.insert("pulse".to_owned(), "white".to_owned());
        agents.insert("mawjs".to_owned(), "white".to_owned());
        agents.insert("volt-colab-ml".to_owned(), "local".to_owned());
        agents.insert("homekeeper".to_owned(), "mba".to_owned());

        let mut hosted = hosted_agents(&agents, "white");
        hosted.sort();

        assert_eq!(hosted, ["mawjs", "pulse", "volt-colab-ml"]);
    }

    #[test]
    fn sync_diff_adds_new_oracles_but_preserves_local_routes() {
        let diff = compute_sync_diff(
            &HashMap::from([
                ("mawjs".to_owned(), "local".to_owned()),
                ("homekeeper".to_owned(), "mba".to_owned()),
            ]),
            &[peer(
                "white",
                "white",
                &["mawjs", "volt-colab-ml", "pulse"],
                true,
            )],
            "oracle-world",
        );
        let mut additions = diff
            .add
            .iter()
            .map(|add| add.oracle.clone())
            .collect::<Vec<_>>();
        additions.sort();

        assert_eq!(additions, ["pulse", "volt-colab-ml"]);
        assert!(diff.conflict.is_empty());
        assert!(diff.stale.is_empty());
        assert!(diff.unreachable.is_empty());
    }

    #[test]
    fn sync_diff_reports_conflict_when_foreign_route_claimed_elsewhere() {
        let diff = compute_sync_diff(
            &HashMap::from([("mawjs".to_owned(), "mba".to_owned())]),
            &[peer("white", "white", &["mawjs"], true)],
            "oracle-world",
        );

        assert_eq!(
            diff.conflict,
            vec![SyncConflict {
                oracle: "mawjs".to_owned(),
                current: "mba".to_owned(),
                proposed: "white".to_owned(),
                from_peer: "white".to_owned(),
            }]
        );
    }

    #[test]
    fn duplicate_oracle_claims_keep_first_peer_winner() {
        let diff = compute_sync_diff(
            &HashMap::new(),
            &[
                peer("white", "white", &["ghost"], true),
                peer("mba", "mba", &["ghost"], true),
            ],
            "oracle-world",
        );

        assert_eq!(diff.add.len(), 1);
        assert_eq!(diff.add[0].peer_node, "white");
        assert!(diff.conflict.is_empty());
    }

    #[test]
    fn stale_only_flags_reachable_peer_routes_and_skips_local() {
        let diff = compute_sync_diff(
            &HashMap::from([
                ("oldGuy".to_owned(), "white".to_owned()),
                ("localGuy".to_owned(), "oracle-world".to_owned()),
            ]),
            &[peer("white", "white", &["mawjs"], true)],
            "oracle-world",
        );

        assert_eq!(
            diff.stale,
            vec![StaleRoute {
                oracle: "oldGuy".to_owned(),
                peer_node: "white".to_owned(),
            }]
        );
    }

    #[test]
    fn unreachable_peers_are_tracked_but_not_marked_stale() {
        let diff = compute_sync_diff(
            &HashMap::from([("oldGuy".to_owned(), "mba".to_owned())]),
            &[peer("mba", "mba", &[], false)],
            "oracle-world",
        );

        assert!(diff.add.is_empty());
        assert!(diff.stale.is_empty());
        assert!(diff.conflict.is_empty());
        assert_eq!(diff.unreachable.len(), 1);
        assert_eq!(diff.unreachable[0].peer_name, "mba");
    }

    #[test]
    fn apply_sync_diff_adds_forces_and_prunes_when_requested() {
        let diff = SyncDiff {
            add: vec![SyncAdd {
                oracle: "pulse".to_owned(),
                peer_node: "white".to_owned(),
                from_peer: "white".to_owned(),
            }],
            conflict: vec![SyncConflict {
                oracle: "mawjs".to_owned(),
                current: "mba".to_owned(),
                proposed: "white".to_owned(),
                from_peer: "white".to_owned(),
            }],
            stale: vec![StaleRoute {
                oracle: "oldGuy".to_owned(),
                peer_node: "white".to_owned(),
            }],
            unreachable: Vec::new(),
        };

        let result = apply_sync_diff(
            &HashMap::from([
                ("mawjs".to_owned(), "mba".to_owned()),
                ("oldGuy".to_owned(), "white".to_owned()),
            ]),
            &diff,
            SyncApplyOptions {
                force: true,
                prune: true,
            },
        );

        assert_eq!(
            result.agents,
            HashMap::from([
                ("mawjs".to_owned(), "white".to_owned()),
                ("pulse".to_owned(), "white".to_owned()),
            ])
        );
        assert_eq!(result.applied.len(), 3);
    }

    #[test]
    fn duplicate_node_identity_keeps_first_peer_claims_only() {
        let diff = compute_sync_diff(
            &HashMap::new(),
            &[
                peer("white-a", "white", &["pulse"], true),
                peer("white-b", "white", &["ghost"], true),
            ],
            "oracle-world",
        );

        assert_eq!(diff.add.len(), 1);
        assert_eq!(diff.add[0].oracle, "pulse");
    }

}

/// Routing resolution result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult {
    Local {
        target: String,
    },
    Peer {
        peer_url: String,
        target: String,
        node: String,
    },
    SelfNode {
        target: String,
    },
    Error {
        reason: String,
        detail: String,
        hint: Option<String>,
    },
}

/// Resolve a user query to a local target, peer target, self-node target, or error.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn resolve_target(query: &str, config: &MawConfig, sessions: &[Session]) -> ResolveResult {
    resolve_target_with_current_session(query, config, sessions, None)
}
