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
