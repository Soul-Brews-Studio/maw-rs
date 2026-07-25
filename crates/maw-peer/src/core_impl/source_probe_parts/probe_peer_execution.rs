
/// Deterministic plan input for maw-js `probePeer` runtime branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbePeerPlan {
    pub url: String,
    pub now: String,
    pub dns_error: Option<ProbeLastError>,
    pub info: ProbeInfoOutcome,
    pub identity: Option<ProbeRemoteIdentity>,
    /// The IP the peer URL's host resolved to, injected by the runtime probe
    /// (real DNS lives in the IO layer). `None` when resolution was skipped or
    /// failed. Feeds `loopback_self` so the map can catch the
    /// `m5.local → 127.0.0.1` trap (a probe that hit our own serve).
    pub resolved_ip: Option<String>,
    /// Result of a **read-only** gated GET against the peer, injected by the
    /// runtime probe. `None` = not attempted, `Some(true)` = the signed path is
    /// trusted, `Some(false)` = reachable but auth refused (e.g. 401). Never
    /// derived from `POST /api/send`, which would deliver a real message.
    pub auth_ok: Option<bool>,
}

/// Deterministic output for maw-js `probePeer`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProbePeerResult {
    pub node: Option<String>,
    pub nickname: Option<String>,
    pub pubkey: Option<String>,
    pub identity: Option<PeerIdentity>,
    pub error: Option<ProbeLastError>,
    /// Resolved IP carried through from the plan (see [`ProbePeerPlan`]).
    pub resolved_ip: Option<String>,
    /// Read-only auth outcome carried through from the plan.
    pub auth_ok: Option<bool>,
    /// `true` when `resolved_ip` is a loopback address — the probe reached our
    /// own serve, not the remote peer, so a `200 OK` here is a false "reachable".
    pub loopback_self: bool,
}

/// Is a resolved IP (or host) a loopback address? Mirrors
/// `maw_auth::is_loopback` without adding a cross-crate dependency to this leaf.
#[must_use]
pub fn is_loopback_ip(address: Option<&str>) -> bool {
    let Some(address) = address else {
        return false;
    };
    address == "127.0.0.1"
        || address == "::1"
        || address == "::ffff:127.0.0.1"
        || address == "localhost"
        || address.starts_with("127.")
}

/// Port of maw-js `probePeer` control flow over deterministic outcomes.
///
/// This deliberately stops short of real DNS/fetch IO; it locks the portable
/// branch behavior before the runtime adapter is wired.
#[must_use]
pub fn probe_peer_from_plan(plan: &ProbePeerPlan) -> ProbePeerResult {
    // Compute the base outcome, then overlay the runtime-injected truth on
    // EVERY path (including the early failure returns) so a DNS-resolved
    // loopback still flags `loopback_self` even when `/info` failed.
    let mut result = probe_peer_outcome(plan);
    result.resolved_ip.clone_from(&plan.resolved_ip);
    result.loopback_self = is_loopback_ip(plan.resolved_ip.as_deref());
    result.auth_ok = plan.auth_ok;
    result
}

