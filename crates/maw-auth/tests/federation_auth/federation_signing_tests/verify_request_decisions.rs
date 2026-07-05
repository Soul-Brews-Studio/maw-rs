#[test]
fn verify_request_covers_o6_current_v3_decisions_and_malformed_cases() {
    assert_eq!(
        verify_req(Headers::new([] as [(&str, &str); 0]), b"", None),
        FromVerifyDecision::AcceptLegacy {
            reason: "no-cache-no-sig".to_owned()
        }
    );

    let signed = sign_headers_v3_at(PEER_KEY, FROM, "POST", "/api/send", Some(b"body"), NOW)
        .expect("v3 headers should sign");
    assert_eq!(
        verify_req(signed.clone(), b"body", None).kind(),
        "accept-tofu-record"
    );
    assert_eq!(
        verify_req(Headers::new([("x-maw-from", FROM)]), b"", Some(PEER_KEY)),
        FromVerifyDecision::RefuseUnsigned {
            reason: "cache-no-sig".to_owned(),
            from: Some(FROM.to_owned()),
        }
    );
    assert_eq!(
        verify_req(signed.clone(), b"body", Some(PEER_KEY)).kind(),
        "accept-verified"
    );
    assert_eq!(
        verify_req(signed.clone(), b"tampered", Some(PEER_KEY)).kind(),
        "refuse-mismatch"
    );
    assert_eq!(
        verify_req(
            Headers::new([
                ("X-Maw-From", FROM),
                (
                    "X-Maw-Signature-V3",
                    signed.get("x-maw-signature-v3").expect("sig")
                ),
                ("X-Maw-Timestamp", &(NOW - 301).to_string()),
            ]),
            b"body",
            Some(PEER_KEY),
        )
        .kind(),
        "refuse-skew"
    );
    assert_eq!(
        verify_req(
            Headers::new([
                ("x-maw-from", FROM),
                ("x-maw-signature-v3", &"0".repeat(64)),
                ("x-maw-timestamp", "nope"),
            ]),
            b"",
            Some(PEER_KEY),
        ),
        FromVerifyDecision::RefuseMalformed {
            reason: "invalid-timestamp".to_owned()
        }
    );
}

#[test]
fn verify_request_accepts_legacy_from_signing_and_identifies_refusals() {
    let iso = "2023-11-14T22:13:20.000Z";
    let legacy_payload =
        build_legacy_from_sign_payload(FROM, iso, "POST", "/api/send", &hash_body(Some(b"body")));
    let legacy_headers = Headers::new([
        ("x-maw-from", FROM),
        ("x-maw-signature", &direct_hmac(PEER_KEY, &legacy_payload)),
        ("x-maw-signed-at", iso),
    ]);
    let legacy = verify_req(legacy_headers, b"body", Some(PEER_KEY));
    assert_eq!(legacy.kind(), "accept-verified");
    assert!(!is_refuse_decision(&legacy));
    assert!(is_refuse_decision(&FromVerifyDecision::RefuseMismatch {
        reason: "signature-invalid".to_owned(),
        from: FROM.to_owned(),
    }));
}


#[test]
fn verify_request_accepts_loopback_real_ip_and_rejects_xff_spoof() {
    use maw_auth::{RequestAuthDecision, RequestAuthParts};
    use std::net::{IpAddr, Ipv4Addr};

    let loopback = maw_auth::verify_request(&RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new([] as [(&str, &str); 0]),
        body: None,
        peer_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        workspace_key: None,
        cached_pubkey: None,
        ed25519_pins: None,
        now: NOW,
    });
    assert_eq!(
        loopback,
        RequestAuthDecision::Accept {
            who: "loopback".to_owned()
        }
    );

    let spoof = maw_auth::verify_request(&RequestAuthParts {
        method: "POST".to_owned(),
        path: "/triggers/fire".to_owned(),
        headers: Headers::new([("x-forwarded-for", "127.0.0.1")]),
        body: None,
        peer_ip: Some(IpAddr::V4(Ipv4Addr::new(198, 51, 100, 10))),
        workspace_key: None,
        cached_pubkey: None,
        ed25519_pins: None,
        now: NOW,
    });
    assert_eq!(spoof.reason(), Some("missing-credentials"));
}

