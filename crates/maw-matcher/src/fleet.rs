use super::{aliases::*, resolver::{Named, ResolveResult}};

/// Window metadata used by [`resolve_fleet_window_session_target`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FleetWindow {
    pub name: Option<String>,
    pub repo: Option<String>,
}

/// Session metadata used by [`resolve_fleet_window_session_target`].
pub trait FleetWindowSessionLike: Named {
    fn windows(&self) -> &[FleetWindow];
}

/// Resolve fleet sessions from authoritative window/repo aliases.
#[must_use]
pub fn resolve_fleet_window_session_target<T>(target: &str, items: &[T]) -> ResolveResult<T>
where
    T: FleetWindowSessionLike + Clone,
{
    let lc = target.trim().to_lowercase();
    if lc.is_empty() {
        return ResolveResult::None { hints: None };
    }
    let lc_bare = strip_oracle_suffix_lower(&lc);

    let matches: Vec<T> = items
        .iter()
        .filter(|item| {
            let aliases = aliases_for(*item);
            aliases
                .iter()
                .any(|alias| alias == &lc || alias == &lc_bare)
        })
        .cloned()
        .collect();

    match matches.len() {
        0 => ResolveResult::None { hints: None },
        1 => ResolveResult::Fuzzy {
            matched: matches[0].clone(),
        },
        _ => ResolveResult::Ambiguous {
            candidates: matches,
        },
    }
}
