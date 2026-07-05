fn strip_activity_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        let _ = chars.next();
                        break;
                    }
                }
            }
            Some('(' | ')') => {
                let _ = chars.next();
                let _ = chars.next();
            }
            _ => {}
        }
    }
    out
}

fn is_stuck_activity_snapshot(input: &str) -> bool {
    let normalized = normalize_activity_snapshot(input);
    let lines = normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(10)
        .collect::<Vec<_>>();
    if lines.iter().any(|line| {
        matches!(*line, ">" | "$" | "#" | "❯" | "›" | "λ")
            || matches!(*line, "> ▌" | "$ ▌" | "# ▌" | "❯ ▌" | "› ▌" | "λ ▌")
    }) {
        return true;
    }
    let lower = normalized.to_ascii_lowercase();
    lower.ends_with("type a message")
        || lower.ends_with("send a message")
        || lower.ends_with("what can i help with?")
        || lower.ends_with("what can i help with")
        || lower.contains("claude code") && lower.ends_with('>')
}

const fn confidence_for_activity(samples: usize) -> ActivityConfidence {
    if samples >= 3 {
        ActivityConfidence::High
    } else if samples == 2 {
        ActivityConfidence::Medium
    } else {
        ActivityConfidence::Low
    }
}

fn round_seconds(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn filter_activity_results(results: &[ActivityResult], opts: &ActivityOptions) -> Vec<ActivityResult> {
    results
        .iter()
        .filter(|result| !opts.stuck_only || result.state == ActivityState::Stuck)
        .cloned()
        .collect()
}

fn record_activity_transitions(
    results: &[ActivityResult],
    previous: &mut BTreeMap<String, ActivityState>,
) -> Vec<ActivityResult> {
    let mut changed = Vec::new();
    for result in results {
        let prev = previous.insert(result.pane.clone(), result.state);
        if prev.is_some_and(|prev| prev != result.state) {
            changed.push(result.clone());
        }
    }
    changed
}

fn format_activity_output(results: &[ActivityResult], opts: &ActivityOptions) -> String {
    if opts.json {
        if opts.all {
            format!("[{}]\n", results.iter().map(format_activity_json_object).collect::<Vec<_>>().join(","))
        } else {
            results.first().map_or_else(String::new, |result| format_activity_json(result) + "\n")
        }
    } else {
        let text = results
            .iter()
            .map(format_activity_human)
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            String::new()
        } else {
            format!("{text}\n")
        }
    }
}

fn format_activity_json(result: &ActivityResult) -> String {
    format_activity_json_object(result)
}

fn format_activity_json_object(result: &ActivityResult) -> String {
    format!(
        "{{\"pane\":{},\"state\":{},\"confidence\":{},\"samples\":{},\"diff_samples\":{},\"last_change_ago_seconds\":{},\"sample_window_seconds\":{}}}",
        json_string(&result.pane),
        json_string(result.state.as_str()),
        json_string(result.confidence.as_str()),
        result.samples,
        result.diff_samples,
        json_number(result.last_change_ago_seconds),
        json_number(result.sample_window_seconds),
    )
}

fn format_activity_human(result: &ActivityResult) -> String {
    let icon = match result.state {
        ActivityState::Busy => "🟢",
        ActivityState::Idle => "🟡",
        ActivityState::Stuck => "🔴",
    };
    let age = match result.state {
        ActivityState::Busy => format!("last change {} ago", format_activity_duration(result.last_change_ago_seconds)),
        ActivityState::Stuck => format!("at prompt (no change in {})", format_activity_duration(result.last_change_ago_seconds)),
        ActivityState::Idle => format!("quiet (no change in {})", format_activity_duration(result.last_change_ago_seconds)),
    };
    format!(
        "{}: {icon} {} ({age}, {}/{} samples diff)",
        result.pane,
        result.state.as_str().to_ascii_uppercase(),
        result.diff_samples,
        result.samples,
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn format_activity_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        return format!("{}s", seconds.round() as u64);
    }
    let minutes = (seconds / 60.0).round() as u64;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    format!("{}h", ((minutes as f64) / 60.0).round() as u64)
}

#[allow(clippy::cast_precision_loss)]
fn format_activity_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    if ms.is_multiple_of(1_000) {
        return format_activity_duration(ms as f64 / 1_000.0);
    }
    let mut text = format!("{:.1}", ms as f64 / 1_000.0);
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    format!("{text}s")
}

fn sampling_description(opts: &ActivityOptions) -> Result<String, String> {
    let parsed = parse_activity_options(opts)?;
    let sampler = match parsed.sampler {
        ActivitySampler::Peek => "peek",
        ActivitySampler::Follow => "follow",
    };
    Ok(format!(
        "window={}, samples={}, sampler={sampler}",
        format_activity_duration_ms(parsed.window_ms),
        parsed.samples
    ))
}

