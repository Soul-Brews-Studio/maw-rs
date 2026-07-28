// "did you mean" for a name that resolved to nothing.
//
// Shared by wake and attach: when a target matches no candidate at all, an edit
// distance pass over the known oracles turns a dead end into a short list the
// caller can act on. Named for what it does rather than for either caller,
// because it belongs to neither.

fn deadend_oracle_candidates() -> Vec<maw_matcher::ResolveTypedCandidate> {
    let mut candidates = Vec::new();
    let mut seen = BTreeSet::new();
    if let Some(cache) = locate_load_registry_cache() {
        for oracle in cache.oracles {
            if oracle.name.is_empty() || !seen.insert(oracle.name.to_lowercase()) { continue; }
            let aliases = [oracle.repo, oracle.local_path].into_iter().filter(|value| !value.is_empty()).collect::<Vec<_>>();
            candidates.push(maw_matcher::ResolveTypedCandidate { kind: maw_matcher::ResolveCandidateKind::Oracle, name: oracle.name, aliases });
        }
    }
    for repo in wake_repo_candidates(&[]) {
        let name = wake_oracle_from_repo_path(&repo.path).unwrap_or(repo.name);
        if name.is_empty() || !seen.insert(name.to_lowercase()) { continue; }
        candidates.push(maw_matcher::ResolveTypedCandidate { kind: maw_matcher::ResolveCandidateKind::Oracle, name, aliases: Vec::new() });
    }
    candidates
}

fn deadend_suggestion_matches(target: &str, candidates: &[maw_matcher::ResolveTypedCandidate]) -> Vec<maw_matcher::ResolveMatch> {
    match maw_matcher::resolve_typed_target(target, candidates) {
        maw_matcher::ResolveTypedResult::Match { matched } => vec![matched],
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => candidates.into_iter().take(5).collect(),
        maw_matcher::ResolveTypedResult::None => deadend_closest_matches(target, candidates),
    }
}

fn deadend_closest_matches(target: &str, candidates: &[maw_matcher::ResolveTypedCandidate]) -> Vec<maw_matcher::ResolveMatch> {
    let targets = maw_matcher::normalized_match_names(target);
    let mut scored = Vec::<(usize, String, maw_matcher::ResolveTypedCandidate)>::new();
    for candidate in candidates {
        let names = std::iter::once(candidate.name.as_str()).chain(candidate.aliases.iter().map(String::as_str)).flat_map(maw_matcher::normalized_match_names).collect::<Vec<_>>();
        let Some(distance) = targets.iter().flat_map(|target| names.iter().map(|name| deadend_edit_distance(target, name))).min() else { continue; };
        if distance <= 2 || (target.len() > 6 && distance <= 3) {
            scored.push((distance, candidate.name.clone(), candidate.clone()));
        }
    }
    scored.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
    let mut seen = BTreeSet::new();
    scored.into_iter().filter_map(|(_, name, candidate)| seen.insert(name).then_some(maw_matcher::ResolveMatch { rank: maw_matcher::ResolveMatchRank::Fuzzy, candidate })).take(5).collect()
}

fn deadend_edit_distance(left: &str, right: &str) -> usize {
    let right_chars = right.chars().collect::<Vec<_>>();
    let mut costs = (0..=right_chars.len()).collect::<Vec<_>>();
    for (row, left_char) in left.chars().enumerate() {
        let mut previous = costs[0];
        costs[0] = row + 1;
        for (col, right_char) in right_chars.iter().enumerate() {
            let insert = costs[col + 1] + 1;
            let delete = costs[col] + 1;
            let replace = previous + usize::from(left_char != *right_char);
            previous = costs[col + 1];
            costs[col + 1] = insert.min(delete).min(replace);
        }
    }
    *costs.last().unwrap_or(&0)
}

fn deadend_suggestions_text(command: &str, target: &str, candidates: &[maw_matcher::ResolveMatch]) -> String {
    use std::fmt::Write as _;

    let mut out = if command == "wake" {
        format!("wake: repo not found for {target}\n")
    } else {
        format!("{command}: '{target}' not found\n")
    };
    if !candidates.is_empty() {
        out.push_str("Did you mean:\n");
        for matched in candidates {
            let action = match (command, matched.candidate.kind) {
                ("attach", maw_matcher::ResolveCandidateKind::LiveSession | maw_matcher::ResolveCandidateKind::Window) => format!("maw attach {}", matched.candidate.name),
                ("attach", maw_matcher::ResolveCandidateKind::FleetSquad) => format!("maw fleet wake {}", matched.candidate.name),
                ("attach", _) => format!("maw wake {} --attach", matched.candidate.name),
                ("wake", _) => format!("maw wake {}", matched.candidate.name),
                _ => matched.candidate.name.clone(),
            };
            let _ = writeln!(out, "  - {} → {action}", matched.candidate.name);
        }
    }
    out
}
