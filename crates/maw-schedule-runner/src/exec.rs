//! Native schedule child execution and terminal outcome finalization.
use crate::{FinishRequest, FireStore, StoredRun};
use maw_schedule::ExecMode;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    process::{Command, Stdio},
    time::{SystemTime, UNIX_EPOCH},
};
const BIN_DIRS: &[&str] = &["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/bin"];
#[rustfmt::skip] #[derive(Debug)]
struct Attempt { code: i32, output_written: bool, output_bytes: u64, deliverable: Option<bool>, expected: Option<String>, error: Option<String> }
/// Resolve an executable without consulting a shell profile or ambient `PATH`.
///
/// # Errors
/// Returns an error when no absolute candidate is a file.
pub fn resolve_binary(name: &str) -> Result<PathBuf, String> {
    resolve_binary_in(
        name,
        &BIN_DIRS.iter().map(PathBuf::from).collect::<Vec<_>>(),
    )
}
/// Resolve from explicit search roots, primarily for deterministic tests and sync planning.
///
/// # Errors
/// Returns an error when no absolute candidate is a file.
#[rustfmt::skip]
pub fn resolve_binary_in(name: &str, roots: &[PathBuf]) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains('/') { return Err("binary name must be a basename".to_owned()); }
    roots.iter().map(|root| root.join(name)).find(|path| path.is_absolute() && path.is_file())
        .ok_or_else(|| format!("required binary {name} not found in fixed search path"))
}
/// Execute one reserved run and always publish/log a terminal outcome after the log opens.
///
/// Execution failures return a terminal `StoredRun`; only log/state failures return `Err`.
///
/// # Errors
/// Returns log I/O or persistence failures.
#[rustfmt::skip]
pub fn execute(store: &FireStore, run_id: &str, today: &str, hour: &str,
    existing_token: Option<&str>) -> Result<StoredRun, String> {
    let run = store.load(run_id)?;
    let log_path = Path::new(&run.log_path);
    if let Some(parent) = log_path.parent() { std::fs::create_dir_all(parent).map_err(|e| format!("create log dir: {e}"))?; }
    let mut log = OpenOptions::new().create(true).append(true).open(log_path)
        .map_err(|e| format!("open schedule log {}: {e}", log_path.display()))?;
    writeln!(log, "[{}] START {}.{} run={run_id}", now(), run.oracle, run.job_id).map_err(log_error)?;
    let attempt = attempt(store, &run, today, hour, existing_token, &mut log).unwrap_or_else(|error| Attempt {
        code: 1, output_written: false, output_bytes: 0, deliverable: None,
        expected: run.outcome.expected_output.clone(), error: Some(error),
    });
    if let Some(error) = &attempt.error { writeln!(log, "[{}] ERROR {error}", now()).map_err(log_error)?; }
    let finished = store.finalize(run_id, FinishRequest { exited_at: now(), exit_code: attempt.code,
        output_file_written: attempt.output_written, output_bytes: attempt.output_bytes,
        deliverable_written: attempt.deliverable, expected_output: attempt.expected, error: attempt.error });
    match finished {
        Ok(run) => { writeln!(log, "[{}] END status={:?} exit={}", now(), run.outcome.status, attempt.code).map_err(log_error)?; Ok(run) }
        Err(error) => { writeln!(log, "[{}] FINALIZE_ERROR {error}", now()).map_err(log_error)?; Err(error) }
    }
}
#[rustfmt::skip]
fn attempt(store: &FireStore, run: &StoredRun, today: &str, hour: &str,
    existing_token: Option<&str>, log: &mut File) -> Result<Attempt, String> {
    let cwd = absolute_dir(&run.cwd, "working directory")?;
    let expected = expected_path(&cwd, run.outcome.expected_output.as_deref(), today, hour)?;
    let before = expected.as_deref().and_then(fingerprint);
    let (program, args, token, output) = match run.outcome.exec {
        ExecMode::Shell => (absolute_file(&run.bash_path, "bash")?, vec!["-c".to_owned(), run.command.clone()], None, None),
        ExecMode::ClaudeHeadless => {
            let claude = absolute_file(run.claude_path.as_deref().unwrap_or(""), "claude")?;
            let token = hydrate(existing_token, run.pass_path.as_deref().unwrap_or(""), &run.token_name)?;
            let output = run.output_path.as_deref().ok_or_else(|| "claude output path missing".to_owned())?;
            (claude, vec!["--dangerously-skip-permissions".into(), "-p".into(), run.command.clone()], Some(token), Some(PathBuf::from(output)))
        }
    };
    let (stdout, stderr) = if let Some(path) = &output {
        if !path.is_absolute() { return Err("claude output path must be absolute".to_owned()); }
        if let Some(parent) = path.parent() { std::fs::create_dir_all(parent).map_err(|e| format!("create output dir: {e}"))?; }
        let file = File::create(path).map_err(|e| format!("create output: {e}"))?;
        (Stdio::from(file.try_clone().map_err(|e| format!("clone output: {e}"))?), Stdio::from(file))
    } else {
        (Stdio::from(log.try_clone().map_err(|e| format!("clone log: {e}"))?), Stdio::from(log.try_clone().map_err(|e| format!("clone log: {e}"))?))
    };
    store.spawned(&run.outcome.run_id, now(), run.output_path.clone())?;
    let mut command = Command::new(program); command.args(args).current_dir(cwd).stdout(stdout).stderr(stderr);
    if let Some(token) = token { command.env("CLAUDE_CODE_OAUTH_TOKEN", token); }
    let status = command.status().map_err(|e| format!("spawn child: {e}"))?;
    let code = status.code().unwrap_or(1);
    let output_bytes = output.as_deref().and_then(|path| path.metadata().ok()).map_or(0, |meta| meta.len());
    let output_written = output_bytes > 0;
    let deliverable = expected.as_deref().map(|path| fingerprint(path).is_some_and(|after|
        after.0 > 0 && after.1 >= run.outcome.reserved_at && Some(after) != before));
    let error = if code != 0 { Some(format!("child exited {code}")) }
        else if run.outcome.exec == ExecMode::ClaudeHeadless && !output_written { Some("claude produced no output".to_owned()) }
        else if deliverable == Some(false) { Some("expected_output was not written".to_owned()) } else { None };
    Ok(Attempt { code, output_written, output_bytes, deliverable,
        expected: expected.map(|path| path.to_string_lossy().into_owned()), error })
}
#[rustfmt::skip]
fn hydrate(existing: Option<&str>, pass: &str, token_name: &str) -> Result<String, String> {
    if let Some(token) = existing.filter(|token| !token.trim().is_empty()) { return Ok(token.to_owned()); }
    if token_name.is_empty() || !token_name.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)) { return Err("invalid token name".to_owned()); }
    let pass = absolute_file(pass, "pass")?;
    let output = Command::new(pass).args(["show", &format!("claude/token-{token_name}")]).output()
        .map_err(|e| format!("pass spawn failed: {e}"))?;
    let token = String::from_utf8_lossy(&output.stdout).trim_end_matches(['\r', '\n']).to_owned();
    if !output.status.success() || token.trim().is_empty() { Err(format!("oauth token hydration failed for {token_name}")) } else { Ok(token) }
}
fn absolute_file(value: &str, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() && path.is_file() {
        Ok(path)
    } else {
        Err(format!("absolute {name} binary is missing"))
    }
}
fn absolute_dir(value: &str, name: &str) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() && path.is_dir() {
        path.canonicalize()
            .map_err(|e| format!("resolve {name}: {e}"))
    } else {
        Err(format!("absolute {name} is missing"))
    }
}
#[rustfmt::skip]
fn expected_path(root: &Path, template: Option<&str>, today: &str, hour: &str) -> Result<Option<PathBuf>, String> {
    let Some(template) = template else { return Ok(None); };
    let yesterday = previous_date(today)?;
    let expanded = template.replace("$TODAY", today).replace("$YESTERDAY", &yesterday).replace("$HOUR", hour);
    let relative = Path::new(&expanded);
    if relative.is_absolute() || relative.components().any(|part| matches!(part, Component::ParentDir | Component::RootDir | Component::Prefix(_))) { return Err("expected_output escapes repository".to_owned()); }
    let path = root.join(relative);
    let mut ancestor = path.parent().unwrap_or(root);
    while !ancestor.exists() { ancestor = ancestor.parent().ok_or_else(|| "expected_output has no existing ancestor".to_owned())?; }
    if !ancestor.canonicalize().map_err(|e| format!("resolve expected_output: {e}"))?.starts_with(root) { return Err("expected_output escapes repository".to_owned()); }
    Ok(Some(path))
}
#[rustfmt::skip]
fn previous_date(today: &str) -> Result<String, String> {
    let invalid = || format!("invalid scheduled local date {today}");
    if today.len() != 10 { return Err(invalid()); }
    let mut parts = today.split('-');
    let year = parts.next().and_then(|part| part.parse::<u32>().ok()).ok_or_else(&invalid)?;
    let month = parts.next().and_then(|part| part.parse::<u32>().ok()).ok_or_else(&invalid)?;
    let day = parts.next().and_then(|part| part.parse::<u32>().ok()).ok_or_else(&invalid)?;
    if parts.next().is_some() || !(1..=days_in_month(year, month).ok_or_else(&invalid)?).contains(&day) { return Err(invalid()); }
    let (year, month, day) = if day > 1 { (year, month, day - 1) } else if month > 1 {
        let month = month - 1; (year, month, days_in_month(year, month).ok_or_else(&invalid)?)
    } else { (year.checked_sub(1).ok_or_else(invalid)?, 12, 31) };
    Ok(format!("{year:04}-{month:02}-{day:02}"))
}
const fn days_in_month(year: u32, month: u32) -> Option<u32> {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => Some(31),
        4 | 6 | 9 | 11 => Some(30),
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            Some(29)
        }
        2 => Some(28),
        _ => None,
    }
}
fn fingerprint(path: &Path) -> Option<(u64, u64)> {
    let meta = path.metadata().ok()?;
    Some((
        meta.len(),
        meta.modified()
            .ok()?
            .duration_since(UNIX_EPOCH)
            .ok()?
            .as_secs(),
    ))
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |time| time.as_secs())
}
#[allow(clippy::needless_pass_by_value)]
fn log_error(error: std::io::Error) -> String {
    format!("write schedule log: {error}")
}
