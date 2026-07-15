#[derive(Debug, Clone, PartialEq, Eq)]
struct PickerRow {
    matched: maw_matcher::ResolveMatch,
    detail: Option<String>,
    action: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PickerSelection {
    Pick(usize),
    Quit,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TypedPickerPlan {
    Target(String),
    Pick { context: &'static str, rows: Vec<PickerRow> },
}

fn typed_picker_plan(
    target: &str,
    candidates: &[maw_matcher::ResolveTypedCandidate],
    priority: fn(maw_matcher::ResolveCandidateKind) -> u8,
    row: fn(maw_matcher::ResolveMatch) -> PickerRow,
) -> TypedPickerPlan {
    let (context, mut matches) = match maw_matcher::resolve_typed_target(target, candidates) {
        maw_matcher::ResolveTypedResult::Match { matched }
            if matched.rank != maw_matcher::ResolveMatchRank::Fuzzy =>
        {
            return TypedPickerPlan::Target(matched.candidate.name);
        }
        maw_matcher::ResolveTypedResult::Match { matched } => ("matched fuzzily", vec![matched]),
        maw_matcher::ResolveTypedResult::Ambiguous { candidates } => {
            let best = candidates.iter().map(|item| priority(item.candidate.kind)).min().unwrap_or(u8::MAX);
            let preferred = candidates.into_iter().filter(|item| priority(item.candidate.kind) == best).collect::<Vec<_>>();
            if preferred.len() == 1 && preferred[0].rank != maw_matcher::ResolveMatchRank::Fuzzy {
                return TypedPickerPlan::Target(preferred[0].candidate.name.clone());
            }
            ("matches multiple targets", preferred)
        }
        maw_matcher::ResolveTypedResult::None => ("was not found exactly", deadend_closest_matches(target, candidates)),
    };
    matches.sort_by(|left, right| left.candidate.name.cmp(&right.candidate.name));
    let rows = matches.into_iter().map(row).collect::<Vec<_>>();
    if rows.is_empty() { TypedPickerPlan::Target(target.to_owned()) } else { TypedPickerPlan::Pick { context, rows } }
}

fn picker_choose_target(command: &str, target: &str, context: &str, rows: &[PickerRow], json: bool) -> Result<String, CliOutput> {
    use std::io::IsTerminal as _;
    if !std::io::stdin().is_terminal() {
        return Err(CliOutput { code: 1, stdout: if json { picker_render_json(command, target, context, rows) } else { picker_render_text(command, target, context, rows) }, stderr: String::new() });
    }
    let row = picker_prompt(command, target, context, rows).ok_or_else(|| CliOutput { code: 1, stdout: String::new(), stderr: format!("{command}: picker cancelled\n") })?;
    Ok(row.matched.candidate.name)
}

fn picker_parse_selection(input: &str, len: usize) -> PickerSelection {
    let trimmed = input.trim();
    if len == 1
        && (trimmed.is_empty()
            || trimmed.eq_ignore_ascii_case("y")
            || trimmed.eq_ignore_ascii_case("yes"))
    {
        return PickerSelection::Pick(0);
    }
    if trimmed.eq_ignore_ascii_case("q") || trimmed.eq_ignore_ascii_case("quit") {
        return PickerSelection::Quit;
    }
    trimmed
        .parse::<usize>()
        .map_or(PickerSelection::Invalid, |index| {
            if (1..=len).contains(&index) {
                PickerSelection::Pick(index - 1)
            } else {
                PickerSelection::Invalid
            }
        })
}

fn picker_render_text(command: &str, target: &str, context: &str, rows: &[PickerRow]) -> String {
    use std::fmt::Write as _;

    let mut out = format!("{command}: '{target}' {context}. Found nearby:\n");
    for (index, row) in rows.iter().enumerate() {
        let _ = writeln!(
            out,
            "  {}. {} {}{} ({:?})   → {}",
            index + 1,
            picker_kind_label(row.matched.candidate.kind),
            row.matched.candidate.name,
            row.detail
                .as_deref()
                .map_or_else(String::new, |detail| format!(" ({detail})")),
            row.matched.rank,
            row.action,
        );
    }
    out
}

fn picker_render_json(command: &str, target: &str, context: &str, rows: &[PickerRow]) -> String {
    let candidates = rows
        .iter()
        .map(|row| {
            format!(
                "{{\"kind\":{},\"name\":{},\"rank\":{},\"detail\":{},\"action\":{}}}",
                json_string(picker_kind_label(row.matched.candidate.kind)),
                json_string(&row.matched.candidate.name),
                json_string(&format!("{:?}", row.matched.rank)),
                row.detail
                    .as_deref()
                    .map_or_else(|| "null".to_owned(), json_string),
                json_string(&row.action),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"command\":{},\"target\":{},\"context\":{},\"candidates\":[{}]}}\n",
        json_string(command),
        json_string(target),
        json_string(context),
        candidates,
    )
}

fn picker_kind_label(kind: maw_matcher::ResolveCandidateKind) -> &'static str {
    match kind {
        maw_matcher::ResolveCandidateKind::LiveSession
        | maw_matcher::ResolveCandidateKind::SleepingRegistry => "session",
        maw_matcher::ResolveCandidateKind::FleetSquad => "fleet squad",
        maw_matcher::ResolveCandidateKind::Oracle => "oracle",
        maw_matcher::ResolveCandidateKind::Repo => "repo",
        maw_matcher::ResolveCandidateKind::Window => "window",
        maw_matcher::ResolveCandidateKind::Peer => "peer",
    }
}
