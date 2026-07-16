//! Portable maw target-name matching primitives.
//!
//! This crate ports the pure matcher logic from maw-js:
//! `src/core/matcher/resolve-target.ts` and `normalize-target.ts`.
//! The behavioral contract is locked by the JSON fixtures copied from
//! maw-js `test/spec/*.fixtures.json`.

mod aliases;
mod fleet;
mod normalize;
mod numeric;
mod resolver;
mod typed_resolver;

pub use fleet::{resolve_fleet_window_session_target, FleetWindow, FleetWindowSessionLike};
pub use normalize::normalize_target;
pub use numeric::{resolve_numeric_fleet_stem_exact, resolve_numeric_fleet_stem_prefix};
pub use resolver::{
    resolve_by_name, resolve_session_target, resolve_worktree_target, Named, ResolveOptions,
    ResolveResult,
};
pub use typed_resolver::{
    normalized_match_names, resolve_typed_target, ResolveCandidateKind, ResolveMatch,
    ResolveMatchRank, ResolveTypedCandidate, ResolveTypedResult,
};

#[cfg(test)]
mod tests;
