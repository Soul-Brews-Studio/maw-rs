
/// Deterministic plan input for maw-js `probePeer` runtime branches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbePeerPlan {
    pub url: String,
    pub now: String,
    pub dns_error: Option<ProbeLastError>,
    pub info: ProbeInfoOutcome,
    pub identity: Option<ProbeRemoteIdentity>,
}

/// Deterministic output for maw-js `probePeer`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbePeerResult {
    pub node: Option<String>,
    pub nickname: Option<String>,
    pub pubkey: Option<String>,
    pub identity: Option<PeerIdentity>,
    pub error: Option<ProbeLastError>,
}

/// Port of maw-js `probePeer` control flow over deterministic outcomes.
///
/// This deliberately stops short of real DNS/fetch IO; it locks the portable
/// branch behavior before the runtime adapter is wired.
#[must_use]
pub fn probe_peer_from_plan(plan: &ProbePeerPlan) -> ProbePeerResult {
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
    }
}

