use fd_lock::RwLock;
use maw_schedule::{
    abandon_if_stale, finalize, mark_spawned, reserve, ExecMode, OutcomeRecord, ReserveRequest,
    RunStatus,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{collections::BTreeMap, fs::OpenOptions, io::Write, path::{Path, PathBuf}};
type Counters = BTreeMap<String, BTreeMap<String, u32>>;
#[rustfmt::skip] #[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartRequest { pub run_id: String, pub oracle: String, pub job_id: String, pub local_date: String, pub reserved_at: u64, pub cadence_seconds: u64, pub boot_identity: String, pub cap: u32, pub forced: bool, pub exec: ExecMode, pub expected_output: Option<String> }
#[rustfmt::skip] #[derive(Debug, Clone, PartialEq, Eq)]
pub struct FinishRequest { pub exited_at: u64, pub exit_code: i32, pub output_file_written: bool, pub output_bytes: u64, pub deliverable_written: Option<bool>, pub error: Option<String> }
#[rustfmt::skip] #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredRun { #[serde(flatten)] pub outcome: OutcomeRecord, pub oracle: String, pub job_id: String, pub local_date: String, pub error: Option<String>, pub output_path: Option<String>, pub output_bytes: u64 }
#[rustfmt::skip] #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LatestRun { pub cadence_seconds: u64, pub run_id: String, pub status: RunStatus, pub updated_at: u64, pub deliverable_written: Option<bool>, pub outcome_path: String }
#[rustfmt::skip] #[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LatestIndex { pub schema_version: u8, pub generated_at: u64, pub jobs: BTreeMap<String, LatestRun> }
#[rustfmt::skip] #[derive(Debug, Clone)] pub struct FireStore { root: PathBuf }
impl FireStore {
    #[must_use]
    pub fn new(root: PathBuf) -> Self { Self { root } }
    /// Reserve a quota slot and atomically publish its run/latest witnesses.
    ///
    /// # Errors
    /// Fails closed on lock, corrupt state, validation, or filesystem errors.
    #[rustfmt::skip]
    pub fn reserve(&self, request: StartRequest) -> Result<StoredRun, String> {
        validate(&request.run_id)?;
        let key = format!("{}.{}", request.oracle, request.job_id);
        self.locked(|| {
            if self.run_path(&request.run_id).try_exists().map_err(|error| format!("inspect run id: {error}"))? { return Err("run id already exists".to_owned()); }
            let counters: Counters = read_or_default(&self.root.join("fires.json"))?;
            let mut active = 0;
            for path in self.run_paths()? {
                let mut run: StoredRun = read_json(&path)?;
                if format!("{}.{}", run.oracle, run.job_id) != key { continue; }
                if abandon_if_stale(&mut run.outcome, request.reserved_at, &request.boot_identity) {
                    atomic_json(&path, &run)?;
                } else if run.outcome.status.is_active() { active += 1; }
            }
            let committed = counters.get(&request.local_date).and_then(|day| day.get(&key)).copied().unwrap_or(0);
            let outcome = reserve(ReserveRequest { run_id: request.run_id, reserved_at: request.reserved_at,
                cadence_seconds: request.cadence_seconds, boot_identity: request.boot_identity,
                cap: request.cap, committed, active_reservations: active, forced: request.forced,
                exec: request.exec, expected_output: request.expected_output });
            let run = StoredRun { outcome, oracle: request.oracle, job_id: request.job_id,
                local_date: request.local_date, error: None, output_path: None, output_bytes: 0 };
            self.publish(&run)?;
            Ok(run)
        })
    }
    /// Publish the spawned transition before waiting for child completion.
    ///
    /// # Errors
    /// Fails closed if the reservation is missing, corrupt, or terminal.
    #[rustfmt::skip]
    pub fn spawned(&self, run_id: &str, at: u64, output_path: Option<String>) -> Result<StoredRun, String> {
        validate(run_id)?;
        self.locked(|| {
            let mut run: StoredRun = read_json(&self.run_path(run_id))?;
            if !mark_spawned(&mut run.outcome, at) { return Err(format!("run {run_id} is not reserved")); }
            run.output_path = output_path;
            self.publish(&run)?;
            Ok(run)
        })
    }
    /// Finalize a reservation, committing legacy quota only for successful work.
    ///
    /// # Errors
    /// Fails closed on missing/corrupt state or attempts to rewrite a terminal record.
    #[rustfmt::skip]
    pub fn finalize(&self, run_id: &str, finish: FinishRequest) -> Result<StoredRun, String> {
        validate(run_id)?;
        self.locked(|| {
            let mut run: StoredRun = read_json(&self.run_path(run_id))?;
            let commit = finalize(&mut run.outcome, finish.exited_at, finish.exit_code,
                finish.output_file_written, finish.deliverable_written)
                .ok_or_else(|| format!("run {run_id} is already terminal"))?;
            run.output_bytes = finish.output_bytes;
            run.error = finish.error;
            if commit {
                let mut counters: Counters = read_or_default(&self.root.join("fires.json"))?;
                *counters.entry(run.local_date.clone()).or_default()
                    .entry(format!("{}.{}", run.oracle, run.job_id)).or_default() += 1;
                while counters.len() > 7 { counters.pop_first(); }
                atomic_json(&self.root.join("fires.json"), &counters)?;
            }
            self.publish(&run)?;
            Ok(run)
        })
    }
    #[rustfmt::skip]
    fn publish(&self, run: &StoredRun) -> Result<(), String> {
        atomic_json(&self.run_path(&run.outcome.run_id), run)?;
        let path = self.runs_dir().join("latest.json");
        let mut latest: LatestIndex = read_or_default(&path)?;
        latest.schema_version = 1;
        latest.generated_at = run.outcome.exited_at.or(run.outcome.spawned_at).unwrap_or(run.outcome.reserved_at);
        latest.jobs.insert(format!("{}.{}", run.oracle, run.job_id), LatestRun {
            cadence_seconds: run.outcome.cadence_seconds, run_id: run.outcome.run_id.clone(),
            status: run.outcome.status, updated_at: latest.generated_at,
            deliverable_written: run.outcome.deliverable_written,
            outcome_path: format!("runs/{}.json", run.outcome.run_id),
        });
        atomic_json(&path, &latest)
    }
    #[rustfmt::skip]
    fn locked<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        std::fs::create_dir_all(self.runs_dir()).map_err(|error| format!("create state: {error}"))?;
        let file = OpenOptions::new().create(true).truncate(false).read(true).write(true)
            .open(self.root.join("fires.json.lock")).map_err(|error| format!("open fire lock: {error}"))?;
        let mut lock = RwLock::new(file);
        let _guard = lock.write().map_err(|error| format!("lock fires: {error}"))?;
        operation()
    }
    #[rustfmt::skip]
    fn run_paths(&self) -> Result<Vec<PathBuf>, String> {
        Ok(std::fs::read_dir(self.runs_dir()).map_err(|error| format!("read runs: {error}"))?
            .filter_map(|entry| entry.ok().map(|value| value.path()))
            .filter(|path| path.extension().is_some_and(|ext| ext == "json")
                && path.file_name().is_some_and(|name| name != "latest.json")).collect())
    }
    fn runs_dir(&self) -> PathBuf { self.root.join("schedule/runs") }
    fn run_path(&self, id: &str) -> PathBuf { self.runs_dir().join(format!("{id}.json")) }
}
impl Default for LatestIndex {
    fn default() -> Self { Self { schema_version: 1, generated_at: 0, jobs: BTreeMap::new() } }
}
#[rustfmt::skip]
fn validate(value: &str) -> Result<(), String> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte)) { Ok(()) }
    else { Err("invalid run id".to_owned()) }
}
#[rustfmt::skip]
fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T, String> {
    let body = std::fs::read(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    serde_json::from_slice(&body).map_err(|error| format!("parse {}: {error}", path.display()))
}
#[rustfmt::skip]
fn read_or_default<T: DeserializeOwned + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read(path) {
        Ok(body) => serde_json::from_slice(&body).map_err(|error| format!("parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(error) => Err(format!("read {}: {error}", path.display())),
    }
}
#[rustfmt::skip]
fn atomic_json(path: &Path, value: &impl Serialize) -> Result<(), String> {
    let mut body = serde_json::to_vec(value).map_err(|error| format!("encode state: {error}"))?;
    body.push(b'\n');
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let mut file = OpenOptions::new().create(true).truncate(true).write(true).open(&temp)
        .map_err(|error| format!("write {}: {error}", temp.display()))?;
    file.write_all(&body).and_then(|()| file.sync_all())
        .map_err(|error| format!("flush {}: {error}", temp.display()))?;
    std::fs::rename(&temp, path).map_err(|error| format!("replace {}: {error}", path.display()))
}
