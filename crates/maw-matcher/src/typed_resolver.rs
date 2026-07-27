#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveCandidateKind {
    LiveSession,
    SleepingRegistry,
    FleetSquad,
    Oracle,
    Repo,
    Window,
    Peer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolveMatchRank {
    Exact,
    Live,
    Registry,
    HashSlotOwner,
    Fuzzy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveTypedCandidate {
    pub kind: ResolveCandidateKind,
    pub name: String,
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveMatch {
    pub rank: ResolveMatchRank,
    pub candidate: ResolveTypedCandidate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveTypedResult {
    None,
    Match { matched: ResolveMatch },
    Ambiguous { candidates: Vec<ResolveMatch> },
}

/// Locate-style comparison names: lower-case, strip `NN-`, strip `-oracle`.
#[must_use]
pub fn normalized_match_names(raw: &str) -> Vec<String> {
    let raw = raw.trim().to_lowercase();
    if raw.is_empty() {
        return Vec::new();
    }
    let stem = strip_numeric_prefix(&raw).unwrap_or(&raw);
    dedup(
        [
            raw.as_str(),
            strip_oracle_suffix(&raw),
            stem,
            strip_oracle_suffix(stem),
        ]
        .into_iter()
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect(),
    )
}

#[must_use]
pub fn resolve_typed_target(
    target: &str,
    candidates: &[ResolveTypedCandidate],
) -> ResolveTypedResult {
    let raw = target.trim().to_lowercase();
    if raw.is_empty() {
        return ResolveTypedResult::None;
    }
    let no_oracle = strip_oracle_suffix(&raw);
    let slot_stem = strip_numeric_prefix(no_oracle);
    let exact_targets = dedup(vec![raw.clone(), no_oracle.to_owned()]);
    let mut best_rank = None;
    let mut matches = Vec::new();

    for candidate in candidates {
        let aliases = candidate_names(candidate);
        if let Some(rank) = rank_candidate(
            &aliases,
            &exact_targets,
            &raw,
            no_oracle,
            slot_stem,
            candidate.kind,
        ) {
            match best_rank {
                None => best_rank = Some(rank),
                Some(best) if rank < best => {
                    best_rank = Some(rank);
                    matches.clear();
                }
                Some(best) if rank > best => continue,
                Some(_) => {}
            }
            matches.push(ResolveMatch {
                rank,
                candidate: candidate.clone(),
            });
        }
    }
    let mut iter = matches.into_iter();
    match (iter.next(), iter.next()) {
        (None, _) => ResolveTypedResult::None,
        (Some(winner), None) => ResolveTypedResult::Match { matched: winner },
        (Some(first), Some(second)) => {
            let mut candidates = vec![first, second];
            candidates.extend(iter);
            if let Some(winner) = live_tiebreak(&candidates) {
                return ResolveTypedResult::Match { matched: winner };
            }
            if let Some(winner) = literal_name_tiebreak(&raw, &candidates) {
                return ResolveTypedResult::Match { matched: winner };
            }
            ResolveTypedResult::Ambiguous { candidates }
        }
    }
}

/// A live session should win a same-rank tie against anything non-live --
/// predates `literal_name_tiebreak` (#612) and must run first: a non-live
/// candidate whose name literally equals the target (e.g. an oracle-registry
/// entry) can out-score a live session whose name only matches after
/// stripping a numeric session prefix, silently reversing #612's policy
/// (confirmed: this file's own tests passed before #665 landed and failed
/// after, on exactly this shape). Only fires when exactly one candidate in
/// the tie is live -- two live candidates (or none) stay genuinely ambiguous
/// or fall through to the literal-name tiebreak.
fn live_tiebreak(candidates: &[ResolveMatch]) -> Option<ResolveMatch> {
    let mut live = candidates
        .iter()
        .filter(|matched| is_live(matched.candidate.kind));
    let only = live.next()?;
    live.next().is_none().then(|| only.clone())
}

/// When a tie leaves more than one candidate at the best rank, prefer the
/// single candidate that matches the raw target most *literally*. Scores
/// each candidate (see [`literal_match_score`]) and wins only when the top
/// score is held by exactly one candidate — otherwise it stays genuinely
/// ambiguous. Disambiguates two families:
/// - repo `-oracle` stripping making both "maw-rs" and "maw-rs-oracle" rank
///   Exact (the whole-name literal wins).
/// - registry `session:window` names where every window in a repo aliases the
///   repo name (`33-maw-rs:maw-rs` vs `…:mawrs-codex-cli` vs
///   `inverted-pendulum-oracle:maw-rs`): the window part + session stem win.
fn literal_name_tiebreak(raw: &str, candidates: &[ResolveMatch]) -> Option<ResolveMatch> {
    let best = candidates
        .iter()
        .max_by_key(|m| literal_match_score(&m.candidate.name, raw))?;
    let best_score = literal_match_score(&best.candidate.name, raw);
    if best_score == 0 {
        return None;
    }
    let ties = candidates
        .iter()
        .filter(|m| literal_match_score(&m.candidate.name, raw) == best_score)
        .count();
    (ties == 1).then(|| best.clone())
}

/// How literally a candidate name matches the raw target:
/// - `3` — whole name equals the target (the repo/exact case).
/// - `+2` — the `session:window` window part equals the target.
/// - `+1` — the session stem (numeric prefix stripped) equals the target.
fn literal_match_score(name: &str, raw: &str) -> u32 {
    let name = name.trim().to_lowercase();
    if name == raw {
        return 3;
    }
    let window = name.rsplit(':').next().unwrap_or(name.as_str());
    let session = name.split(':').next().unwrap_or("");
    let session_stem = strip_numeric_prefix(session).unwrap_or(session);
    u32::from(window == raw) * 2 + u32::from(session_stem == raw)
}

fn candidate_names(candidate: &ResolveTypedCandidate) -> Vec<String> {
    let mut names = Vec::new();
    for raw in
        std::iter::once(candidate.name.as_str()).chain(candidate.aliases.iter().map(String::as_str))
    {
        names.extend(normalized_match_names(raw));
    }
    dedup(names)
}

fn rank_candidate(
    aliases: &[String],
    exact_targets: &[String],
    raw: &str,
    no_oracle: &str,
    slot_stem: Option<&str>,
    kind: ResolveCandidateKind,
) -> Option<ResolveMatchRank> {
    if aliases.iter().any(|alias| exact_targets.contains(alias)) {
        return Some(ResolveMatchRank::Exact);
    }
    if is_live(kind)
        && aliases
            .iter()
            .any(|alias| segment_match(alias, raw, no_oracle))
    {
        return Some(ResolveMatchRank::Live);
    }
    if is_registry(kind)
        && aliases
            .iter()
            .any(|alias| segment_match(alias, raw, no_oracle))
    {
        return Some(ResolveMatchRank::Registry);
    }
    if slot_stem.is_some_and(|stem| aliases.iter().any(|alias| alias == stem)) {
        return Some(ResolveMatchRank::HashSlotOwner);
    }
    fuzzy_targets(raw, no_oracle, slot_stem)
        .iter()
        .any(|target| aliases.iter().any(|alias| alias.contains(target)))
        .then_some(ResolveMatchRank::Fuzzy)
}

fn fuzzy_targets<'a>(raw: &'a str, no_oracle: &'a str, slot_stem: Option<&'a str>) -> Vec<&'a str> {
    let mut targets = vec![raw, no_oracle];
    if let Some(stem) = slot_stem {
        targets.push(stem);
    }
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn segment_match(alias: &str, raw: &str, no_oracle: &str) -> bool {
    [raw, no_oracle].into_iter().any(|target| {
        alias.ends_with(&format!("-{target}"))
            || alias.starts_with(&format!("{target}-"))
            || alias.contains(&format!("-{target}-"))
    })
}

fn is_live(kind: ResolveCandidateKind) -> bool {
    matches!(
        kind,
        ResolveCandidateKind::LiveSession | ResolveCandidateKind::Window
    )
}
fn is_registry(kind: ResolveCandidateKind) -> bool {
    matches!(
        kind,
        ResolveCandidateKind::SleepingRegistry | ResolveCandidateKind::Oracle
    )
}

fn strip_numeric_prefix(value: &str) -> Option<&str> {
    let (prefix, stem) = value.split_once('-')?;
    (!prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) && !stem.is_empty())
        .then_some(stem)
}

fn strip_oracle_suffix(value: &str) -> &str {
    value.strip_suffix("-oracle").unwrap_or(value)
}
fn dedup<T: Ord>(mut values: Vec<T>) -> Vec<T> {
    values.sort_unstable();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::{
        resolve_typed_target, ResolveCandidateKind, ResolveMatchRank, ResolveTypedCandidate,
        ResolveTypedResult,
    };

    fn repo(name: &str) -> ResolveTypedCandidate {
        ResolveTypedCandidate {
            kind: ResolveCandidateKind::Repo,
            name: name.to_owned(),
            aliases: Vec::new(),
        }
    }

    // A sleeping-registry `session:window` entry. Every window in a repo aliases
    // the repo name (`wake_registry_aliases`), so they all rank equally.
    fn registry(name: &str) -> ResolveTypedCandidate {
        ResolveTypedCandidate {
            kind: ResolveCandidateKind::SleepingRegistry,
            name: name.to_owned(),
            aliases: vec!["maw-rs".to_owned()],
        }
    }

    #[test]
    fn oracle_suffix_stripping_does_not_shadow_literal_base_name() {
        let candidates = vec![repo("maw-rs"), repo("maw-rs-oracle")];
        let result = resolve_typed_target("maw-rs", &candidates);
        match result {
            ResolveTypedResult::Match { matched } => {
                assert_eq!(matched.candidate.name, "maw-rs");
                assert_eq!(matched.rank, ResolveMatchRank::Exact);
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn oracle_suffix_stripping_does_not_shadow_literal_oracle_name() {
        let candidates = vec![repo("maw-rs"), repo("maw-rs-oracle")];
        let result = resolve_typed_target("maw-rs-oracle", &candidates);
        match result {
            ResolveTypedResult::Match { matched } => {
                assert_eq!(matched.candidate.name, "maw-rs-oracle");
                assert_eq!(matched.rank, ResolveMatchRank::Exact);
            }
            other => panic!("expected Match, got {other:?}"),
        }
    }

    #[test]
    fn registry_window_and_session_stem_beat_repo_alias_ambiguity() {
        // Every window aliases the repo "maw-rs" so all rank equally; the
        // window part (+2) plus session stem (+1) single out the real oracle.
        let candidates = vec![
            registry("33-maw-rs:maw-rs"),
            registry("33-maw-rs:maw-rs-oracle"),
            registry("33-maw-rs:mawrs-codex-cli"),
            registry("inverted-pendulum-oracle:maw-rs"),
        ];
        match resolve_typed_target("maw-rs", &candidates) {
            ResolveTypedResult::Match { matched } => {
                assert_eq!(matched.candidate.name, "33-maw-rs:maw-rs");
            }
            other => panic!("expected Match(33-maw-rs:maw-rs), got {other:?}"),
        }
    }

    #[test]
    fn registry_stays_ambiguous_when_stem_cannot_separate() {
        // Two windows named "maw-rs" whose session stems don't match the
        // target both score 2 — genuinely ambiguous, must not mis-pick.
        let candidates = vec![registry("aa:maw-rs"), registry("bb:maw-rs")];
        match resolve_typed_target("maw-rs", &candidates) {
            ResolveTypedResult::Ambiguous { .. } => {}
            other => panic!("expected Ambiguous, got {other:?}"),
        }
    }

    #[test]
    fn live_session_beats_an_oracle_registry_exact_tie_even_when_its_name_scores_lower() {
        // #612's whole point: a live session should win a same-rank tie
        // against a non-live candidate. Both rank Exact here -- the oracle
        // candidate via its literal name, the live session via its numeric-
        // prefix-stripped name -- but literal_name_tiebreak (#665, unaware
        // of liveness) scores the oracle's whole-name match (3) higher than
        // the live session's stem-only match (1) and would otherwise pick
        // it outright, silently reversing #612. live_tiebreak must run
        // first. Confirmed via a worktree at #665's parent commit that the
        // maw-cli test this mirrors (attach_auto_picks_single_live_session_
        // over_oracle_registry_tie) passed there and failed after.
        let live = ResolveTypedCandidate {
            kind: ResolveCandidateKind::LiveSession,
            name: "14-oracle-hall".to_owned(),
            aliases: Vec::new(),
        };
        let oracle = ResolveTypedCandidate {
            kind: ResolveCandidateKind::Oracle,
            name: "oracle-hall".to_owned(),
            aliases: Vec::new(),
        };
        match resolve_typed_target("oracle-hall", &[oracle, live]) {
            ResolveTypedResult::Match { matched } => {
                assert_eq!(matched.candidate.kind, ResolveCandidateKind::LiveSession);
                assert_eq!(matched.candidate.name, "14-oracle-hall");
            }
            other => panic!("expected the live session to win, got {other:?}"),
        }
    }
}
