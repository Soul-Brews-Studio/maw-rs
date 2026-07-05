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
