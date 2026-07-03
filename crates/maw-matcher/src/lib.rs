//! Portable maw target-name matching primitives.
//!
//! This crate ports the pure matcher logic from maw-js:
//! `src/core/matcher/resolve-target.ts` and `normalize-target.ts`.
//! The behavioral contract is locked by the JSON fixtures copied from
//! maw-js `test/spec/*.fixtures.json`.

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

/// Normalize user-typed target names by trimming and removing trailing `/` and `/.git`.
#[must_use]
pub fn normalize_target(raw: &str) -> String {
    let mut s = raw.trim().to_owned();
    if s.is_empty() {
        return s;
    }

    loop {
        let previous = s.clone();
        while s.ends_with('/') {
            s.pop();
        }
        if s.ends_with("/.git") {
            let new_len = s.len() - "/.git".len();
            s.truncate(new_len);
        }
        if s == previous {
            break;
        }
    }

    s
}

fn has_numeric_fleet_prefix(name: &str) -> bool {
    strip_numeric_fleet_prefix(name).is_some()
}

fn strip_numeric_fleet_prefix(name: &str) -> Option<&str> {
    let (prefix, rest) = name.split_once('-')?;
    if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
        Some(rest)
    } else {
        None
    }
}

fn strip_oracle_suffix_lower(name: &str) -> String {
    name.strip_suffix("-oracle").unwrap_or(name).to_owned()
}

fn repo_basename_lower(repo: &str) -> String {
    repo.rsplit('/').next().unwrap_or(repo).to_lowercase()
}

fn aliases_for<T>(item: &T) -> Vec<String>
where
    T: FleetWindowSessionLike,
{
    let mut aliases = Vec::new();
    for window in item.windows() {
        if let Some(win) = window
            .name
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let win = win.to_lowercase();
            aliases.push(win.clone());
            aliases.push(strip_oracle_suffix_lower(&win));
        }
        if let Some(repo) = window
            .repo
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let base = repo_basename_lower(repo);
            aliases.push(base.clone());
            aliases.push(strip_oracle_suffix_lower(&base));
        }
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct Session {
        name: String,
        windows: Vec<FleetWindow>,
    }

    impl Named for Session {
        fn name(&self) -> &str {
            &self.name
        }
    }

    impl FleetWindowSessionLike for Session {
        fn windows(&self) -> &[FleetWindow] {
            &self.windows
        }
    }

    fn session(name: &str) -> Session {
        Session {
            name: name.to_owned(),
            windows: Vec::new(),
        }
    }

    #[test]
    fn numeric_fleet_prefix_helper_preserves_dash_boundary_rule() {
        assert_eq!(
            resolve_numeric_fleet_stem_prefix("homeke", &[session("20-homekeeper")]),
            ResolveResult::Fuzzy {
                matched: session("20-homekeeper")
            }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_prefix("mawjs", &[session("114-mawjs-no2")]),
            ResolveResult::None { hints: None }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_prefix(
                "homeke",
                &[session("20-homekeeper"), session("21-homekey")],
            ),
            ResolveResult::Ambiguous {
                candidates: vec![session("20-homekeeper"), session("21-homekey")]
            }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_exact("homekeeper", &[session("20-homekeeper")]),
            ResolveResult::Exact {
                matched: session("20-homekeeper")
            }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_exact(
                "homekeeper",
                &[session("20-homekeeper"), session("21-homekeeper")]
            ),
            ResolveResult::Ambiguous {
                candidates: vec![session("20-homekeeper"), session("21-homekeeper")]
            }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_exact("homekeeper", &[session("not-homekeeper")]),
            ResolveResult::None { hints: None }
        );
    }

    #[test]
    fn fleet_window_helper_uses_window_and_repo_oracle_aliases() {
        let items = vec![
            Session {
                name: "23-discord-admin".to_owned(),
                windows: vec![FleetWindow {
                    name: Some("discord-oracle".to_owned()),
                    repo: Some("Soul-Brews-Studio/discord-oracle".to_owned()),
                }],
            },
            Session {
                name: "114-mawjs-no2".to_owned(),
                windows: vec![FleetWindow {
                    name: Some("mawjs-no2".to_owned()),
                    repo: Some("Soul-Brews-Studio/mawjs-no2".to_owned()),
                }],
            },
        ];

        assert_eq!(
            resolve_fleet_window_session_target("discord", &items),
            ResolveResult::Fuzzy {
                matched: items[0].clone()
            }
        );
        assert_eq!(
            resolve_fleet_window_session_target("mawjs", &items),
            ResolveResult::None { hints: None }
        );
    }
    #[test]
    fn matcher_edge_branches_cover_empty_str_alias_and_ambiguous_windows() {
        let borrowed: &str = "borrowed";
        assert_eq!(Named::name(&borrowed), "borrowed");

        assert_eq!(
            resolve_numeric_fleet_stem_prefix("", &[session("20-homekeeper")]),
            ResolveResult::None { hints: None }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_prefix("home", &[session("homekeeper")]),
            ResolveResult::None { hints: None }
        );
        assert_eq!(
            resolve_numeric_fleet_stem_prefix("homekeeper", &[session("20-homekeeper")]),
            ResolveResult::None { hints: None }
        );

        assert_eq!(
            resolve_fleet_window_session_target("", &[session("20-homekeeper")]),
            ResolveResult::None { hints: None }
        );

        let ambiguous = vec![
            Session {
                name: "a".to_owned(),
                windows: vec![FleetWindow {
                    name: Some("shared-oracle".to_owned()),
                    repo: None,
                }],
            },
            Session {
                name: "b".to_owned(),
                windows: vec![FleetWindow {
                    name: None,
                    repo: Some("org/shared-oracle".to_owned()),
                }],
            },
        ];
        assert_eq!(
            resolve_fleet_window_session_target("shared", &ambiguous),
            ResolveResult::Ambiguous {
                candidates: ambiguous
            }
        );
    }
}
