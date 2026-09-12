#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
//! `effective_tier` is the one shared answer for "what tier is this plugin?" (#792).

use maw_plugin_manifest::{effective_tier, parse_manifest, weight_to_tier, PluginTier};
use std::path::Path;

fn manifest(json_text: &str) -> maw_plugin_manifest::PluginManifest {
    parse_manifest(json_text, Path::new("/tmp/maw-rs-tier-policy")).expect("valid manifest")
}

#[test]
fn weight_to_tier_matches_the_portable_maw_policy_thresholds() {
    assert_eq!(weight_to_tier(0), PluginTier::Core);
    assert_eq!(weight_to_tier(9), PluginTier::Core);
    assert_eq!(weight_to_tier(10), PluginTier::Standard);
    assert_eq!(weight_to_tier(49), PluginTier::Standard);
    assert_eq!(weight_to_tier(50), PluginTier::Extra);
    assert_eq!(weight_to_tier(100), PluginTier::Extra);
    // Manifest weights are u64; anything past i32::MAX still saturates into extra.
    assert_eq!(weight_to_tier(u64::MAX), PluginTier::Extra);
}

#[test]
fn effective_tier_prefers_the_explicit_tier_field_over_weight() {
    let parsed = manifest(
        r#"{ "name": "explicit", "version": "1.0.0", "sdk": "*", "tier": "core", "weight": 90 }"#,
    );
    assert_eq!(parsed.tier, Some(PluginTier::Core));
    assert_eq!(effective_tier(&parsed), PluginTier::Core);
}

#[test]
fn effective_tier_falls_back_to_weight_when_the_manifest_omits_tier() {
    let core = manifest(r#"{ "name": "c", "version": "1.0.0", "sdk": "*", "weight": 5 }"#);
    assert_eq!(core.tier, None);
    assert_eq!(effective_tier(&core), PluginTier::Core);

    let standard = manifest(r#"{ "name": "s", "version": "1.0.0", "sdk": "*", "weight": 10 }"#);
    assert_eq!(effective_tier(&standard), PluginTier::Standard);

    // The #792 repro: `"weight": 50` with no `"tier"` key is extra, never core.
    let extra = manifest(r#"{ "name": "e", "version": "1.0.0", "sdk": "*", "weight": 50 }"#);
    assert_eq!(effective_tier(&extra), PluginTier::Extra);
}

#[test]
fn effective_tier_assumes_the_default_weight_when_both_fields_are_absent() {
    let bare = manifest(r#"{ "name": "bare", "version": "1.0.0", "sdk": "*" }"#);
    assert_eq!(bare.tier, None);
    assert_eq!(bare.weight, None);
    assert_eq!(
        effective_tier(&bare),
        weight_to_tier(maw_plugin_manifest::DEFAULT_PLUGIN_WEIGHT)
    );
    assert_eq!(effective_tier(&bare), PluginTier::Extra);
}
