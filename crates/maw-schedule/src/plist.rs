//! Validated cadence plans and deterministic launchd plist rendering.

use crate::Schedule;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CadencePlan {
    Interval { seconds: u32 },
    Calendar(Vec<CalendarTime>),
}
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarTime { pub hour: u8, pub minute: u8 }
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CadenceError {
    Unsupported, NonPositive, HourOutOfRange, MinuteOutOfRange, IntervalOverflow,
}
impl std::fmt::Display for CadenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::Unsupported => "unsupported cadence",
            Self::NonPositive => "cadence interval must be positive",
            Self::HourOutOfRange => "hour must be between 0 and 23",
            Self::MinuteOutOfRange => "minute must be between 0 and 59",
            Self::IntervalOverflow => "cadence interval is too large",
        };
        formatter.write_str(message)
    }
}
impl std::error::Error for CadenceError {}
/// Parse the cadence forms accepted by the legacy Python scheduler.
///
/// # Errors
/// Rejects unsupported forms, zero intervals, overflow, and invalid clock values.
pub fn parse_cadence(schedule: &Schedule) -> Result<CadencePlan, CadenceError> {
    let cadence = schedule
        .cadence
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase();
    validate_clock(schedule.at_hour, schedule.at_minute)?;
    if let Some(value) = cadence.strip_prefix("every ") {
        if let Some(minutes) = value
            .strip_suffix("min")
            .or_else(|| value.strip_suffix('m'))
        {
            let minutes = positive(minutes.trim())?;
            let seconds = minutes
                .checked_mul(60)
                .ok_or(CadenceError::IntervalOverflow)?;
            return Ok(CadencePlan::Interval { seconds });
        }
        if let Some(hours) = value.strip_suffix('h') {
            let hours = positive(hours.trim())?;
            if hours > 24 {
                return Err(CadenceError::HourOutOfRange);
            }
            let minute = schedule.at_minute.unwrap_or(0);
            let start = if hours == 1 {
                0_u32
            } else {
                u32::from(schedule.at_hour.unwrap_or(0))
            };
            let times = (0..24 / hours)
                .map(|index| CalendarTime {
                    hour: ((start + index * hours) % 24) as u8,
                    minute,
                })
                .collect();
            return Ok(CadencePlan::Calendar(times));
        }
    }
    if let Some(time) = cadence.strip_prefix("daily at ") {
        let (hour, minute) = time.split_once(':').ok_or(CadenceError::Unsupported)?;
        if !(1..=2).contains(&hour.len()) || minute.len() != 2 {
            return Err(CadenceError::Unsupported);
        }
        let hour = hour.parse::<u8>().map_err(|_| CadenceError::Unsupported)?;
        let minute = minute
            .parse::<u8>()
            .map_err(|_| CadenceError::Unsupported)?;
        validate_clock(Some(hour), Some(minute))?;
        return Ok(CadencePlan::Calendar(vec![CalendarTime { hour, minute }]));
    }
    Err(CadenceError::Unsupported)
}
fn positive(value: &str) -> Result<u32, CadenceError> {
    match value.parse::<u32>() {
        Ok(0) => Err(CadenceError::NonPositive),
        Ok(value) => Ok(value),
        Err(_) => Err(CadenceError::Unsupported),
    }
}
fn validate_clock(hour: Option<u8>, minute: Option<u8>) -> Result<(), CadenceError> {
    if hour.is_some_and(|value| value > 23) {
        return Err(CadenceError::HourOutOfRange);
    }
    if minute.is_some_and(|value| value > 59) {
        return Err(CadenceError::MinuteOutOfRange);
    }
    Ok(())
}
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchdPlist {
    pub label: String, pub program_arguments: Vec<String>, pub cadence: CadencePlan,
    pub standard_out_path: String, pub standard_error_path: String,
    pub home: String, pub path: String, pub run_at_load: bool,
}

#[must_use]
pub fn render_plist(job: &LaunchdPlist) -> String {
    let arguments = job
        .program_arguments
        .iter()
        .map(|value| format!("      <string>{}</string>", escape_xml(value)))
        .collect::<Vec<_>>()
        .join("\n");
    let schedule = match &job.cadence {
        CadencePlan::Interval { seconds } => {
            format!("  <key>StartInterval</key>\n  <integer>{seconds}</integer>")
        }
        CadencePlan::Calendar(times) => {
            let entries = times.iter().map(|time| format!(
                "    <dict>\n      <key>Hour</key>\n      <integer>{}</integer>\n      <key>Minute</key>\n      <integer>{}</integer>\n    </dict>",
                time.hour, time.minute
            )).collect::<Vec<_>>().join("\n");
            format!("  <key>StartCalendarInterval</key>\n  <array>\n{entries}\n  </array>")
        }
    };
    let enabled = if job.run_at_load { "true" } else { "false" };
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
{arguments}
  </array>
{schedule}
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
  <key>EnvironmentVariables</key>
  <dict>
    <key>PATH</key>
    <string>{}</string>
    <key>HOME</key>
    <string>{}</string>
  </dict>
  <key>RunAtLoad</key>
  <{enabled}/>
</dict>
</plist>
"#,
        escape_xml(&job.label),
        escape_xml(&job.standard_out_path),
        escape_xml(&job.standard_error_path),
        escape_xml(&job.path),
        escape_xml(&job.home)
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
