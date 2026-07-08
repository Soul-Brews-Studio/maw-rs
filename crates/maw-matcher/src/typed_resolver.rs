#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveCandidateKind { LiveSession, SleepingRegistry, FleetGroup, Oracle, Repo, Window, Peer }

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResolveMatchRank { Exact, Live, Registry, HashSlotOwner, Fuzzy }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveTypedCandidate { pub kind: ResolveCandidateKind, pub name: String, pub aliases: Vec<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveMatch { pub rank: ResolveMatchRank, pub candidate: ResolveTypedCandidate }

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveTypedResult { None, Match { matched: ResolveMatch }, Ambiguous { candidates: Vec<ResolveMatch> } }

/// Locate-style comparison names: lower-case, strip `NN-`, strip `-oracle`.
#[must_use]
pub fn normalized_match_names(raw: &str) -> Vec<String> {
    let raw = raw.trim().to_lowercase();
    if raw.is_empty() { return Vec::new(); }
    let stem = strip_numeric_prefix(&raw).unwrap_or(&raw);
    dedup([
        raw.as_str(), strip_oracle_suffix(&raw), stem, strip_oracle_suffix(stem),
    ].into_iter().filter(|name| !name.is_empty()).map(str::to_owned).collect())
}

#[must_use]
pub fn resolve_typed_target(target: &str, candidates: &[ResolveTypedCandidate]) -> ResolveTypedResult {
    let raw = target.trim().to_lowercase();
    if raw.is_empty() { return ResolveTypedResult::None; }
    let no_oracle = strip_oracle_suffix(&raw);
    let slot_stem = strip_numeric_prefix(no_oracle);
    let exact_targets = dedup(vec![raw.clone(), no_oracle.to_owned()]);
    let mut best_rank = None;
    let mut matches = Vec::new();

    for candidate in candidates {
        let aliases = candidate_names(candidate);
        if let Some(rank) = rank_candidate(&aliases, &exact_targets, &raw, no_oracle, slot_stem, candidate.kind) {
            match best_rank {
                None => best_rank = Some(rank),
                Some(best) if rank < best => { best_rank = Some(rank); matches.clear(); }
                Some(best) if rank > best => continue,
                Some(_) => {}
            }
            matches.push(ResolveMatch { rank, candidate: candidate.clone() });
        }
    }
    let mut iter = matches.into_iter();
    match (iter.next(), iter.next()) {
        (None, _) => ResolveTypedResult::None,
        (Some(winner), None) => ResolveTypedResult::Match { matched: winner },
        (Some(first), Some(second)) => {
            let mut candidates = vec![first, second];
            candidates.extend(iter);
            ResolveTypedResult::Ambiguous { candidates }
        }
    }
}

fn candidate_names(candidate: &ResolveTypedCandidate) -> Vec<String> {
    let mut names = Vec::new();
    for raw in std::iter::once(candidate.name.as_str()).chain(candidate.aliases.iter().map(String::as_str)) {
        names.extend(normalized_match_names(raw));
    }
    dedup(names)
}

fn rank_candidate(
    aliases: &[String], exact_targets: &[String], raw: &str, no_oracle: &str,
    slot_stem: Option<&str>, kind: ResolveCandidateKind,
) -> Option<ResolveMatchRank> {
    if aliases.iter().any(|alias| exact_targets.contains(alias)) { return Some(ResolveMatchRank::Exact); }
    if is_live(kind) && aliases.iter().any(|alias| segment_match(alias, raw, no_oracle)) { return Some(ResolveMatchRank::Live); }
    if is_registry(kind) && aliases.iter().any(|alias| segment_match(alias, raw, no_oracle)) { return Some(ResolveMatchRank::Registry); }
    if slot_stem.is_some_and(|stem| aliases.iter().any(|alias| alias == stem)) { return Some(ResolveMatchRank::HashSlotOwner); }
    fuzzy_targets(raw, no_oracle, slot_stem).iter()
        .any(|target| aliases.iter().any(|alias| alias.contains(target)))
        .then_some(ResolveMatchRank::Fuzzy)
}

fn fuzzy_targets<'a>(raw: &'a str, no_oracle: &'a str, slot_stem: Option<&'a str>) -> Vec<&'a str> {
    let mut targets = vec![raw, no_oracle];
    if let Some(stem) = slot_stem { targets.push(stem); }
    targets.sort_unstable();
    targets.dedup();
    targets
}

fn segment_match(alias: &str, raw: &str, no_oracle: &str) -> bool {
    [raw, no_oracle].into_iter().any(|target| {
        alias.ends_with(&format!("-{target}")) || alias.starts_with(&format!("{target}-")) || alias.contains(&format!("-{target}-"))
    })
}

fn is_live(kind: ResolveCandidateKind) -> bool { matches!(kind, ResolveCandidateKind::LiveSession | ResolveCandidateKind::Window) }
fn is_registry(kind: ResolveCandidateKind) -> bool { matches!(kind, ResolveCandidateKind::SleepingRegistry | ResolveCandidateKind::Oracle) }

fn strip_numeric_prefix(value: &str) -> Option<&str> {
    let (prefix, stem) = value.split_once('-')?;
    (!prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) && !stem.is_empty()).then_some(stem)
}

fn strip_oracle_suffix(value: &str) -> &str { value.strip_suffix("-oracle").unwrap_or(value) }
fn dedup<T: Ord>(mut values: Vec<T>) -> Vec<T> { values.sort_unstable(); values.dedup(); values }
