/// Render maw-js `formatProbeAll` table output.
#[must_use]
pub fn format_probe_all(result: &ProbeAllResult) -> String {
    if result.rows.is_empty() {
        return "no peers".to_owned();
    }

    let header = ["alias", "url", "node", "lastSeen", "result"].map(str::to_owned);
    let rows: Vec<[String; 5]> = result
        .rows
        .iter()
        .map(|row| {
            [
                row.alias.clone(),
                row.url.clone(),
                row.node.clone().unwrap_or_else(|| "-".to_owned()),
                row.last_seen.clone().unwrap_or_else(|| "-".to_owned()),
                if row.ok {
                    format!("\u{1b}[32m✓\u{1b}[0m ok ({}ms)", row.ms)
                } else {
                    format!(
                        "\u{1b}[31m✗\u{1b}[0m {}",
                        row.error
                            .as_ref()
                            .map_or("UNKNOWN", |err| err.code.as_str())
                    )
                },
            ]
        })
        .collect();

    let widths: Vec<usize> = header
        .iter()
        .enumerate()
        .map(|(index, heading)| {
            rows.iter()
                .map(|row| ansi_stripped_len(&row[index]))
                .max()
                .unwrap_or(0)
                .max(heading.len())
        })
        .collect();

    let divider = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>();
    let mut lines = vec![
        format_probe_all_row(&header, &widths),
        format_probe_all_row(&divider, &widths),
    ];
    lines.extend(rows.iter().map(|row| format_probe_all_row(row, &widths)));
    lines.push(String::new());
    lines.push(format!(
        "{}/{} ok{}",
        result.ok_count,
        result.rows.len(),
        if result.fail_count > 0 {
            format!(", {} failed", result.fail_count)
        } else {
            String::new()
        }
    ));
    lines.join("\n")
}

fn format_probe_all_row(cols: &[String], widths: &[usize]) -> String {
    cols.iter()
        .enumerate()
        .map(|(index, col)| {
            let padding = widths[index].saturating_sub(ansi_stripped_len(col));
            format!("{col}{}", " ".repeat(padding))
        })
        .collect::<Vec<_>>()
        .join("  ")
}

fn ansi_stripped_len(value: &str) -> usize {
    let mut len = 0;
    let mut chars = value.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code_ch in chars.by_ref() {
                if code_ch == 'm' {
                    break;
                }
            }
        } else {
            len += ch.len_utf8();
        }
    }
    len
}

/// Validate a peer alias using maw-js `impl.ts` rules.
#[must_use]
pub fn validate_peer_alias(alias: &str) -> Option<String> {
    if is_valid_peer_alias(alias) {
        None
    } else {
        Some(format!(
            "invalid alias \"{alias}\" (must match ^[a-z0-9][a-z0-9_-]{{0,31}}$)"
        ))
    }
}

fn is_valid_peer_alias(alias: &str) -> bool {
    let mut chars = alias.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
        return false;
    }
    let rest_len = chars
        .try_fold(0usize, |count, ch| {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '_' | '-') {
                Some(count + 1)
            } else {
                None
            }
        })
        .unwrap_or(usize::MAX);
    rest_len <= 31
}

/// Validate a peer URL using maw-js `impl.ts` rules.
#[must_use]
pub fn validate_peer_url(raw: &str) -> Option<String> {
    let Some((protocol, rest)) = raw.split_once("://") else {
        return Some(format!("invalid URL \"{raw}\""));
    };
    if !matches!(protocol, "http" | "https") {
        return Some(format!(
            "invalid URL \"{raw}\" (must be http:// or https://)"
        ));
    }
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.chars().any(char::is_whitespace) {
        return Some(format!("invalid URL \"{raw}\""));
    }
    None
}

/// Renderable peer-list row, ported from maw-js `PeerListRow`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerListRow {
    pub alias: String,
    pub url: String,
    pub node: Option<String>,
    pub nickname: Option<String>,
    pub last_seen: Option<String>,
    pub stale: bool,
    pub stale_age_ms: Option<u64>,
}

/// Render maw-js `formatList` output for peer rows.
#[must_use]
pub fn format_peer_list(rows: &[PeerListRow]) -> String {
    if rows.is_empty() {
        return "no peers".to_owned();
    }

    let header = ["alias", "url", "node", "nickname", "lastSeen"].map(str::to_owned);
    let lines: Vec<[String; 5]> = rows
        .iter()
        .map(|row| {
            [
                row.alias.clone(),
                row.url.clone(),
                row.node.clone().unwrap_or_else(|| "-".to_owned()),
                row.nickname.clone().unwrap_or_else(|| "-".to_owned()),
                row.last_seen.clone().unwrap_or_else(|| "-".to_owned()),
            ]
        })
        .collect();
    let widths: Vec<usize> = header
        .iter()
        .enumerate()
        .map(|(index, heading)| {
            lines
                .iter()
                .map(|line| line[index].len())
                .max()
                .unwrap_or(0)
                .max(heading.len())
        })
        .collect();

    let divider = widths
        .iter()
        .map(|width| "-".repeat(*width))
        .collect::<Vec<_>>();
    let mut out = vec![
        format_peer_list_row(&header, &widths),
        format_peer_list_row(&divider, &widths),
    ];
    out.extend(rows.iter().zip(lines.iter()).map(|(row, line)| {
        let mut rendered = format_peer_list_row(line, &widths);
        if row.stale {
            let suffix = row.stale_age_ms.map_or_else(
                || "never seen".to_owned(),
                |age| format!("last seen {}d ago", age / (24 * 60 * 60 * 1000)),
            );
            let _ = write!(rendered, "  \u{1b}[2m(stale, {suffix})\u{1b}[0m");
        }
        rendered
    }));
    out.join("\n")
}

