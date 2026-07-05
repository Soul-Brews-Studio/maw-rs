use super::resolver::{Named, ResolveResult};

/// Resolve a short prefix of the canonical stem in numbered fleet sessions.
///
/// The prefix must continue within the same word; `mawjs` does not match
/// `114-mawjs-no2` because the next character is a dash boundary.
#[must_use]
pub fn resolve_numeric_fleet_stem_prefix<T>(target: &str, items: &[T]) -> ResolveResult<T>
where
    T: Named + Clone,
{
    let lc = target.trim().to_lowercase();
    if lc.is_empty() {
        return ResolveResult::None { hints: None };
    }

    let matches: Vec<T> = items
        .iter()
        .filter(|item| {
            let name = item.name().to_lowercase();
            let Some(stem) = strip_numeric_fleet_prefix(&name) else {
                return false;
            };
            if !stem.starts_with(&lc) || stem.len() <= lc.len() {
                return false;
            }
            stem.as_bytes().get(lc.len()).copied() != Some(b'-')
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

/// Resolve an exact canonical stem in numbered fleet sessions.
#[must_use]
pub fn resolve_numeric_fleet_stem_exact<T>(target: &str, items: &[T]) -> ResolveResult<T>
where
    T: Named + Clone,
{
    let lc = target.trim().to_lowercase();
    if lc.is_empty() {
        return ResolveResult::None { hints: None };
    }

    let matches: Vec<T> = items
        .iter()
        .filter(|item| {
            let name = item.name().to_lowercase();
            strip_numeric_fleet_prefix(&name).is_some_and(|stem| stem == lc)
        })
        .cloned()
        .collect();

    match matches.len() {
        0 => ResolveResult::None { hints: None },
        1 => ResolveResult::Exact {
            matched: matches[0].clone(),
        },
        _ => ResolveResult::Ambiguous {
            candidates: matches,
        },
    }
}

pub(super) fn has_numeric_fleet_prefix(name: &str) -> bool {
    strip_numeric_fleet_prefix(name).is_some()
}

pub(super) fn strip_numeric_fleet_prefix(name: &str) -> Option<&str> {
    let (prefix, rest) = name.split_once('-')?;
    if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}
