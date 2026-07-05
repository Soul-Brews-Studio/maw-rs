use maw_peer::{
    probe_peer_from_plan, PeerIdentity, ProbeErrorCode, ProbeInfoBody, ProbeInfoOutcome,
    ProbeLastError, ProbeMawHandshake, ProbePeerPlan, ProbePeerResult, ProbeRemoteIdentity,
};

fn at() -> String {
    "2026-05-18T00:00:00.000Z".to_owned()
}

fn dns_error() -> ProbeLastError {
    ProbeLastError {
        code: ProbeErrorCode::Dns,
        message: "getaddrinfo ENOTFOUND missing.local".to_owned(),
        at: at(),
    }
}

#[test]
fn probe_peer_plan_returns_modern_identity_like_maw_js_probe_peer() {
    let result = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://peer.test:3456/some/path".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::SchemaObject("1".to_owned()),
            node: Some("peer-node".to_owned()),
            name: None,
            nickname: Some("Peer Nick".to_owned()),
        }),
        identity: Some(ProbeRemoteIdentity::Body {
            pubkey: Some("pub-123".to_owned()),
            oracle: Some("oracle-x".to_owned()),
            node: Some("peer-node".to_owned()),
        }),
    });

    assert_eq!(
        result,
        ProbePeerResult {
            node: Some("peer-node".to_owned()),
            nickname: Some("Peer Nick".to_owned()),
            pubkey: Some("pub-123".to_owned()),
            identity: Some(PeerIdentity {
                oracle: "oracle-x".to_owned(),
                node: "peer-node".to_owned(),
            }),
            error: None,
        }
    );
}

#[test]
fn probe_peer_plan_uses_legacy_name_and_default_oracle_identity() {
    let result = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::LegacyTrue,
            node: None,
            name: Some("legacy-name".to_owned()),
            nickname: Some(String::new()),
        }),
        identity: Some(ProbeRemoteIdentity::Body {
            pubkey: Some("pub-default".to_owned()),
            oracle: None,
            node: Some("legacy-name".to_owned()),
        }),
    });

    assert_eq!(
        result,
        ProbePeerResult {
            node: Some("legacy-name".to_owned()),
            nickname: None,
            pubkey: Some("pub-default".to_owned()),
            identity: Some(PeerIdentity {
                oracle: "mawjs".to_owned(),
                node: "legacy-name".to_owned(),
            }),
            error: None,
        }
    );
}

#[test]
fn probe_peer_plan_treats_blank_identity_fields_like_maw_js() {
    let result = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::LegacyTrue,
            node: Some("node-from-info".to_owned()),
            name: None,
            nickname: None,
        }),
        identity: Some(ProbeRemoteIdentity::Body {
            pubkey: Some(String::new()),
            oracle: Some(String::new()),
            node: Some("identity-node".to_owned()),
        }),
    });

    assert_eq!(result.pubkey, None);
    assert_eq!(
        result.identity,
        Some(PeerIdentity {
            oracle: "mawjs".to_owned(),
            node: "identity-node".to_owned(),
        })
    );
}

#[test]
fn probe_peer_plan_keeps_info_success_when_identity_is_absent_or_malformed() {
    let base = ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::LegacyTrue,
            node: Some("legacy".to_owned()),
            name: None,
            nickname: Some("Legacy Peer".to_owned()),
        }),
        identity: Some(ProbeRemoteIdentity::Missing),
    };

    assert_eq!(
        probe_peer_from_plan(&base),
        ProbePeerResult {
            node: Some("legacy".to_owned()),
            nickname: Some("Legacy Peer".to_owned()),
            pubkey: None,
            identity: None,
            error: None,
        }
    );

    let mut malformed = base;
    malformed.identity = Some(ProbeRemoteIdentity::MalformedJson);
    assert_eq!(
        probe_peer_from_plan(&malformed).node.as_deref(),
        Some("legacy")
    );
    assert_eq!(probe_peer_from_plan(&malformed).identity, None);
}
