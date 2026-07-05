
#[test]
fn probe_peer_plan_returns_structured_failures_like_maw_js_probe_peer() {
    let dns = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://missing.local:3456".to_owned(),
        now: at(),
        dns_error: Some(dns_error()),
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::LegacyTrue,
            node: Some("never-fetched".to_owned()),
            name: None,
            nickname: None,
        }),
        identity: None,
    });
    assert_eq!(dns.node, None);
    assert_eq!(
        dns.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::Dns)
    );
    assert_eq!(
        dns.error.as_ref().map(|err| err.message.as_str()),
        Some("getaddrinfo ENOTFOUND missing.local")
    );

    let http = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::HttpStatus {
            status: 503,
            ok: false,
        },
        identity: None,
    });
    assert_eq!(
        http.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::Http5xx)
    );
    assert_eq!(
        http.error.as_ref().map(|err| err.message.as_str()),
        Some("HTTP 503 from http://127.0.0.1:3456/info")
    );

    let invalid_json = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::InvalidJson,
        identity: None,
    });
    assert_eq!(
        invalid_json.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::BadBody)
    );
    assert_eq!(
        invalid_json.error.as_ref().map(|err| err.message.as_str()),
        Some("/info body was not valid JSON")
    );

    let missing_maw = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::Missing,
            node: Some("not-maw".to_owned()),
            name: None,
            nickname: None,
        }),
        identity: None,
    });
    assert_eq!(
        missing_maw.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::BadBody)
    );
    assert_eq!(
        missing_maw.error.as_ref().map(|err| err.message.as_str()),
        Some("/info response missing valid \"maw\" handshake field")
    );

    let nameless = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::Body(ProbeInfoBody {
            maw: ProbeMawHandshake::LegacyTrue,
            node: None,
            name: None,
            nickname: Some("nameless".to_owned()),
        }),
        identity: None,
    });
    assert_eq!(
        nameless.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::BadBody)
    );
    assert_eq!(
        nameless.error.as_ref().map(|err| err.message.as_str()),
        Some("/info response had neither \"node\" nor \"name\" string")
    );
}

#[test]
fn probe_peer_plan_classifies_fetch_failures_with_context() {
    let refused = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::FetchCode {
            code: "ECONNREFUSED".to_owned(),
            message: "connect ECONNREFUSED".to_owned(),
        },
        identity: None,
    });
    assert_eq!(
        refused.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::Refused)
    );
    assert_eq!(
        refused.error.as_ref().map(|err| err.message.as_str()),
        Some("connect ECONNREFUSED")
    );

    let tls_non_error_throw = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::FetchCodeWithoutMessage {
            code: "UNABLE_TO_VERIFY_LEAF_SIGNATURE".to_owned(),
        },
        identity: None,
    });
    assert_eq!(
        tls_non_error_throw.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::Tls)
    );
    assert_eq!(
        tls_non_error_throw
            .error
            .as_ref()
            .map(|err| err.message.as_str()),
        Some("fetch http://127.0.0.1:3456/info failed")
    );

    let timeout_name = probe_peer_from_plan(&ProbePeerPlan {
        url: "http://127.0.0.1:3456".to_owned(),
        now: at(),
        dns_error: None,
        info: ProbeInfoOutcome::FetchName {
            name: "TimeoutError".to_owned(),
            message: "operation timed out".to_owned(),
        },
        identity: None,
    });
    assert_eq!(
        timeout_name.error.as_ref().map(|err| err.code),
        Some(ProbeErrorCode::Timeout)
    );
    assert_eq!(
        timeout_name.error.as_ref().map(|err| err.message.as_str()),
        Some("operation timed out")
    );
}
