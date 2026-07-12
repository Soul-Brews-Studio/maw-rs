//! macOS schedule config, plist, and launchctl synchronization boundary.

use maw_schedule::{parse_schedule, ScheduleFile};
use std::path::{Path, PathBuf};

#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesiredJob { pub label: String, pub plist_path: PathBuf, pub xml: String }
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlistState { Missing, Changed, Current }
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobState { pub plist: PlistState, pub loaded: bool }
impl JobState {
    #[must_use]
    pub const fn is_healthy(self) -> bool {
        matches!(self.plist, PlistState::Current) && self.loaded
    }
}
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncMode { Check, Apply }
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncResult { pub before: JobState, pub after: JobState, pub changed: bool }
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchctlOutput { pub success: bool, pub stderr: String }
pub trait LaunchctlRunner {
    /// Execute `launchctl` with an argv vector and no shell.
    ///
    /// # Errors
    /// Returns spawn or transport failures.
    fn run(&mut self, args: &[String]) -> Result<LaunchctlOutput, String>;
}
pub struct SystemLaunchctl;
impl LaunchctlRunner for SystemLaunchctl {
    fn run(&mut self, args: &[String]) -> Result<LaunchctlOutput, String> {
        #[cfg(not(target_os = "macos"))]
        return Err("launchd scheduling is supported only on macOS".to_owned());
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("launchctl")
                .args(args)
                .output()
                .map_err(|error| format!("launchctl spawn failed: {error}"))?;
            Ok(LaunchctlOutput {
                success: output.status.success(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            })
        }
    }
}
/// Read the byte-compatible schedule TOML through the pure schema parser.
///
/// # Errors
/// Returns filesystem and TOML parse failures with the config path attached.
pub fn load_config(path: &Path) -> Result<ScheduleFile, String> {
    let body = std::fs::read_to_string(path)
        .map_err(|error| format!("read {}: {error}", path.display()))?;
    parse_schedule(&body).map_err(|error| format!("parse {}: {error}", path.display()))
}
/// Inspect or repair one desired launchd job.
///
/// `Check` is non-mutating. `Apply` atomically replaces a missing/changed plist and
/// bootstraps unloaded jobs, booting out a loaded job only when its plist changes.
///
/// # Errors
/// Returns invalid target, filesystem, or launchctl failures.
pub fn sync_job<R: LaunchctlRunner>(
    job: &DesiredJob,
    domain: &str,
    mode: SyncMode,
    runner: &mut R,
) -> Result<SyncResult, String> {
    validate_target(&job.label, domain)?;
    let before = inspect(job, domain, runner)?;
    if mode == SyncMode::Check || before.is_healthy() {
        return Ok(SyncResult { before, after: before, changed: false });
    }
    let plist_changed = before.plist != PlistState::Current;
    if plist_changed {
        atomic_write(&job.plist_path, &job.xml)?;
    }
    if before.loaded && plist_changed {
        launchctl(runner, &["bootout", &format!("{domain}/{}", job.label)])?;
    }
    if !before.loaded || plist_changed {
        launchctl(runner, &["bootstrap", domain, &job.plist_path.to_string_lossy()])?;
    }
    let after = inspect(job, domain, runner)?;
    if !after.is_healthy() {
        return Err(format!("{} remains out of sync after repair", job.label));
    }
    Ok(SyncResult { before, after, changed: true })
}
/// Check or remove a stale plist and its loaded launchd job.
///
/// # Errors
/// Returns invalid target, filesystem, or launchctl failures.
pub fn remove_job<R: LaunchctlRunner>(label: &str, path: &Path, domain: &str,
    mode: SyncMode, runner: &mut R) -> Result<bool, String> {
    validate_target(label, domain)?;
    let exists = path.try_exists().map_err(|error| format!("inspect {}: {error}", path.display()))?;
    let target = format!("{domain}/{label}");
    let loaded = runner.run(&["print".to_owned(), target.clone()])?.success;
    if mode == SyncMode::Check { return Ok(exists || loaded); }
    if loaded { launchctl(runner, &["bootout", &target])?; }
    if exists { std::fs::remove_file(path).map_err(|error| format!("remove {}: {error}", path.display()))?; }
    Ok(exists || loaded)
}
fn inspect<R: LaunchctlRunner>(
    job: &DesiredJob,
    domain: &str,
    runner: &mut R,
) -> Result<JobState, String> {
    let plist = match std::fs::read_to_string(&job.plist_path) {
        Ok(body) if body == job.xml => PlistState::Current,
        Ok(_) => PlistState::Changed,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => PlistState::Missing,
        Err(error) => return Err(format!("read {}: {error}", job.plist_path.display())),
    };
    let target = format!("{domain}/{}", job.label);
    let loaded = runner.run(&["print".to_owned(), target])?.success;
    Ok(JobState { plist, loaded })
}
fn launchctl<R: LaunchctlRunner>(runner: &mut R, args: &[&str]) -> Result<(), String> {
    let args = args.iter().map(|value| (*value).to_owned()).collect::<Vec<_>>();
    let output = runner.run(&args)?;
    if output.success {
        Ok(())
    } else {
        Err(format!("launchctl {} failed: {}", args[0], output.stderr))
    }
}
fn atomic_write(path: &Path, body: &str) -> Result<(), String> {
    let parent = path.parent().ok_or_else(|| "plist path has no parent".to_owned())?;
    std::fs::create_dir_all(parent)
        .map_err(|error| format!("create {}: {error}", parent.display()))?;
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("schedule.plist");
    let temp = path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    std::fs::write(&temp, body).map_err(|error| format!("write {}: {error}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|error| format!("replace {}: {error}", path.display()))
}
fn validate_target(label: &str, domain: &str) -> Result<(), String> {
    let label_ok = !label.is_empty()
        && label.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte));
    let domain_ok = domain.strip_prefix("gui/").is_some_and(|uid|
        !uid.is_empty() && uid.bytes().all(|byte| byte.is_ascii_digit()));
    if label_ok && domain_ok { Ok(()) } else { Err("invalid launchctl label or domain".to_owned()) }
}
