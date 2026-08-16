// Effective plugin tier resolution — the single shared answer to
// "what tier is this plugin?" (#792).
//
// Ported from maw-js `src/commands/shared/plugins-ls-info.ts` `effectiveTier`
// and `src/api/plugin-list-manifest.ts` `toPeerPluginEntry`, which both spell
// the same rule (#675):
//
//     p.manifest.tier ?? weightToTier(p.manifest.weight ?? 50)
//
// The weight thresholds themselves are NOT redefined here: they live in the
// portable, fixture-locked `maw_policy::weight_to_tier`. This module owns only
// the two fallbacks (explicit `tier` first, then `weight`, then
// `DEFAULT_PLUGIN_WEIGHT`) and the crossing between the two `PluginTier` enums.
//
// `maw_policy::DEFAULT_TIER` is a *constants* export (`maw policy --constants`),
// not a fallback for an absent manifest tier — maw-js never uses it that way.

/// Weight assumed when a manifest omits `weight` (maw-js `m.weight ?? 50`).
pub const DEFAULT_PLUGIN_WEIGHT: u64 = 50;

/// Map a manifest weight onto its tier using the portable `maw_policy` thresholds.
///
/// Manifest weights are unsigned; weights beyond `i32::MAX` saturate, which keeps
/// them in `Extra` exactly like every other large weight.
#[must_use]
pub fn weight_to_tier(weight: u64) -> PluginTier {
    match maw_policy::weight_to_tier(i32::try_from(weight).unwrap_or(i32::MAX)) {
        maw_policy::PluginTier::Core => PluginTier::Core,
        maw_policy::PluginTier::Standard => PluginTier::Standard,
        maw_policy::PluginTier::Extra => PluginTier::Extra,
    }
}

/// Resolve the tier a plugin is actually treated as: the explicit `tier` field
/// when the manifest declares one, otherwise derived from `weight` (#675).
///
/// Every surface that reports or gates on a plugin's tier must call this —
/// `plugin info`, `plugin ls`, `plugins ls/info/profile`, and the discovery
/// active-profile filter — so a manifest that omits `tier` reads the same
/// everywhere (#792).
#[must_use]
pub fn effective_tier(manifest: &PluginManifest) -> PluginTier {
    manifest
        .tier
        .unwrap_or_else(|| weight_to_tier(manifest.weight.unwrap_or(DEFAULT_PLUGIN_WEIGHT)))
}
