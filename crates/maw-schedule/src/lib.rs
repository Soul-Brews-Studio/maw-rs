//! Pure schedule configuration and fire lifecycle transitions.

pub mod plist;

use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct ScheduleFile {
    #[serde(default)]
    pub schedule: Vec<Schedule>,
}
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct Schedule {
    pub id: String,
    pub command: String,
    pub cadence: String,
    #[serde(default = "default_cap")]
    pub max_fires_per_day: u32,
    #[serde(default)]
    pub exec: ExecMode,
    pub expected_output: Option<String>,
    #[serde(default = "default_token")]
    pub token_name: String,
    pub created: Option<String>,
    pub at_minute: Option<u8>,
    pub at_hour: Option<u8>,
}
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ExecMode {
    #[default]
    ClaudeHeadless,
    Shell,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum RunStatus {
    Reserved,
    Spawned,
    Succeeded,
    Failed,
    CompletedWithoutDeliverable,
    Abandoned,
    CapHit,
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct OutcomeRecord {
    pub schema_version: u8,
    pub run_id: String,
    pub reserved_at: u64,
    pub spawned_at: Option<u64>,
    pub exited_at: Option<u64>,
    pub status: RunStatus,
    pub exit_code: Option<i32>,
    pub forced: bool,
    pub cap_committed: bool,
    pub output_file_written: Option<bool>,
    pub deliverable_written: Option<bool>,
    pub expected_output: Option<String>,
    pub cadence_seconds: u64,
    pub boot_identity: String,
    pub exec: ExecMode,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReserveRequest {
    pub run_id: String,
    pub reserved_at: u64,
    pub cadence_seconds: u64,
    pub boot_identity: String,
    pub cap: u32,
    pub committed: u32,
    pub active_reservations: u32,
    pub forced: bool,
    pub exec: ExecMode,
    pub expected_output: Option<String>,
}
/// Parse the Python-compatible `[[schedule]]` TOML shape.
///
/// # Errors
/// Returns TOML syntax or type errors without performing I/O.
pub fn parse_schedule(input: &str) -> Result<ScheduleFile, toml::de::Error> {
    toml::from_str(input)
}
#[must_use]
pub fn reserve(request: ReserveRequest) -> OutcomeRecord {
    let cap_hit = !request.forced
        && request
            .committed
            .saturating_add(request.active_reservations)
            >= request.cap;
    let status = if cap_hit {
        RunStatus::CapHit
    } else {
        RunStatus::Reserved
    };
    OutcomeRecord {
        schema_version: 1,
        run_id: request.run_id,
        reserved_at: request.reserved_at,
        spawned_at: None,
        exited_at: cap_hit.then_some(request.reserved_at),
        status,
        exit_code: None,
        forced: request.forced,
        cap_committed: false,
        output_file_written: None,
        deliverable_written: None,
        expected_output: request.expected_output,
        cadence_seconds: request.cadence_seconds,
        boot_identity: request.boot_identity,
        exec: request.exec,
    }
}
#[must_use]
pub fn mark_spawned(run: &mut OutcomeRecord, at: u64) -> bool {
    if run.status != RunStatus::Reserved {
        return false;
    }
    run.status = RunStatus::Spawned;
    run.spawned_at = Some(at);
    true
}
/// Finalize an active run and return whether its quota counter should be committed.
#[must_use]
pub fn finalize(
    run: &mut OutcomeRecord,
    exited_at: u64,
    exit_code: i32,
    output_file_written: bool,
    deliverable_written: Option<bool>,
) -> Option<bool> {
    if !run.status.is_active() {
        return None;
    }
    run.exited_at = Some(exited_at);
    run.exit_code = Some(exit_code);
    run.output_file_written = Some(output_file_written);
    run.deliverable_written = run
        .expected_output
        .as_ref()
        .map(|_| deliverable_written == Some(true));
    let transport_ok = run.spawned_at.is_some()
        && exit_code == 0
        && (run.exec == ExecMode::Shell || output_file_written);
    run.status = if transport_ok && run.deliverable_written != Some(false) {
        run.cap_committed = true;
        RunStatus::Succeeded
    } else if transport_ok {
        RunStatus::CompletedWithoutDeliverable
    } else {
        RunStatus::Failed
    };
    Some(run.cap_committed)
}
#[must_use]
pub fn abandon_if_stale(run: &mut OutcomeRecord, now: u64, boot_identity: &str) -> bool {
    let expired = now.saturating_sub(run.reserved_at) > run.cadence_seconds.saturating_mul(2);
    if !run.status.is_active() || (!expired && run.boot_identity == boot_identity) {
        return false;
    }
    run.status = RunStatus::Abandoned;
    run.exited_at = Some(now);
    true
}
impl RunStatus {
    const fn is_active(self) -> bool {
        matches!(self, Self::Reserved | Self::Spawned)
    }
}
const fn default_cap() -> u32 {
    24
}
fn default_token() -> String {
    "t2".to_owned()
}
#[cfg(test)]
mod tests {
    use super::*;
    fn request(id: &str) -> ReserveRequest {
        ReserveRequest {
            run_id: id.into(), reserved_at: 100, cadence_seconds: 60,
            boot_identity: "boot-a".into(), cap: 1, committed: 0,
            active_reservations: 0, forced: false, exec: ExecMode::ClaudeHeadless,
            expected_output: None,
        }
    }
    #[test]
    fn parses_python_toml_defaults_and_extensions() {
        let file = parse_schedule(concat!(
            "[[schedule]]\nid=\"huginn\"\ncommand=\"digest\"\ncadence=\"every 1h\"\nunknown=\"ok\"\n",
            "[[schedule]]\nid=\"argus\"\ncommand=\"rotate\"\ncadence=\"daily at 00:15\"\n",
            "exec=\"shell\"\nexpected_output=\"ψ/memory/$TODAY/result.md\"\ntoken_name=\"account-b\"\n",
        )).unwrap();
        assert_eq!((file.schedule[0].max_fires_per_day, file.schedule[0].token_name.as_str()),
            (24, "t2"));
        assert_eq!(file.schedule[1].exec, ExecMode::Shell);
        assert_eq!(file.schedule[1].expected_output.as_deref(),
            Some("ψ/memory/$TODAY/result.md"));
    }
    #[test]
    fn quota_commits_only_for_complete_success() {
        let capped = reserve(ReserveRequest { active_reservations: 1, ..request("cap") });
        assert_eq!(capped.status, RunStatus::CapHit);
        let forced = reserve(ReserveRequest { active_reservations: 1, forced: true,
            ..request("force") });
        assert_eq!(forced.status, RunStatus::Reserved);
        let mut failed = reserve(request("failed"));
        assert!(mark_spawned(&mut failed, 110));
        assert_eq!(finalize(&mut failed, 120, 0, false, None), Some(false));
        assert_eq!((failed.status, failed.output_file_written), (RunStatus::Failed, Some(false)));
        assert_eq!(reserve(request("retry")).status, RunStatus::Reserved);
        let mut missing = reserve(ReserveRequest { expected_output: Some("digest.md".into()),
            ..request("missing") });
        assert!(mark_spawned(&mut missing, 110));
        assert_eq!(finalize(&mut missing, 120, 0, true, Some(false)), Some(false));
        assert_eq!((missing.status, missing.deliverable_written, missing.cap_committed),
            (RunStatus::CompletedWithoutDeliverable, Some(false), false));
        let mut success = reserve(ReserveRequest { expected_output: Some("digest.md".into()),
            ..request("success") });
        assert!(mark_spawned(&mut success, 110));
        assert_eq!(finalize(&mut success, 120, 0, true, Some(true)), Some(true));
        assert_eq!((success.status, success.cap_committed), (RunStatus::Succeeded, true));
        assert_eq!(finalize(&mut success, 121, 0, true, Some(true)), None);
    }
    #[test]
    fn stale_bound_is_strict_and_boot_change_abandons() {
        let mut age = reserve(request("age"));
        assert!(mark_spawned(&mut age, 105));
        assert!(!abandon_if_stale(&mut age, 220, "boot-a"));
        assert!(abandon_if_stale(&mut age, 221, "boot-a"));
        assert_eq!((age.status, age.spawned_at, age.exited_at, age.cap_committed),
            (RunStatus::Abandoned, Some(105), Some(221), false));
        let mut boot = reserve(request("boot"));
        assert!(abandon_if_stale(&mut boot, 100, "boot-b"));
        assert_eq!(boot.deliverable_written, None);
    }
}
