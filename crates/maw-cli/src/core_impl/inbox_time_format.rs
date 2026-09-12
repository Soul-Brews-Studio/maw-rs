// Dates in, human phrasing out.
//
// Timestamps arrive as ISO strings, as epoch millis and encoded in filenames,
// and leave as "3h ago" or a padded column. The civil-date conversions are done
// arithmetically rather than pulled from a date crate, which is why they are
// worth having in one place with a name on them.

fn inbox_parse_iso_ms(value: &str) -> Option<u64> {
    let prefix = value.get(0..16)?;
    let year = prefix.get(0..4)?.parse::<i32>().ok()?;
    let month = prefix.get(5..7)?.parse::<u32>().ok()?;
    let day = prefix.get(8..10)?.parse::<u32>().ok()?;
    let hour = prefix.get(11..13)?.parse::<u32>().ok()?;
    let minute = prefix.get(14..16)?.parse::<u32>().ok()?;
    inbox_ymdhm_to_ms(year, month, day, hour, minute)
}

fn inbox_parse_filename_ms(filename: &str) -> Option<u64> {
    let head = filename.get(0..16)?;
    let normalized = head.replace('_', "T").replace('-', "");
    let year = normalized.get(0..4)?.parse::<i32>().ok()?;
    let month = normalized.get(4..6)?.parse::<u32>().ok()?;
    let day = normalized.get(6..8)?.parse::<u32>().ok()?;
    let hour = normalized.get(9..11)?.parse::<u32>().ok()?;
    let minute = normalized.get(11..13)?.parse::<u32>().ok()?;
    inbox_ymdhm_to_ms(year, month, day, hour, minute)
}

fn inbox_ymdhm_to_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = inbox_days_from_civil(year, month, day)?;
    let seconds = days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60;
    u64::try_from(seconds).ok().map(|value| value * 1000)
}

fn inbox_days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_i = i32::try_from(month).ok()?;
    let day_i = i32::try_from(day).ok()?;
    let doy = (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5 + day_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
}

fn inbox_now_ms() -> u64 {
    inbox_system_time_ms(SystemTime::now())
}

fn inbox_system_time_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |duration| {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    })
}

fn inbox_age_seconds(timestamp_ms: u64, now_ms: u64) -> u64 {
    now_ms.saturating_sub(timestamp_ms) / 1000
}

fn inbox_relative_time(timestamp_ms: u64, now_ms: u64) -> String {
    if timestamp_ms == 0 {
        return "—".to_owned();
    }
    if timestamp_ms > now_ms {
        return "future".to_owned();
    }
    let mins = inbox_age_seconds(timestamp_ms, now_ms) / 60;
    if mins < 1 {
        "just now".to_owned()
    } else if mins < 60 {
        format!("{mins}m ago")
    } else if mins < 24 * 60 {
        format!("{}h ago", mins / 60)
    } else {
        format!("{}d ago", mins / (24 * 60))
    }
}

fn inbox_format_duration(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "never".to_owned();
    };
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 48 * 3600 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn inbox_format_delta(delta: i64) -> String {
    if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}

fn inbox_iso_label(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let secs_of_day = seconds % 86_400;
    let (year, month, day) = inbox_civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:00.000Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

fn inbox_file_time_label(ms: u64) -> String {
    let iso = inbox_iso_label(ms);
    format!("{}_{}", &iso[0..10], iso[11..16].replace(':', "-"))
}

fn inbox_civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (
        i32::try_from(y + i64::from(m <= 2)).unwrap_or(1970),
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

fn inbox_pad(value: &str, width: usize) -> String {
    let mut out = value.to_owned();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn inbox_truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}
