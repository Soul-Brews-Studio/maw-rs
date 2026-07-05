use super::numeric::has_numeric_fleet_prefix;

/// Name-shaped candidate accepted by the generic resolver.
pub trait Named {
    fn name(&self) -> &str;
}

impl Named for String {
    fn name(&self) -> &str {
        self
    }
}

impl Named for str {
    fn name(&self) -> &str {
        self
    }
}

impl Named for &str {
    fn name(&self) -> &str {
        self
    }
}

/// Matcher result equivalent to maw-js `ResolveResult<T>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveResult<T> {
    None { hints: Option<Vec<T>> },
    Exact { matched: T },
    Fuzzy { matched: T },
    Ambiguous { candidates: Vec<T> },
}

/// Options for [`resolve_by_name`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResolveOptions {
    /// When true, prefix/middle matching excludes numeric fleet sessions (`NN-*`).
    pub fleet_sessions: bool,
}

/// Resolve a bare user-typed name against name-shaped items.
///
/// Match ladder:
/// 1. case-insensitive exact
/// 2. suffix segment (`*-target`), preferred
/// 3. prefix/middle segment (`target-*` or `*-target-*`)
/// 4. substring hints only (`kind: none` equivalent)
#[must_use]
pub fn resolve_by_name<T>(target: &str, items: &[T], options: ResolveOptions) -> ResolveResult<T>
where
    T: Named + Clone,
{
    let lc = target.trim().to_lowercase();
    if lc.is_empty() {
        return ResolveResult::None { hints: None };
    }

    if let Some(exact) = items.iter().find(|item| item.name().to_lowercase() == lc) {
        return ResolveResult::Exact {
            matched: exact.clone(),
        };
    }

    let suffix: Vec<T> = items
        .iter()
        .filter(|item| item.name().to_lowercase().ends_with(&format!("-{lc}")))
        .cloned()
        .collect();
    match suffix.len() {
        0 => {}
        1 => {
            return ResolveResult::Fuzzy {
                matched: suffix[0].clone(),
            }
        }
        _ => return ResolveResult::Ambiguous { candidates: suffix },
    }

    let prefix = format!("{lc}-");
    let middle = format!("-{lc}-");
    let prefix_or_mid: Vec<T> = items
        .iter()
        .filter(|item| {
            let name = item.name().to_lowercase();
            if options.fleet_sessions && has_numeric_fleet_prefix(&name) {
                return false;
            }
            name.starts_with(&prefix) || name.contains(&middle)
        })
        .cloned()
        .collect();
    match prefix_or_mid.len() {
        0 => {}
        1 => {
            return ResolveResult::Fuzzy {
                matched: prefix_or_mid[0].clone(),
            }
        }
        _ => {
            return ResolveResult::Ambiguous {
                candidates: prefix_or_mid,
            }
        }
    }

    let hints: Vec<T> = items
        .iter()
        .filter(|item| item.name().to_lowercase().contains(&lc))
        .cloned()
        .collect();
    if hints.is_empty() {
        ResolveResult::None { hints: None }
    } else {
        ResolveResult::None { hints: Some(hints) }
    }
}

/// Session target resolver. Numeric fleet sessions opt out of prefix/middle matches.
#[must_use]
pub fn resolve_session_target<T>(target: &str, items: &[T]) -> ResolveResult<T>
where
    T: Named + Clone,
{
    resolve_by_name(
        target,
        items,
        ResolveOptions {
            fleet_sessions: true,
        },
    )
}

/// Worktree target resolver. Numeric prefixes are sequence counters, so middle matching remains enabled.
#[must_use]
pub fn resolve_worktree_target<T>(target: &str, items: &[T]) -> ResolveResult<T>
where
    T: Named + Clone,
{
    resolve_by_name(target, items, ResolveOptions::default())
}
