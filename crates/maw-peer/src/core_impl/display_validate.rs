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

fn format_peer_list_row(cols: &[String], widths: &[usize]) -> String {
    cols.iter()
        .enumerate()
        .map(|(index, col)| {
            format!(
                "{col}{}",
                " ".repeat(widths[index].saturating_sub(col.len()))
            )
        })
        .collect::<Vec<_>>()
        .join("  ")
}

/// Default maw-js stale peer TTL: 7 days in milliseconds.
#[must_use]
pub const fn default_stale_ttl_ms() -> u64 {
    7 * 24 * 60 * 60 * 1000
}

/// Resolve stale TTL from `MAW_PEER_STALE_TTL_MS`-style input.
#[must_use]
pub fn parse_stale_ttl_ms(raw: Option<&str>) -> u64 {
    let Some(raw) = raw.filter(|value| !value.is_empty()) else {
        return default_stale_ttl_ms();
    };
    raw.parse::<u64>()
        .ok()
        .filter(|value| *value > 0)
        .unwrap_or_else(default_stale_ttl_ms)
}

/// Age of a peer's most informative timestamp in milliseconds.
///
/// Mirrors maw-js: use `lastSeen` when present, otherwise `addedAt`; invalid
/// provenance returns `None`, and future timestamps clamp to `0`.
#[must_use]
pub fn stale_age_ms(peer: &PeerRecord, now_ms: u64) -> Option<u64> {
    let reference = peer.last_seen.as_deref().unwrap_or(&peer.added_at);
    let timestamp = parse_iso_timestamp_ms(reference)?;
    Some(now_ms.saturating_sub(timestamp))
}

/// Is a peer stale for a given TTL and wall-clock timestamp?
#[must_use]
pub fn is_peer_stale(peer: &PeerRecord, ttl_ms: u64, now_ms: u64) -> bool {
    stale_age_ms(peer, now_ms).is_none_or(|age| age > ttl_ms)
}

fn parse_iso_timestamp_ms(value: &str) -> Option<u64> {
    let (date, time) = value.strip_suffix('Z')?.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i32>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }

    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second_part = time_parts.next()?;
    if time_parts.next().is_some() {
        return None;
    }
    let (second_raw, millis_raw) = second_part.split_once('.').unwrap_or((second_part, "0"));
    let second = second_raw.parse::<u32>().ok()?;
    let millis = parse_millis(millis_raw)?;

    if !(1..=12).contains(&month)
        || day == 0
        || day > days_in_month(year, month)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }

    let days = days_from_civil(year, month, day);
    let seconds = days
        .checked_mul(86_400)?
        .checked_add(i64::from(hour) * 3_600 + i64::from(minute) * 60 + i64::from(second))?;
    let ms = seconds.checked_mul(1000)?.checked_add(i64::from(millis))?;
    u64::try_from(ms).ok()
}

fn parse_millis(raw: &str) -> Option<u32> {
    if raw.is_empty() || !raw.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let mut value = raw.chars().take(3).collect::<String>();
    while value.len() < 3 {
        value.push('0');
    }
    value.parse::<u32>().ok()
}

const fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap_year(year) => 29,
        2 => 28,
        _ => 0,
    }
}

const fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// Days since Unix epoch for a Gregorian date.
fn days_from_civil(year: i32, month: u32, day: u32) -> i64 {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_i = month.cast_signed();
    let day_i = day.cast_signed();
    let doy = (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5 + day_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    i64::from(era) * 146_097 + i64::from(doe) - 719_468
}

#[cfg(test)]
mod remaining_coverage_tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("maw-peer-{name}-{nonce}"))
    }

    fn peer_record(url: &str) -> PeerRecord {
        PeerRecord {
            url: url.to_owned(),
            node: None,
            added_at: "2026-05-21T00:00:00Z".to_owned(),
            last_seen: None,
            last_error: None,
            nickname: None,
            pubkey: None,
            pubkey_first_seen: None,
            identity: None,
            one_way: None,
            last_symmetric_check: None,
        }
    }

    fn successful_probe(pubkey: Option<&str>) -> ProbePeerResult {
        ProbePeerResult {
            node: Some("white".to_owned()),
            nickname: Some("White".to_owned()),
            pubkey: pubkey.map(str::to_owned),
            identity: None,
            error: None,
        }
    }

    #[test]
    fn peer_store_parent_dir_helper_tolerates_parentless_paths() {
        create_peer_store_parent_dir(Path::new("")).expect("parentless path is already usable");
    }

    #[test]
    fn save_and_mutate_create_peer_store_parent_dirs() {
        let home = temp_dir("store-parent");
        let env = PeerStoreEnv::new(&home);
        let mut data = empty_peer_store();
        data.peers
            .insert("white".to_owned(), peer_record("http://white:3456"));

        save_peer_store(&env, &data).expect("save peer store");
        assert!(peer_store_path(&env).exists());

        let updated = mutate_peer_store(&env, |store| {
            store
                .peers
                .insert("mba".to_owned(), peer_record("http://mba:3456"));
        })
        .expect("mutate peer store");

        assert!(updated.peers.contains_key("white"));
        assert!(updated.peers.contains_key("mba"));
        let _ = fs::remove_dir_all(home);
    }

    #[test]
    fn peer_add_authenticated_and_probe_pubkey_mismatch_preserves_existing_store() {
        let mut peers = BTreeMap::new();
        peers.insert("white".to_owned(), peer_record("http://old:3456"));
        let plan = PeerAddPlan {
            alias: "white".to_owned(),
            url: "http://white:3456".to_owned(),
            node: None,
            authenticated_pubkey: Some("auth-key".to_owned()),
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers,
            probe: successful_probe(Some("probe-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add mismatch result");

        assert!(result.overwrote);
        assert_eq!(result.peer.url, "http://old:3456");
        assert_eq!(
            result.pubkey_mismatch,
            Some(PeerPubkeyMismatchError::new(
                "white",
                "auth-key",
                "probe-key"
            ))
        );
        assert_eq!(result.peers_after, plan.peers);
    }

    #[test]
    fn peer_add_authenticated_probe_mismatch_without_existing_peer_stays_empty() {
        let plan = PeerAddPlan {
            alias: "new-peer".to_owned(),
            url: "http://new-peer:3456".to_owned(),
            node: None,
            authenticated_pubkey: Some("auth-key".to_owned()),
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::new(),
            probe: successful_probe(Some("probe-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add mismatch result");

        assert!(!result.overwrote);
        assert_eq!(result.peer.url, "http://new-peer:3456");
        assert_eq!(
            result.pubkey_mismatch,
            Some(PeerPubkeyMismatchError::new(
                "new-peer",
                "auth-key",
                "probe-key"
            ))
        );
        assert!(result.peers_after.is_empty());
    }

    #[test]
    fn peer_add_allows_matching_authenticated_and_probe_pubkeys() {
        let plan = PeerAddPlan {
            alias: "white".to_owned(),
            url: "http://white:3456".to_owned(),
            node: None,
            authenticated_pubkey: Some("same-key".to_owned()),
            authenticated_identity: None,
            mark_symmetric_check: false,
            one_way: None,
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::new(),
            probe: successful_probe(Some("same-key")),
        };

        let result = cmd_peer_add_from_plan(&plan).expect("peer add succeeds");

        assert_eq!(result.pubkey_mismatch, None);
        assert_eq!(result.peer.pubkey.as_deref(), Some("same-key"));
        assert_eq!(
            result.peers_after["white"].pubkey.as_deref(),
            Some("same-key")
        );
    }

    #[test]
    fn unreadable_peer_store_path_returns_empty_store() {
        let dir = temp_dir("unreadable");
        fs::create_dir_all(&dir).expect("create dir path");

        assert_eq!(read_peer_store_unlocked(&dir), empty_peer_store());
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn invalid_iso_month_hits_zero_day_count() {
        assert_eq!(days_in_month(2026, 13), 0);
        assert_eq!(parse_iso_timestamp_ms("2026-13-01T00:00:00Z"), None);
    }

    #[test]
    fn private_iso_parser_rejects_malformed_components_and_handles_negative_eras() {
        for invalid in [
            "2026-05-21T00:00:00",
            "2026-05-2100:00:00Z",
            "year-05-21T00:00:00Z",
            "2026-month-21T00:00:00Z",
            "2026-05-dayT00:00:00Z",
            "2026-05-21Thour:00:00Z",
            "2026-05-21T00:minute:00Z",
            "2026-05-21T00:00:secondZ",
            "2026-05-21T00:00:00.badZ",
            "2026-05-21T00:00:00.Z",
            "2026-02-29T00:00:00Z",
            "2026-05-21T24:00:00Z",
            "2026-05-21T00:60:00Z",
            "2026-05-21T00:00:60Z",
        ] {
            assert_eq!(parse_iso_timestamp_ms(invalid), None, "{invalid}");
        }

        for (input, expected) in [
            ("2026-05-21T00:00:00.7Z", 1_779_321_600_700),
            ("2026-05-21T00:00:00.78Z", 1_779_321_600_780),
            ("2026-05-21T00:00:00.7899Z", 1_779_321_600_789),
        ] {
            assert_eq!(parse_iso_timestamp_ms(input), Some(expected), "{input}");
        }
        assert!(days_from_civil(-1, 3, 1) < 0);
    }

    #[test]
    fn peer_store_io_and_parser_error_edges_surface_without_mutation() {
        let blocked_parent = temp_dir("blocked-parent");
        fs::write(&blocked_parent, "not a directory").expect("write blocked parent");
        let blocked_file = blocked_parent.join("peers.json").display().to_string();
        let blocked_env =
            PeerStoreEnv::with_vars(temp_dir("blocked-home"), [("PEERS_FILE", blocked_file)]);

        let save_err = save_peer_store(&blocked_env, &empty_peer_store())
            .expect_err("file parent prevents save mkdir");
        assert_eq!(save_err.kind(), io::ErrorKind::AlreadyExists);
        let _ = fs::remove_file(&blocked_parent);

        let dir_path = temp_dir("rename-target-dir");
        fs::create_dir_all(&dir_path).expect("create directory target");
        let dir_env = PeerStoreEnv::with_vars(
            temp_dir("dir-home"),
            [("PEERS_FILE", dir_path.display().to_string())],
        );
        assert!(mutate_peer_store(&dir_env, |_| {}).is_err());
        let _ = fs::remove_dir_all(&dir_path);

        assert!(
            parse_peer_store(r#"{"peers":{"bad":{"url":1,"addedAt":2}}}"#)
                .expect_err("bad peer shape")
                .contains("invalid type")
        );
    }

    #[test]
    fn probe_mismatch_result_falls_back_to_existing_node() {
        let mut existing = peer_record("http://white:3456");
        existing.node = Some("white-node".to_owned());
        existing.pubkey = Some("cached-key".to_owned());
        let plan = PeerProbePlan {
            alias: "white".to_owned(),
            now: "2026-05-21T00:00:00Z".to_owned(),
            peers: BTreeMap::from([("white".to_owned(), existing)]),
            probe: ProbePeerResult {
                node: None,
                nickname: None,
                pubkey: Some("observed-key".to_owned()),
                identity: None,
                error: None,
            },
            remove_before_mutate: false,
        };

        let result = cmd_peer_probe_from_plan(&plan).expect("probe mismatch result");

        assert_eq!(result.node.as_deref(), Some("white-node"));
        assert_eq!(
            result.pubkey_mismatch,
            Some(PeerPubkeyMismatchError::new(
                "white",
                "cached-key",
                "observed-key"
            ))
        );
    }
}
