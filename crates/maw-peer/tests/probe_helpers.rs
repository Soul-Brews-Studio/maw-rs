// Ported from maw-js test/isolated/pair-probe-coverage.test.ts classifier and formatting cases.

use maw_peer::{
    classify_probe_error, format_probe_error, is_valid_maw_handshake, pick_probe_hint,
    probe_exit_code, ProbeErrorCode, ProbeFailureInput, ProbeLastError, ProbeMawHandshake,
};

#[test]
fn probe_classifier_matches_maw_js_error_buckets() {
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Http {
            status: 404,
            ok: false
        }),
        ProbeErrorCode::Http4xx
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Http {
            status: 503,
            ok: false
        }),
        ProbeErrorCode::Http5xx
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::CauseCode("EAI_AGAIN".to_owned())),
        ProbeErrorCode::Dns
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code("ConnectionRefused".to_owned())),
        ProbeErrorCode::Refused
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Name("AbortError".to_owned())),
        ProbeErrorCode::Timeout
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code("CERT_HAS_EXPIRED".to_owned())),
        ProbeErrorCode::Tls
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code(
            "SELF_SIGNED_CERT_IN_CHAIN".to_owned()
        )),
        ProbeErrorCode::Tls
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code("WEIRD".to_owned())),
        ProbeErrorCode::Unknown
    );
    // #733: EHOSTUNREACH/ENETUNREACH classify as Unreachable, not UNKNOWN —
    // EHOSTUNREACH is the macOS Local Network (TCC) denial signature.
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code("EHOSTUNREACH".to_owned())),
        ProbeErrorCode::Unreachable
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code("ENETUNREACH".to_owned())),
        ProbeErrorCode::Unreachable
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code(
            "HostUnreachable".to_owned()
        )),
        ProbeErrorCode::Unreachable
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::NonObject),
        ProbeErrorCode::Unknown
    );
    assert_eq!(probe_exit_code(ProbeErrorCode::Timeout), 5);
}

#[test]
fn probe_codes_hints_and_hosts_cover_all_maw_js_buckets() {
    use maw_peer::{probe_hint, safe_probe_host};

    for (code, name, exit, hint_part) in [
        (ProbeErrorCode::Dns, "DNS", 3, "Host does not resolve"),
        (ProbeErrorCode::Refused, "REFUSED", 4, "port is closed"),
        (ProbeErrorCode::Timeout, "TIMEOUT", 5, "within 2s"),
        (ProbeErrorCode::Unreachable, "UNREACHABLE", 7, "No route"),
        (ProbeErrorCode::Http4xx, "HTTP_4XX", 6, "client error"),
        (ProbeErrorCode::Http5xx, "HTTP_5XX", 6, "server error"),
        (ProbeErrorCode::Tls, "TLS", 2, "TLS handshake"),
        (ProbeErrorCode::BadBody, "BAD_BODY", 2, "body shape"),
        (ProbeErrorCode::Unknown, "UNKNOWN", 2, "unclassified"),
    ] {
        assert_eq!(code.as_str(), name);
        assert_eq!(probe_exit_code(code), exit);
        assert!(probe_hint(code).contains(hint_part), "{code:?}");
    }

    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Code(
            "UND_ERR_CONNECT_TIMEOUT".to_owned()
        )),
        ProbeErrorCode::Timeout
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Name("TimeoutError".to_owned())),
        ProbeErrorCode::Timeout
    );
    assert_eq!(
        classify_probe_error(&ProbeFailureInput::Http {
            status: 204,
            ok: true
        }),
        ProbeErrorCode::Unknown
    );

    assert_eq!(safe_probe_host("http://"), "http://");
}

#[test]
fn probe_handshake_validation_matches_maw_js_shapes() {
    assert!(is_valid_maw_handshake(&ProbeMawHandshake::LegacyTrue));
    assert!(is_valid_maw_handshake(&ProbeMawHandshake::SchemaObject(
        "1".to_owned()
    )));
    assert!(!is_valid_maw_handshake(&ProbeMawHandshake::EmptyObject));
    assert!(!is_valid_maw_handshake(&ProbeMawHandshake::OtherTruthy));
    assert!(!is_valid_maw_handshake(&ProbeMawHandshake::Missing));
}

#[test]
fn probe_hints_and_formatting_match_actionable_maw_js_contract() {
    let mdns = ProbeLastError {
        code: ProbeErrorCode::Dns,
        message: "query ENOTIMP white.local".to_owned(),
        at: "now".to_owned(),
    };
    assert!(pick_probe_hint(&mdns).contains("avahi-daemon"));

    let unknown = ProbeLastError {
        code: ProbeErrorCode::Unknown,
        message: "weird".to_owned(),
        at: "now".to_owned(),
    };
    assert_eq!(
        pick_probe_hint(&unknown),
        "Probe failed for an unclassified reason."
    );

    let formatted = format_probe_error(&mdns, "http://white.local:3456/base", "white");
    assert!(formatted.contains("peer handshake failed"), "{formatted}");
    assert!(formatted.contains("host: white.local:3456"), "{formatted}");
    assert!(
        formatted.contains("retry: maw peers probe white"),
        "{formatted}"
    );
    assert!(format_probe_error(&mdns, "not a url", "bad").contains("host: not a url"));
}
