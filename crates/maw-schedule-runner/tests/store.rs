use maw_schedule::{ExecMode, RunStatus};
use maw_schedule_runner::{FinishRequest, FireStore, StartRequest};
use std::{path::PathBuf, sync::atomic::{AtomicU64, Ordering}};
#[rustfmt::skip]
fn root() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!("maw-schedule-store-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
    std::fs::create_dir_all(&path).unwrap(); path
}
#[rustfmt::skip]
fn request(id: &str, at: u64) -> StartRequest {
    StartRequest { run_id: id.into(), oracle: "odin".into(), job_id: "digest".into(),
        local_date: "2026-07-13".into(), reserved_at: at, cadence_seconds: 60,
        boot_identity: "boot-a".into(), cap: 1, forced: false, exec: ExecMode::Shell,
        expected_output: None }
}
#[rustfmt::skip]
fn finish(at: u64, exit: i32) -> FinishRequest {
    FinishRequest { exited_at: at, exit_code: exit, output_file_written: false,
        output_bytes: 0, deliverable_written: None, error: (exit != 0).then(|| "child failed".into()) }
}
#[test]
#[rustfmt::skip]
fn failed_run_releases_slot_and_success_commits_legacy_counter_and_witness() {
    let root = root();
    let store = FireStore::new(root.clone());
    assert_eq!(store.reserve({ let mut run = request("first", 100); run.expected_output = Some("digest.md".into()); run }).unwrap().outcome.status, RunStatus::Reserved);
    assert_eq!(store.reserve(request("blocked", 101)).unwrap().outcome.status, RunStatus::CapHit);
    store.spawned("first", 101, None).unwrap();
    let failed = store.finalize("first", { let mut end = finish(102, 0); end.output_file_written = true; end.deliverable_written = Some(false); end }).unwrap();
    assert_eq!((failed.outcome.status, failed.outcome.cap_committed), (RunStatus::CompletedWithoutDeliverable, false));
    assert_eq!(store.reserve(request("retry", 103)).unwrap().outcome.status, RunStatus::Reserved);
    store.spawned("retry", 103, Some("digest.log".into())).unwrap();
    let success = store.finalize("retry", finish(104, 0)).unwrap();
    assert_eq!((success.outcome.status, success.outcome.cap_committed), (RunStatus::Succeeded, true));
    let counters: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("fires.json")).unwrap()).unwrap();
    assert_eq!(counters["2026-07-13"]["odin.digest"], 1);
    let latest: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("schedule/runs/latest.json")).unwrap()).unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&std::fs::read(root.join("schedule/runs/first.json")).unwrap()).unwrap()["deliverable_written"], false);
    assert_eq!(latest["jobs"]["odin.digest"]["outcome_path"], "runs/retry.json");
    assert!(store.finalize("retry", finish(105, 0)).unwrap_err().contains("terminal"));
    assert!(store.reserve(request("retry", 106)).unwrap_err().contains("exists"));
}
#[test]
#[rustfmt::skip]
fn stale_and_boot_changed_reservations_are_abandoned_and_corruption_fails_closed() {
    let root = root();
    let store = FireStore::new(root.clone());
    let mut old = request("old", 100); old.cadence_seconds = 10;
    store.reserve(old).unwrap();
    assert_eq!(store.reserve(request("after-age", 121)).unwrap().outcome.status, RunStatus::Reserved);
    let old: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("schedule/runs/old.json")).unwrap()).unwrap();
    assert_eq!(old["status"], "abandoned");
    store.spawned("after-age", 121, None).unwrap();
    store.finalize("after-age", finish(122, 1)).unwrap();
    store.reserve(request("same-boot", 123)).unwrap();
    let mut boot = request("boot", 123); boot.boot_identity = "boot-b".into();
    assert_eq!(store.reserve(boot).unwrap().outcome.status, RunStatus::Reserved);
    let old: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("schedule/runs/same-boot.json")).unwrap()).unwrap();
    assert_eq!(old["status"], "abandoned");
    std::fs::write(root.join("fires.json"), b"not-json").unwrap();
    assert!(store.reserve(request("closed", 124)).unwrap_err().contains("parse"));
}
#[test]
#[rustfmt::skip]
fn concurrent_reservations_share_the_advisory_lock() {
    let store = FireStore::new(root());
    let threads = (0..8).map(|n| { let store = store.clone(); std::thread::spawn(move ||
        store.reserve(request(&format!("run-{n}"), 100)).unwrap().outcome.status) }).collect::<Vec<_>>();
    let statuses = threads.into_iter().map(|thread| thread.join().unwrap()).collect::<Vec<_>>();
    assert_eq!(statuses.iter().filter(|status| **status == RunStatus::Reserved).count(), 1);
    assert_eq!(statuses.iter().filter(|status| **status == RunStatus::CapHit).count(), 7);
}
