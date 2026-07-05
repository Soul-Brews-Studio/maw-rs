// Ported from maw-js `test/scout-pair-proof.test.ts` and
// `src/transports/scout-pair-proof.ts`.
#[test]
fn auto_pair_proofs_sign_stable_canonical_identity_fields() {
    use maw_auth::{sign_auto_pair_proof, AutoPairIdentity};

    let identity = AutoPairIdentity {
        node: "m5".to_owned(),
        oracle: "mawjs".to_owned(),
        url: "http://m5.local:3456".to_owned(),
        pubkey: "pub-abc".to_owned(),
    };

    let proof = sign_auto_pair_proof(&identity, "token-a");
    assert_eq!(proof.len(), 64);
    assert!(proof.chars().all(|ch| ch.is_ascii_hexdigit()));
    assert_eq!(sign_auto_pair_proof(&identity.clone(), "token-a"), proof);
    assert_ne!(
        sign_auto_pair_proof(
            &AutoPairIdentity {
                node: "other".to_owned(),
                ..identity
            },
            "token-a",
        ),
        proof
    );
}

// Ported from maw-js `test/scout-pair-proof.test.ts` and
// `src/transports/scout-pair-proof.ts`.
#[test]
fn auto_pair_proofs_verify_valid_proofs_and_reject_wrong_inputs() {
    use maw_auth::{sign_auto_pair_proof, verify_auto_pair_proof, AutoPairIdentity};

    let identity = AutoPairIdentity {
        node: "m5".to_owned(),
        oracle: "mawjs".to_owned(),
        url: "http://m5.local:3456".to_owned(),
        pubkey: "pub-abc".to_owned(),
    };
    let proof = sign_auto_pair_proof(&identity, "token-a");

    assert!(verify_auto_pair_proof(&identity, "token-a", &proof));
    assert!(!verify_auto_pair_proof(&identity, "token-b", &proof));
    assert!(!verify_auto_pair_proof(
        &AutoPairIdentity {
            pubkey: "pub-other".to_owned(),
            ..identity.clone()
        },
        "token-a",
        &proof,
    ));
    assert!(!verify_auto_pair_proof(&identity, "token-a", &proof[2..]));
    assert!(!verify_auto_pair_proof(
        &identity,
        "token-a",
        &"z".repeat(64)
    ));
}

// Ported from maw-js `src/lib/pair-codes.ts` and `test/pair-api-default.test.ts`.
#[test]
fn pair_code_helpers_match_maw_js_shape_format_and_redaction() {
    use maw_auth::{
        generate_pair_code_from_bytes, is_valid_pair_code_shape, normalize_pair_code,
        pretty_pair_code, redact_pair_code, PAIR_CODE_ALPHABET,
    };

    assert_eq!(normalize_pair_code("abc-234"), "ABC234");
    assert_eq!(normalize_pair_code(" ab c-2 34\n"), "ABC234");
    assert!(is_valid_pair_code_shape("ABC-234"));
    assert!(is_valid_pair_code_shape("abc234"));
    assert!(!is_valid_pair_code_shape("ABCDE"));
    assert!(!is_valid_pair_code_shape("ABCDEFG"));
    assert!(!is_valid_pair_code_shape("ABCDE0"));
    assert!(!is_valid_pair_code_shape("ABCDE1"));
    assert!(!is_valid_pair_code_shape("ABCDEI"));
    assert!(!is_valid_pair_code_shape("ABCDEO"));

    assert_eq!(pretty_pair_code("abc234"), "ABC-234");
    assert_eq!(pretty_pair_code("bad"), "BAD");
    assert_eq!(redact_pair_code("abc234"), "ABC-***");
    assert_eq!(redact_pair_code("ab"), "***");

    let code = generate_pair_code_from_bytes(&[0, 1, 31, 32, 33, 255]);
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|ch| PAIR_CODE_ALPHABET.contains(ch)));
    assert_eq!(code, "AB9AB9");
}

// Ported from maw-js `src/lib/pair-codes.ts` and `test/pair-api-default.test.ts`.
#[test]
fn pair_code_store_register_lookup_and_consume_match_maw_js_ttl_contract() {
    use maw_auth::{LookupResult, PairCodeStore};

    let mut store = PairCodeStore::default();
    let entry = store.register_at("abc-234", 120_000, 1_000_000);
    assert_eq!(entry.code, "ABC234");
    assert_eq!(entry.created_at, 1_000_000);
    assert_eq!(entry.expires_at, 1_120_000);
    assert!(!entry.consumed);

    assert_eq!(
        store.lookup_at("ABC234", 1_000_000),
        LookupResult::Live(entry)
    );
    assert_eq!(store.lookup_at("ZZZ999", 1_000_000), LookupResult::NotFound);
    assert_eq!(store.lookup_at("ABC234", 1_120_001), LookupResult::Expired);

    let consumed = store.consume_at("abc 234", 1_000_001);
    assert!(matches!(consumed, LookupResult::Live(_)));
    assert_eq!(
        store.lookup_at("ABC-234", 1_000_002),
        LookupResult::Consumed
    );
    assert_eq!(
        store.consume_at("ABC234", 1_000_003),
        LookupResult::Consumed
    );
}

