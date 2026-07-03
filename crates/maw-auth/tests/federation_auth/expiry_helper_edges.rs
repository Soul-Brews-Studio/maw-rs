#[test]
fn consent_expiry_parses_iso_fraction_and_calendar_edges() {
    use maw_auth::{apply_consent_expiry, ConsentStatus};

    for (expires_at, now_ms) in [
        ("2026-01-02T00:01:00Z", 1_767_312_061_000),
        ("2026-01-02T00:01:00.1Z", 1_767_312_060_101),
        ("2026-01-02T00:01:00.12Z", 1_767_312_060_121),
        ("2024-02-29T00:00:00Z", 1_709_164_801_000),
        ("2023-02-28T23:59:59Z", 1_677_628_800_000),
        ("1969-12-31T23:59:59Z", 0),
        ("0000-01-01T00:00:00Z", 1),
    ] {
        assert_eq!(
            apply_consent_expiry(&pending_with_expires_at(expires_at), now_ms).status,
            ConsentStatus::Expired,
            "timestamp {expires_at} should expire"
        );
    }
}

#[test]
fn consent_expiry_ignores_invalid_iso_shapes() {
    use maw_auth::{apply_consent_expiry, ConsentStatus};

    for invalid in [
        "2026-01-02-extraT00:01:00Z",
        "2026-13-02T00:01:00Z",
        "2026-01-32T00:01:00Z",
        "2026-01-02T24:01:00Z",
        "2026-01-02T00:01:00.aZ",
        "",
        "not-a-date",
        "nopeT00:01:00Z",
        "2026-nope-02T00:01:00Z",
        "2026-01-nopeT00:01:00Z",
        "2026-01-02Tbad:01:00Z",
        "2026-01-02T00:bad:00Z",
        "2026-01-02T00:01Z",
        "2026T00:01:00Z",
        "2026-01T00:01:00Z",
        "2026-01-02T",
        "2026-01-02T00",
        "2026-01-02T00:01:nopeZ",
    ] {
        assert_eq!(
            apply_consent_expiry(&pending_with_expires_at(invalid), i64::MAX).status,
            ConsentStatus::Pending,
            "invalid timestamp {invalid} must not expire"
        );
    }
}

#[test]
fn auth_public_helpers_cover_map_conversion_status_names_and_validation_rejections() {
    use maw_auth::{trust_key, ConsentAction};

    let headers = Headers::new([("X-Test", "one")]);
    let as_map = headers.to_btree_map();
    assert_eq!(as_map.get("x-test").map(String::as_str), Some("one"));

    assert_eq!(
        trust_key("a", "b", ConsentAction::PluginInstall),
        "a→b:plugin-install"
    );
    assert_eq!(
        verify_req(Headers::new([] as [(&str, &str); 0]), b"", None).kind(),
        "accept-legacy"
    );
    assert_eq!(
        verify_req(
            Headers::new([
                ("x-maw-from", FROM),
                ("x-maw-signature", &"0".repeat(64)),
                ("x-maw-signed-at", "not-an-iso-date"),
            ]),
            b"",
            Some(PEER_KEY),
        )
        .kind(),
        "refuse-malformed"
    );
    assert!(!verify_hmac_sig(TOKEN, "payload", ""));
    assert!(!verify_hmac_sig(TOKEN, "payload", "00"));
}
