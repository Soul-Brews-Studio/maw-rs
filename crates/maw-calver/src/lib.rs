//! Pure `CalVer` arithmetic ported from maw-js `scripts/calver.ts`.
//!
//! This crate intentionally excludes git/package IO. Behavior is locked by the
//! maw-js portable fixture file `test/spec/calver.fixtures.json`.
/// Pre-release channel used by alpha/beta cuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Alpha,
    Beta,
}

impl Channel {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Alpha => "alpha",
            Self::Beta => "beta",
        }
    }
}

const DAYS_IN_MONTH: [i32; 13] = [0, 31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

/// Local wall-clock parts used by the portable `CalVer` spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DateParts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Pure arguments for [`compute_version`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ComputeArgs {
    pub stable: bool,
    pub channel: Option<Channel>,
    pub now: DateParts,
}

/// Return `YY.M.D` with no zero padding.
#[must_use]
pub fn date_base(now: DateParts) -> String {
    format!("{}.{}.{}", now.year.rem_euclid(100), now.month, now.day)
}

/// Extract leading `YY.M.D` from stable or prerelease `CalVer` strings.
#[must_use]
pub fn extract_base_from_version(version: &str) -> Option<String> {
    if version.is_empty() {
        return None;
    }
    let stripped = version.strip_prefix('v').unwrap_or(version);
    let base = stripped
        .split_once(['-', '+'])
        .map_or(stripped, |(base, _)| base);
    let mut parts = base.split('.');
    let yy = parts.next()?;
    let mo = parts.next()?;
    let da = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    if yy.is_empty() || mo.is_empty() || da.is_empty() {
        return None;
    }
    if ![yy, mo, da]
        .iter()
        .all(|part| part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    Some(format!("{yy}.{mo}.{da}"))
}

/// Numeric comparison of `YY.M.D` bases.
///
/// # Panics
/// Panics when either input is not a three-segment integer base, mirroring the
/// TypeScript helper throwing for invalid upstream calls.
#[must_use]
pub fn compare_bases(a: &str, b: &str) -> i32 {
    let pa = parse_base(a)
        .unwrap_or_else(|| panic!("compareBases expects YY.M.D, got \"{a}\" vs \"{b}\""));
    let pb = parse_base(b)
        .unwrap_or_else(|| panic!("compareBases expects YY.M.D, got \"{a}\" vs \"{b}\""));
    for (left, right) in pa.into_iter().zip(pb) {
        if left != right {
            return left - right;
        }
    }
    0
}

/// Validate the permissive maw-js calendar base.
///
/// February allows day 29 regardless of leap year to preserve the maw-js guard.
#[must_use]
pub fn is_valid_calendar_date(base: &str) -> bool {
    let Some([_, month, day]) = parse_base(base) else {
        return false;
    };
    if !(1..=12).contains(&month) {
        return false;
    }
    let month_index = usize::try_from(month).unwrap_or_default();
    (1..=DAYS_IN_MONTH[month_index]).contains(&day)
}

/// Pick the later of today's base and package.json's valid `CalVer` base.
///
/// # Errors
/// Returns an error when `package_version` contains a ghost calendar date.
pub fn effective_base(today_base: &str, package_version: &str) -> Result<String, String> {
    let Some(pkg_base) = extract_base_from_version(package_version) else {
        return Ok(today_base.to_owned());
    };
    if !is_valid_calendar_date(&pkg_base) {
        let parts: Vec<&str> = pkg_base.split('.').collect();
        return Err(format!(
            "ghost date in package.json: {package_version} (day {} doesn't exist in month {}) — fix package.json version to a real date",
            parts.get(2).copied().unwrap_or(""),
            parts.get(1).copied().unwrap_or("")
        ));
    }
    if compare_bases(&pkg_base, today_base) > 0 {
        Ok(pkg_base)
    } else {
        Ok(today_base.to_owned())
    }
}

/// Max numeric suffix in matching `v{base}-{channel}.{N}` tags.
#[must_use]
pub fn max_n_from_tags(base: &str, channel: Channel, tags: &[String]) -> i32 {
    let prefix = format!("v{base}-{}.", channel.as_str());
    tags.iter()
        .filter_map(|tag| tag.strip_prefix(&prefix))
        .filter(|rest| !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit()))
        .filter_map(|rest| rest.parse::<i32>().ok())
        .max()
        .unwrap_or(-1)
}

/// Back-compat alpha-only tag scanner.
#[must_use]
pub fn max_alpha_from_tags(base: &str, tags: &[String]) -> i32 {
    max_n_from_tags(base, Channel::Alpha, tags)
}

/// Max numeric suffix in matching package.json version.
#[must_use]
pub fn max_n_from_package_json(base: &str, channel: Channel, package_version: &str) -> i32 {
    if package_version.is_empty() {
        return -1;
    }
    let stripped = package_version.strip_prefix('v').unwrap_or(package_version);
    let prefix = format!("{base}-{}.", channel.as_str());
    let Some(rest) = stripped.strip_prefix(&prefix) else {
        return -1;
    };
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_digit()) {
        return -1;
    }
    rest.parse::<i32>().unwrap_or(-1)
}

/// HHMM stamp as integer string (`H * 100 + M`) with no leading zeroes.
#[must_use]
pub fn hhmm_stamp(now: DateParts) -> String {
    (now.hour * 100 + now.minute).to_string()
}

/// Return the next permissive calendar base.
///
/// # Panics
/// Panics if `base` is not a valid maw-js calendar base.
#[must_use]
pub fn next_calendar_base(base: &str) -> String {
    assert!(
        is_valid_calendar_date(base),
        "nextCalendarBase expects a real YY.M.D date, got \"{base}\""
    );
    let [mut yy, mut month, mut day] = parse_base(base).expect("validated base parses");
    day += 1;
    let month_index = usize::try_from(month).expect("validated month is non-negative");
    if day > DAYS_IN_MONTH[month_index] {
        day = 1;
        month += 1;
        if month > 12 {
            month = 1;
            yy = (yy + 1).rem_euclid(100);
        }
    }
    format!("{yy}.{month}.{day}")
}

/// Compute a pure next `CalVer` string from supplied tags/package state.
///
/// # Errors
/// Returns an error when effective base validation detects corrupted package
/// `CalVer` data.
pub fn compute_version(
    args: ComputeArgs,
    tags: &[String],
    package_version: &str,
) -> Result<String, String> {
    let today_base = date_base(args.now);
    let base = if args.stable {
        today_base
    } else {
        effective_base(&today_base, package_version)?
    };
    if args.stable {
        return Ok(base);
    }

    let channel = args.channel.unwrap_or(Channel::Alpha);
    let stamp = i32::try_from(
        args.now
            .hour
            .saturating_mul(100)
            .saturating_add(args.now.minute),
    )
    .map_err(|_| "HHMM stamp overflow".to_owned())?;
    let max_existing = max_n_from_tags(&base, channel, tags).max(max_n_from_package_json(
        &base,
        channel,
        package_version,
    ));
    let version_base = if stamp > max_existing {
        base
    } else {
        next_calendar_base(&base)
    };
    Ok(format!("{version_base}-{}.{stamp}", channel.as_str()))
}

fn parse_base(base: &str) -> Option<[i32; 3]> {
    let mut parts = base.split('.').map(|part| part.parse::<i32>().ok());
    let parsed = [parts.next()??, parts.next()??, parts.next()??];
    if parts.next().is_some() {
        return None;
    }
    Some(parsed)
}

#[cfg(test)]
mod calver_regression_tests;
