use maw_schedule_launchd::{load_config, remove_job, sync_job, DesiredJob, JobState, LaunchctlOutput,
    LaunchctlRunner, PlistState, SyncMode};
use std::path::PathBuf;
#[rustfmt::skip]
#[derive(Default)]
struct FakeLaunchctl { loaded: bool, calls: Vec<Vec<String>>, fail: bool }
#[rustfmt::skip]
impl LaunchctlRunner for FakeLaunchctl {
    fn run(&mut self, args: &[String]) -> Result<LaunchctlOutput, String> {
        self.calls.push(args.to_vec());
        if self.fail { return Ok(LaunchctlOutput { success: false, stderr: "denied".into() }); }
        match args[0].as_str() { "bootout" => self.loaded = false, "bootstrap" => self.loaded = true, _ => {} }
        Ok(LaunchctlOutput { success: args[0] != "print" || self.loaded, stderr: String::new() })
    }
}
#[rustfmt::skip]
#[test]
fn check_reads_config_and_reports_drift_without_mutation() {
    let root = temp("check");
    let config = root.join("schedule.toml");
    std::fs::write(&config, "[[schedule]]\nid='digest'\ncommand='run'\ncadence='every 1h'\n").unwrap();
    assert_eq!(load_config(&config).unwrap().schedule[0].id, "digest");
    let job = job(&root);
    let mut runner = FakeLaunchctl::default();
    let result = sync_job(&job, "gui/501", SyncMode::Check, &mut runner).unwrap();
    assert_eq!(result.before, JobState { plist: PlistState::Missing, loaded: false });
    assert_eq!(result.after, result.before);
    assert!(!result.changed && !job.plist_path.exists());
    assert_eq!(commands(&runner), ["print"]);
}
#[rustfmt::skip]
#[test]
fn apply_atomically_replaces_and_reloads_changed_job() {
    let root = temp("changed"); let job = job(&root);
    std::fs::write(&job.plist_path, "old").unwrap();
    let mut runner = FakeLaunchctl { loaded: true, ..FakeLaunchctl::default() };
    let result = sync_job(&job, "gui/501", SyncMode::Apply, &mut runner).unwrap();
    assert_eq!(result.before, JobState { plist: PlistState::Changed, loaded: true });
    assert!(result.after.is_healthy() && result.changed);
    assert_eq!(std::fs::read_to_string(&job.plist_path).unwrap(), job.xml);
    assert_eq!(commands(&runner), ["print", "bootout", "bootstrap", "print"]);
    assert_eq!(runner.calls[1][1], "gui/501/com.maw.schedule.test.digest");
    assert_eq!(runner.calls[2][1..], ["gui/501", job.plist_path.to_str().unwrap()]);
}
#[rustfmt::skip]
#[test]
fn apply_bootstraps_missing_job_and_healthy_job_is_noop() {
    let root = temp("missing"); let job = job(&root); let mut runner = FakeLaunchctl::default();
    assert!(sync_job(&job, "gui/501", SyncMode::Apply, &mut runner).unwrap().after.is_healthy());
    assert_eq!(commands(&runner), ["print", "bootstrap", "print"]);
    runner.calls.clear();
    assert!(!sync_job(&job, "gui/501", SyncMode::Apply, &mut runner).unwrap().changed);
    assert_eq!(commands(&runner), ["print"]);
    runner.calls.clear();
    assert!(remove_job(&job.label, &job.plist_path, "gui/501", SyncMode::Check, &mut runner).unwrap());
    assert!(job.plist_path.exists()); runner.calls.clear();
    assert!(remove_job(&job.label, &job.plist_path, "gui/501", SyncMode::Apply, &mut runner).unwrap());
    assert!(!job.plist_path.exists()); assert_eq!(commands(&runner), ["print", "bootout"]);
}
#[rustfmt::skip]
#[test]
fn invalid_targets_and_launchctl_failures_are_loud() {
    let root = temp("errors"); let job = job(&root); let mut runner = FakeLaunchctl::default();
    assert!(sync_job(&job, "user/501", SyncMode::Check, &mut runner).unwrap_err().contains("invalid"));
    runner.fail = true;
    assert!(sync_job(&job, "gui/501", SyncMode::Apply, &mut runner).unwrap_err().contains("bootstrap"));
}
fn commands(runner: &FakeLaunchctl) -> Vec<&str> {
    runner.calls.iter().map(|args| args[0].as_str()).collect()
}
#[rustfmt::skip]
fn job(root: &std::path::Path) -> DesiredJob {
    DesiredJob { label: "com.maw.schedule.test.digest".into(), plist_path: root.join("job.plist"), xml: "<plist/>\n".into() }
}
fn temp(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("schedule-launchd-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path); std::fs::create_dir_all(&path).unwrap(); path
}
