
#[cfg(test)]
mod federation_symmetric_tests {
    use super::*;

    fn base(peers: Vec<FederationPeerStatus>) -> FederationStatus {
        FederationStatus {
            local_url: "http://localhost:3456".to_owned(),
            peers,
        }
    }

    fn peer(url: &str, reachable: bool, node: Option<&str>) -> FederationPeerStatus {
        FederationPeerStatus {
            url: url.to_owned(),
            node: node.map(str::to_owned),
            reachable,
            latency: Some(40),
            agents: Vec::new(),
            clock_warning: false,
        }
    }

    fn remote(peers: Vec<FederationPeerView>) -> PeerFederationStatusResult {
        PeerFederationStatusResult::Ok(PeerFederationStatus { peers })
    }

    fn view(url: &str, node: Option<&str>, reachable: bool) -> FederationPeerView {
        FederationPeerView {
            url: Some(url.to_owned()),
            node: node.map(str::to_owned),
            reachable: Some(reachable),
        }
    }

    #[test]
    fn no_peers_reports_empty_pair_counts() {
        let status = classify_symmetric_federation_status(&base(Vec::new()), &[], "white");

        assert!(status.pairs.is_empty());
        assert_eq!(status.total_pairs, 0);
        assert_eq!(status.healthy_pairs, 0);
        assert_eq!(status.local_node, "white");
    }

    #[test]
    fn reachable_peer_with_reachable_reverse_view_is_healthy() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://localhost:3456", Some("white"), true)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Healthy);
        assert!(status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, Some(true));
        assert_eq!(status.healthy_pairs, 1);
    }

    #[test]
    fn reachable_peer_missing_local_view_is_half_up() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[("http://mba:3456".to_owned(), remote(Vec::new()))],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::HalfUp);
        assert!(status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, Some(false));
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("not in peer")));
    }

    #[test]
    fn reachable_peer_marking_local_unreachable_is_half_up() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://localhost:3456", Some("white"), false)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::HalfUp);
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("unreachable")));
    }

    #[test]
    fn forward_unreachable_peer_is_down_without_reverse_check() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", false, None)]),
            &[],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Down);
        assert!(!status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, None);
        assert_eq!(
            status.pairs[0].reason.as_deref(),
            Some("forward unreachable")
        );
    }

    #[test]
    fn non_ok_peer_status_is_unknown() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                PeerFederationStatusResult::HttpStatus(500),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Unknown);
        assert!(status.pairs[0].forward);
        assert_eq!(status.pairs[0].reverse, None);
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("returned 500")));
    }

    #[test]
    fn fetch_error_peer_status_is_unknown_with_reason() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                PeerFederationStatusResult::FetchError("network cable unplugged".to_owned()),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Unknown);
        assert!(status.pairs[0]
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("network cable")));
    }

    #[test]
    fn local_node_identity_match_wins_when_url_differs() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://10.0.0.1:3456", Some("white"), true)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Healthy);
    }

    #[test]
    fn local_url_match_supports_legacy_peer_without_node_identity() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, None)]),
            &[(
                "http://mba:3456".to_owned(),
                remote(vec![view("http://localhost:3456", None, true)]),
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::Healthy);
    }

    #[test]
    fn mixed_three_peer_mesh_counts_one_healthy_one_half_up_one_down() {
        let status = classify_symmetric_federation_status(
            &base(vec![
                peer("http://alpha:3456", true, Some("alpha")),
                peer("http://bravo:3456", true, Some("bravo")),
                peer("http://charlie:3456", false, None),
            ]),
            &[
                (
                    "http://alpha:3456".to_owned(),
                    remote(vec![view("http://localhost:3456", Some("white"), true)]),
                ),
                ("http://bravo:3456".to_owned(), remote(Vec::new())),
            ],
            "white",
        );
        let mut pair_healths = status
            .pairs
            .iter()
            .map(|pair| pair.pair)
            .collect::<Vec<_>>();
        pair_healths.sort_unstable_by_key(|state| state.as_str());

        assert_eq!(status.total_pairs, 3);
        assert_eq!(status.healthy_pairs, 1);
        assert_eq!(
            pair_healths,
            vec![PairHealth::Down, PairHealth::HalfUp, PairHealth::Healthy]
        );
    }

    #[test]
    fn peer_response_without_peers_field_is_half_up_defensively() {
        let status = classify_symmetric_federation_status(
            &base(vec![peer("http://mba:3456", true, Some("mba"))]),
            &[(
                "http://mba:3456".to_owned(),
                PeerFederationStatusResult::MissingPeers,
            )],
            "white",
        );

        assert_eq!(status.pairs[0].pair, PairHealth::HalfUp);
    }
}