fn probe_peer_outcome(plan: &ProbePeerPlan) -> ProbePeerResult {
    if let Some(err) = &plan.dns_error {
        return probe_failure(err.clone());
    }

    let body = match &plan.info {
        ProbeInfoOutcome::Body(body) => body,
        ProbeInfoOutcome::HttpStatus { status, ok } => {
            return probe_failure(ProbeLastError {
                code: classify_probe_error(&ProbeFailureInput::Http {
                    status: *status,
                    ok: *ok,
                }),
                message: format!("HTTP {status} from {}/info", plan.url),
                at: plan.now.clone(),
            });
        }
        ProbeInfoOutcome::InvalidJson => {
            return probe_bad_body("/info body was not valid JSON", &plan.now);
        }
        ProbeInfoOutcome::FetchCode { code, message } => {
            return probe_failure(ProbeLastError {
                code: classify_probe_error(&ProbeFailureInput::Code(code.clone())),
                message: message.clone(),
                at: plan.now.clone(),
            });
        }
        ProbeInfoOutcome::FetchCodeWithoutMessage { code } => {
            return probe_failure(ProbeLastError {
                code: classify_probe_error(&ProbeFailureInput::Code(code.clone())),
                message: format!("fetch {}/info failed", plan.url),
                at: plan.now.clone(),
            });
        }
        ProbeInfoOutcome::FetchName { name, message } => {
            return probe_failure(ProbeLastError {
                code: classify_probe_error(&ProbeFailureInput::Name(name.clone())),
                message: message.clone(),
                at: plan.now.clone(),
            });
        }
    };

    if !is_valid_maw_handshake(&body.maw) {
        return probe_bad_body(
            "/info response missing valid \"maw\" handshake field",
            &plan.now,
        );
    }

    let node = body
        .node
        .as_deref()
        .filter(|value| !value.is_empty())
        .or_else(|| body.name.as_deref().filter(|value| !value.is_empty()));
    let Some(node) = node else {
        return probe_bad_body(
            "/info response had neither \"node\" nor \"name\" string",
            &plan.now,
        );
    };

    let nickname = body
        .nickname
        .as_deref()
        .filter(|value| !value.is_empty())
        .map(str::to_owned);
    let identity_fields = plan.identity.as_ref().and_then(parse_remote_identity);

    ProbePeerResult {
        node: Some(node.to_owned()),
        nickname,
        pubkey: identity_fields
            .as_ref()
            .and_then(|fields| fields.pubkey.clone()),
        identity: identity_fields.and_then(|fields| fields.identity),
        error: None,
        ..Default::default()
    }
}


#[cfg(test)]
mod truth_probe_tests {
    use super::*;
    use crate::{ProbeInfoBody, ProbeInfoOutcome, ProbeMawHandshake};

    #[test]
    fn is_loopback_ip_matches_loopback_forms_only() {
        assert!(is_loopback_ip(Some("127.0.0.1")));
        assert!(is_loopback_ip(Some("127.1.2.3")));
        assert!(is_loopback_ip(Some("::1")));
        assert!(is_loopback_ip(Some("localhost")));
        assert!(!is_loopback_ip(Some("192.168.1.118")));
        assert!(!is_loopback_ip(Some("10.20.0.18")));
        assert!(!is_loopback_ip(None));
    }

    fn body_plan(resolved_ip: Option<&str>, auth_ok: Option<bool>) -> ProbePeerPlan {
        ProbePeerPlan {
            url: "http://peer.test:3456".to_owned(),
            now: "1700000000000".to_owned(),
            dns_error: None,
            info: ProbeInfoOutcome::Body(ProbeInfoBody {
                maw: ProbeMawHandshake::SchemaObject("1".to_owned()),
                node: Some("peer-node".to_owned()),
                name: None,
                nickname: None,
            }),
            identity: None,
            resolved_ip: resolved_ip.map(str::to_owned),
            auth_ok,
        }
    }

    #[test]
    fn overlay_flags_loopback_self_on_a_successful_probe() {
        // m5.local → 127.0.0.1 trap: /info succeeds (we reach our own serve),
        // but the resolved IP is loopback, so the map must not read it "reachable".
        let result = probe_peer_from_plan(&body_plan(Some("127.0.0.1"), Some(true)));
        assert!(result.error.is_none());
        assert_eq!(result.resolved_ip.as_deref(), Some("127.0.0.1"));
        assert!(result.loopback_self);
        assert_eq!(result.auth_ok, Some(true));
    }

    #[test]
    fn overlay_carries_auth_false_for_reachable_but_unsigned_peer() {
        // blackmachine: /info OK, LAN IP (not loopback), but the signed path 401s.
        let result = probe_peer_from_plan(&body_plan(Some("192.168.1.118"), Some(false)));
        assert!(!result.loopback_self);
        assert_eq!(result.auth_ok, Some(false));
        assert_eq!(result.resolved_ip.as_deref(), Some("192.168.1.118"));
    }

    #[test]
    fn overlay_applies_to_failure_paths_too() {
        // Even when /info fails we still surface the resolved loopback IP.
        let mut plan = body_plan(Some("127.0.0.1"), None);
        plan.info = ProbeInfoOutcome::InvalidJson;
        let result = probe_peer_from_plan(&plan);
        assert!(result.error.is_some());
        assert!(result.loopback_self);
        assert_eq!(result.resolved_ip.as_deref(), Some("127.0.0.1"));
    }
}
