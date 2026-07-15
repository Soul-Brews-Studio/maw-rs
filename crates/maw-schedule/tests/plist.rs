use maw_schedule::{
    plist::{parse_cadence, render_plist, CadenceError, CadencePlan, CalendarTime, LaunchdPlist},
    ExecMode, Schedule,
};
use serde::Deserialize;
const REPO: &str = "/opt/Code/github.com/Soul-Brews-Studio/odin-oracle";
const HOME: &str = "/Users/tester";
const PATH: &str = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";
#[rustfmt::skip]
#[derive(Deserialize)]
struct PythonFixture { schedule: Schedule, xml: String }
#[rustfmt::skip]
#[test]
fn renders_actual_python_scheduler_fixtures_byte_for_byte() {
    let fixtures: Vec<PythonFixture> = serde_json::from_str(include_str!("fixtures/python_plists.json")).unwrap();
    for fixture in fixtures {
        let schedule = fixture.schedule;
        let exec = match schedule.exec { ExecMode::ClaudeHeadless => "claude-headless", ExecMode::Shell => "shell" };
        let log = format!("{HOME}/.maw/state/logs/odin-oracle.{}.log", schedule.id);
        let job = LaunchdPlist {
            label: format!("com.maw.schedule.odin-oracle.{}", schedule.id),
            program_arguments: vec![format!("{HOME}/.maw/bin/maw-schedule-fire"), "odin-oracle".into(),
                schedule.id.clone(), schedule.command.clone(), REPO.into(),
                schedule.max_fires_per_day.to_string(), exec.into()],
            cadence: parse_cadence(&schedule).unwrap(), standard_out_path: log.clone(), standard_error_path: log,
            home: HOME.into(), path: PATH.into(), run_at_load: false,
        };
        assert_eq!(render_plist(&job), fixture.xml, "{}", schedule.id);
    }
}
#[rustfmt::skip]
#[test]
fn parses_hourly_and_daily_calendar_plans() {
    let CadencePlan::Calendar(times) = parse_cadence(&schedule(" every   1H ", Some(7), Some(12))).unwrap()
        else { panic!("expected calendar plan") };
    assert_eq!(times.len(), 24);
    assert_eq!((times[0], times[23]), (CalendarTime { hour: 0, minute: 7 }, CalendarTime { hour: 23, minute: 7 }));
    assert_eq!(parse_cadence(&schedule("daily at 09:05", None, None)).unwrap(),
        CadencePlan::Calendar(vec![CalendarTime { hour: 9, minute: 5 }]));
}
#[rustfmt::skip]
#[test]
fn rejects_invalid_or_dangerous_cadences() {
    for (value, minute, expected) in [("every 0m", None, CadenceError::NonPositive),
        ("every 25h", None, CadenceError::HourOutOfRange),
        ("every 1h", Some(60), CadenceError::MinuteOutOfRange),
        ("daily at 24:00", None, CadenceError::HourOutOfRange),
        ("daily at 9:5", None, CadenceError::Unsupported),
        ("sometimes", None, CadenceError::Unsupported)] {
        assert_eq!(parse_cadence(&schedule(value, minute, None)), Err(expected));
    }
}
#[rustfmt::skip]
#[test]
fn escapes_caller_supplied_xml_values() {
    let job = LaunchdPlist { label: "job<&\"'".into(), program_arguments: vec!["a&b".into()],
        cadence: CadencePlan::Interval { seconds: 60 }, standard_out_path: "out".into(),
        standard_error_path: "err".into(), home: HOME.into(), path: PATH.into(), run_at_load: true };
    let xml = render_plist(&job);
    assert!(xml.contains("job&lt;&amp;&quot;&apos;") && xml.contains("<string>a&amp;b</string>") && xml.contains("<true/>"));
}
#[rustfmt::skip]
fn schedule(cadence: &str, at_minute: Option<u8>, at_hour: Option<u8>) -> Schedule {
    Schedule { id: "test".into(), command: "run".into(), cadence: cadence.into(), max_fires_per_day: 24,
        exec: ExecMode::ClaudeHeadless, expected_output: None, token_name: "t2".into(), created: None, at_minute, at_hour }
}
