fn inbox_parse_hours_seconds(value: &str) -> Result<u64, String> {
    if value.is_empty() || value.starts_with('-') {
        return Err("--older-than-hours must be a non-negative number".to_owned());
    }
    let (whole, frac) = value.split_once('.').unwrap_or((value, ""));
    let hours = whole
        .parse::<u64>()
        .map_err(|_| "--older-than-hours must be a non-negative number".to_owned())?;
    let mut seconds = hours
        .checked_mul(3600)
        .ok_or_else(|| "--older-than-hours is too large".to_owned())?;
    if !frac.is_empty() {
        if !frac.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("--older-than-hours must be a non-negative number".to_owned());
        }
        let scale = 10_u64.pow(u32::try_from(frac.len().min(6)).unwrap_or(0));
        let trimmed = &frac[..frac.len().min(6)];
        let fraction = trimmed
            .parse::<u64>()
            .map_err(|_| "--older-than-hours must be a non-negative number".to_owned())?;
        seconds += fraction.saturating_mul(3600) / scale;
    }
    Ok(seconds)
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

fn inbox_archive_day(now_ms: u64) -> String {
    inbox_iso_label(now_ms)
        .get(0..10)
        .unwrap_or("1970-01-01")
        .to_owned()
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
    format!("{}_{}", &iso[0..10], &iso[11..16].replace(':', "-"))
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

#[cfg(test)]
mod inbox_tests {
    include!("inbox_inbox_tests/01_inbox_fake_sender_to_inbox_pending_show_ap_f225c6.rs");
    include!("inbox_inbox_tests/02_inbox_approve_sends_f_7ca5b6_to_inbox_path_has_no_s_e1df95.rs");
}
