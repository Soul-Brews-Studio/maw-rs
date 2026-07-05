fn format_pane_ambiguity_error(target: &str, candidates: &[PaneTargetCandidate]) -> String {
    let lines = candidates
        .iter()
        .map(|candidate| {
            let target_note = if candidate.target.is_empty() {
                String::new()
            } else {
                format!(" ({})", candidate.target)
            };
            format!(
                "    • {} → {}{} [{}]",
                candidate.name, candidate.resolved, target_note, candidate.source
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "'{target}' is ambiguous — matches {} panes:\n{lines}\n  use the pane id or full session:window.pane target",
        candidates.len()
    )
}

fn basename(path: &str) -> &str {
    path.split('/')
        .rfind(|part| !part.is_empty())
        .unwrap_or(path)
}

fn nested_agents_worktree(cwd: &str) -> Option<(&str, &str)> {
    let parts = cwd
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let [.., repo, "agents", worktree] = parts.as_slice() else {
        return None;
    };
    Some((repo, worktree))
}

fn worktree_names_from_cwd(cwd: &str) -> Vec<(String, String)> {
    let (repo, base) = nested_agents_worktree(cwd).unwrap_or_else(|| ("", basename(cwd)));
    if base.is_empty() {
        return Vec::new();
    }
    let mut out = vec![(base.to_owned(), "worktree-dir".to_owned())];
    let nested = !repo.is_empty();
    let (repo, rest) = if nested {
        (repo, base)
    } else {
        let Some((repo, rest)) = base.split_once(".wt-") else {
            return out;
        };
        (repo, rest)
    };
    let role = rest
        .split_once('-')
        .map_or(if nested { rest } else { "" }, |(_, role)| role)
        .trim();
    if !role.is_empty() {
        out.push((role.to_owned(), "worktree-role".to_owned()));
        if let Some(repo_stem) = repo.strip_suffix("-oracle") {
            if !repo_stem.is_empty() {
                out.push((format!("{repo_stem}-{role}"), "worktree-alias".to_owned()));
            }
        }
    }
    out
}

/// Parse `PANE_TARGET_FORMAT` rows into pane target resolution candidates.
#[must_use]
pub fn pane_target_candidates_from_list_panes_output(raw: &str) -> Vec<PaneTargetCandidate> {
    let mut candidates = Vec::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let fields = line.split("|||").collect::<Vec<_>>();
        let id = fields.first().copied().unwrap_or_default().trim();
        let target = fields.get(1).copied().unwrap_or_default().trim();
        let title = fields.get(2).copied().unwrap_or_default();
        let tile_role = fields.get(3).copied().unwrap_or_default();
        let cwd = fields.get(4).copied().unwrap_or_default();
        let resolved = if id.is_empty() { target } else { id };
        if resolved.is_empty() {
            continue;
        }
        add_pane_target_candidate(&mut candidates, title, resolved, "pane-title", target);
        add_pane_target_candidate(&mut candidates, tile_role, resolved, "tile-role", target);
        for (name, source) in worktree_names_from_cwd(cwd) {
            add_pane_target_candidate(&mut candidates, &name, resolved, &source, target);
        }
    }
    candidates
}

fn add_pane_target_candidate(
    candidates: &mut Vec<PaneTargetCandidate>,
    name: &str,
    resolved: &str,
    source: &str,
    target: &str,
) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    candidates.push(PaneTargetCandidate {
        name: name.to_owned(),
        resolved: resolved.to_owned(),
        source: source.to_owned(),
        target: target.to_owned(),
    });
}

fn unique_by_resolved(candidates: Vec<PaneTargetCandidate>) -> Vec<PaneTargetCandidate> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for candidate in candidates {
        if seen.insert(candidate.resolved.clone()) {
            out.push(candidate);
        }
    }
    out
}

/// Resolve a natural pane title, tile role, worktree dir, or worktree alias to a pane id.
#[must_use]
pub fn resolve_pane_target_from_candidates(
    target: &str,
    candidates: &[PaneTargetCandidate],
) -> PaneTargetResolution {
    let trimmed = target.trim().to_lowercase();
    let exact = unique_by_resolved(
        candidates
            .iter()
            .filter(|candidate| candidate.name.to_lowercase() == trimmed)
            .cloned()
            .collect(),
    );
    match exact.len() {
        1 => {
            return PaneTargetResolution::Match {
                candidate: exact[0].clone(),
            }
        }
        2.. => return PaneTargetResolution::Ambiguous { candidates: exact },
        0 => {}
    }

    match resolve_by_name(target, candidates, ResolveOptions::default()) {
        ResolveResult::Exact { matched } | ResolveResult::Fuzzy { matched } => {
            PaneTargetResolution::Match { candidate: matched }
        }
        ResolveResult::Ambiguous { candidates } => PaneTargetResolution::Ambiguous {
            candidates: unique_by_resolved(candidates),
        },
        ResolveResult::None { .. } => PaneTargetResolution::None,
    }
}

/// Resolve a pane target directly from `PANE_TARGET_FORMAT` list-panes output.
#[must_use]
pub fn resolve_pane_target_from_list_panes_output(target: &str, raw: &str) -> PaneTargetResolution {
    resolve_pane_target_from_candidates(target, &pane_target_candidates_from_list_panes_output(raw))
}

/// Parse `tmux list-sessions -F '#{session_name}\t#{session_created}'` style epoch rows.
#[must_use]
pub fn parse_session_epoch_list(raw: &str) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Some((name, epoch_raw)) = line.split_once('\t') else {
            continue;
        };
        let Ok(epoch) = epoch_raw.parse::<u64>() else {
            continue;
        };
        if !name.is_empty() && epoch > 0 {
            out.insert(name.to_owned(), epoch);
        }
    }
    out
}

/// Parse tmux session creation rows.
#[must_use]
pub fn parse_session_created_list(raw: &str) -> BTreeMap<String, u64> {
    parse_session_epoch_list(raw)
}

/// Parse tmux session activity rows.
#[must_use]
pub fn parse_session_activity_list(raw: &str) -> BTreeMap<String, u64> {
    parse_session_epoch_list(raw)
}

/// Parse `maw ls --active` duration values. Bare numbers are minutes.
#[must_use]
pub fn parse_active_duration_seconds(raw: Option<&str>) -> Option<u64> {
    let trimmed = raw?.trim().to_lowercase();
    if trimmed.is_empty() {
        return None;
    }
    let last = trimmed.chars().last()?;
    let (digits, multiplier) = match last {
        's' | 'm' | 'h' | 'd' => (&trimmed[..trimmed.len() - 1], active_duration_multiplier(last)),
        _ => (trimmed.as_str(), 60),
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let value = digits.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }
    value.checked_mul(multiplier)
}

