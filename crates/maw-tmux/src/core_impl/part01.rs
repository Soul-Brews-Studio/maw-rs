// Testable tmux command and parser adapter for maw-rs.
//
// This crate ports the deterministic parts of maw-js `src/core/transport/tmux-class.ts`:
// shell-safe command construction plus parsing of `list-windows` / `list-panes` output.
// Real process execution is intentionally injected through [`TmuxRunner`].

use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    ffi::OsString,
    fmt,
    io::Write,
    process::{Command, Stdio},
};

use maw_matcher::{resolve_by_name, Named, ResolveOptions, ResolveResult};

const DEFAULT_CAPTURE_LINES: u32 = 80;
const DEFAULT_PTY_COLS_LIMIT: u32 = 500;
const DEFAULT_PTY_ROWS_LIMIT: u32 = 200;
pub const SEND_SETTLE_MS: u64 = 1_500;
pub const SUBMIT_CONFIRM_MS: u64 = 700;
pub const SUBMIT_GRACE_MS: u64 = 300;
pub const MAX_SUBMIT_ATTEMPTS: u32 = 4;
const COOLDOWN_MS: u64 = 500;
const QUOTA_PER_MINUTE: u32 = 100;
const QUOTA_WINDOW_MS: u64 = 60_000;

const VALID_LAYOUTS: [&str; 5] = [
    "even-horizontal",
    "even-vertical",
    "main-horizontal",
    "main-vertical",
    "tiled",
];

/// Tmux format used by maw-js pane target fallback resolution.
pub const PANE_TARGET_FORMAT: &str =
    "#{pane_id}|||#{session_name}:#{window_index}.#{pane_index}|||#{pane_title}|||#{@maw_tile_role}|||#{pane_current_path}";

/// Tmux window metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxWindow {
    pub index: u32,
    pub name: String,
    pub active: bool,
    pub cwd: Option<String>,
}

/// Tmux session metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxSession {
    pub name: String,
    pub windows: Vec<TmuxWindow>,
}

/// Tmux pane metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxPane {
    pub id: String,
    pub command: String,
    pub target: String,
    pub title: String,
    pub pid: Option<u32>,
    pub cwd: Option<String>,
    pub last_activity: Option<u64>,
}

/// Options for creating a tmux session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSessionOptions {
    pub window: Option<String>,
    pub cwd: Option<String>,
    pub detached: bool,
    pub command: Option<String>,
    pub print_format: Option<String>,
}

impl Default for NewSessionOptions {
    fn default() -> Self {
        Self {
            window: None,
            cwd: None,
            detached: true,
            command: None,
            print_format: None,
        }
    }
}

/// Options for creating a grouped tmux session.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupedSessionOptions {
    pub cols: Option<u32>,
    pub rows: Option<u32>,
    pub window: Option<String>,
    pub window_size: Option<String>,
}

/// Options for creating a tmux pane split.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SplitWindowOptions {
    pub cwd: Option<String>,
    pub command: Option<String>,
    pub print_format: Option<String>,
}

/// Options for selecting a tmux pane.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectPaneOptions {
    pub title: Option<String>,
}

/// Outcome from maw-js-style smart text submission.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SendTextReport {
    pub used_buffer: bool,
    pub enter_attempts: u32,
    pub warned_pending: bool,
}

/// Options for lock-protected `split-window` construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SplitWindowLockedOptions {
    pub vertical: Option<bool>,
    pub pct: Option<u32>,
    pub shell_command: Option<String>,
}

/// Pane tags: title plus tmux `@custom` options.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneTags {
    pub title: String,
    pub meta: BTreeMap<String, String>,
}

/// Minimal pane shape used by `maw tmux ls` annotation logic.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxLsPaneRef {
    pub id: String,
    pub target: String,
    pub command: Option<String>,
}

/// Result of tmux send destructive-command safety scanning.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DestructiveCheck {
    pub destructive: bool,
    pub reasons: Vec<String>,
}

/// Options for Rust's maw-js-compatible `maw tmux send` action wrapper.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxSendCommandOptions {
    pub literal: bool,
    pub allow_destructive: bool,
    pub force: bool,
}

/// Options for Rust's maw-js-compatible `maw tmux split` action wrapper.
#[derive(Debug, Clone, PartialEq)]
pub struct TmuxSplitActionOptions {
    pub vertical: bool,
    pub pct: f64,
    pub command: Option<String>,
}

impl Default for TmuxSplitActionOptions {
    fn default() -> Self {
        Self {
            vertical: false,
            pct: 50.0,
            command: None,
        }
    }
}

/// Per-pane heartbeat throttle state for `maw tmux send`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SendTrackerEntry {
    pub last_ts: u64,
    pub count: u32,
    pub window_start: u64,
}

/// Send throttle outcome before tmux mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendThrottle {
    Allowed,
    Cooldown { cooldown_ms: u64 },
    Quota { quota_per_minute: u32 },
}

/// In-memory cooldown + quota tracker ported from maw-js `_sendTracker`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxSendTracker {
    entries: BTreeMap<String, SendTrackerEntry>,
}

impl TmuxSendTracker {
    /// Return the current entry for tests/diagnostics.
    #[must_use]
    pub fn get(&self, resolved: &str) -> Option<SendTrackerEntry> {
        self.entries.get(resolved).copied()
    }

    /// Insert or replace a tracker entry for tests/recovery.
    pub fn set(&mut self, resolved: impl Into<String>, entry: SendTrackerEntry) {
        self.entries.insert(resolved.into(), entry);
    }

    /// Clear all tracker entries.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Apply maw-js heartbeat cooldown and quota gates.
    ///
    /// `force` bypasses the tracker and does not mutate it, matching the JavaScript action.
    pub fn check(&mut self, resolved: &str, now_ms: u64, force: bool) -> SendThrottle {
        if force {
            return SendThrottle::Allowed;
        }
        let Some(prev) = self.entries.get_mut(resolved) else {
            self.entries.insert(
                resolved.to_owned(),
                SendTrackerEntry {
                    last_ts: now_ms,
                    count: 1,
                    window_start: now_ms,
                },
            );
            return SendThrottle::Allowed;
        };
        if now_ms.saturating_sub(prev.last_ts) < COOLDOWN_MS {
            return SendThrottle::Cooldown {
                cooldown_ms: COOLDOWN_MS,
            };
        }
        if now_ms.saturating_sub(prev.window_start) > QUOTA_WINDOW_MS {
            prev.count = 0;
            prev.window_start = now_ms;
        }
        if prev.count >= QUOTA_PER_MINUTE {
            return SendThrottle::Quota {
                quota_per_minute: QUOTA_PER_MINUTE,
            };
        }
        prev.last_ts = now_ms;
        prev.count += 1;
        SendThrottle::Allowed
    }
}

/// Outcome from a high-level `maw tmux send` action attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxSendCommandOutcome {
    Sent,
    Throttled(SendThrottle),
}

/// Execution action selected by maw-js `maw tmux attach`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxAttachAction {
    Print { session: String },
    SwitchClient { session: String },
    Attach { session: String },
    Recover { session: String },
}

/// Session-name resolution selected before a high-level attach action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxAttachSessionResolution {
    Match { session: String },
    Ambiguous { query: String, candidates: Vec<String> },
    Missing { session: String },
}

/// Spawn command selected by `cmdTmuxAttach` or its recovery path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnCommand {
    pub program: String,
    pub args: Vec<String>,
}

/// Candidate shown by maw-js attach recovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachRecoveryCandidate {
    pub oracle: String,
    pub label: String,
}

/// Fleet entry fragment used to seed attach recovery candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttachRecoveryFleetEntry {
    pub session: String,
    pub first_window_name: Option<String>,
    pub repo: Option<String>,
}

/// Pure attach recovery decision after candidate construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachRecoveryDecision {
    NoCandidates,
    AutoWake {
        command: SpawnCommand,
        label: String,
    },
    PrintCandidates {
        candidates: Vec<AttachRecoveryCandidate>,
    },
    Prompt {
        candidates: Vec<AttachRecoveryCandidate>,
    },
    WakeChoice {
        command: SpawnCommand,
    },
    InvalidChoice,
}

/// Options for Rust's maw-js-compatible `maw tmux kill` action wrapper.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TmuxKillCommandOptions {
    pub force: bool,
    pub session: bool,
}

/// Target plus source metadata after kill fallback resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxKillTarget {
    pub resolved: String,
    pub source: String,
}

/// Successful tmux kill operation kind and concrete target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxKillOutcome {
    Pane { target: String },
    Session { session: String },
}

/// Candidate name that can resolve to a live tmux pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaneTargetCandidate {
    pub name: String,
    pub resolved: String,
    pub source: String,
    pub target: String,
}

impl Named for PaneTargetCandidate {
    fn name(&self) -> &str {
        &self.name
    }
}

/// Resolution result for orphan pane kill fallback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneTargetResolution {
    None,
    Match {
        candidate: PaneTargetCandidate,
    },
    Ambiguous {
        candidates: Vec<PaneTargetCandidate>,
    },
}

/// Error returned by an injected tmux runner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TmuxError {
    pub message: String,
}

impl TmuxError {
    #[must_use]
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TmuxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl Error for TmuxError {}

/// Injectable tmux execution seam.
pub trait TmuxRunner {
    /// Run `tmux <subcommand> <args...>` and return stdout.
    ///
    /// # Errors
    ///
    /// Returns [`TmuxError`] when tmux exits non-zero or the host command cannot be executed.
    fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError>;

    /// Run `tmux <subcommand> <args...>` with stdin.
    ///
    /// # Errors
    ///
    /// Returns [`TmuxError`] when the runner does not support stdin or tmux execution fails.
    fn run_with_stdin(
        &mut self,
        subcommand: &str,
        args: &[String],
        _stdin: &[u8],
    ) -> Result<String, TmuxError> {
        self.run(subcommand, args)
    }
}

/// Concrete tmux runner backed by `std::process::Command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTmuxRunner {
    program: OsString,
    socket: Option<OsString>,
}

impl Default for CommandTmuxRunner {
    fn default() -> Self {
        Self {
            program: OsString::from("tmux"),
            socket: None,
        }
    }
}

impl CommandTmuxRunner {
    /// Create a runner that invokes the default `tmux` binary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a runner that invokes a custom tmux-compatible program.
    #[must_use]
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            socket: None,
        }
    }

    /// Set the tmux socket passed as `-S <socket>`.
    #[must_use]
    pub fn with_socket(mut self, socket: impl Into<OsString>) -> Self {
        self.socket = Some(socket.into());
        self
    }

    /// Return the exact argv vector this runner will execute.
    ///
    /// This keeps runtime command construction testable without requiring a live tmux server.
    #[must_use]
    pub fn argv(&self, subcommand: &str, tmux_args: &[String]) -> Vec<OsString> {
        let mut command_line = vec![self.program.clone()];
        if let Some(socket) = &self.socket {
            command_line.push(OsString::from("-S"));
            command_line.push(socket.clone());
        }
        command_line.push(OsString::from(subcommand));
        command_line.extend(tmux_args.iter().map(OsString::from));
        command_line
    }
}

impl TmuxRunner for CommandTmuxRunner {
    fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError> {
        self.run_command(subcommand, args, None)
    }

    fn run_with_stdin(
        &mut self,
        subcommand: &str,
        args: &[String],
        stdin: &[u8],
    ) -> Result<String, TmuxError> {
        self.run_command(subcommand, args, Some(stdin))
    }
}

impl CommandTmuxRunner {
    fn run_command(
        &self,
        subcommand: &str,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<String, TmuxError> {
        let command_line = self.argv(subcommand, args);
        let (program, rest) = command_line
            .split_first()
            .expect("tmux command line always includes a program");
        validate_tmux_program(program)?;
        validate_tmux_option_values(rest)?;
        let mut command = Command::new(program);
        command.args(rest);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command.spawn().map_err(|error| {
            TmuxError::new(format!(
                "failed to execute {}: {error}",
                program.to_string_lossy()
            ))
        })?;
        if let Some(stdin) = stdin {
            let mut child_stdin = child
                .stdin
                .take()
                .ok_or_else(|| TmuxError::new("failed to open tmux stdin"))?;
            child_stdin
                .write_all(stdin)
                .map_err(|error| tmux_program_io_error("write stdin for", program, &error))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| tmux_program_io_error("collect output from", program, &error))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        let code = output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        if detail.is_empty() {
            Err(TmuxError::new(format!("tmux exited with status {code}")))
        } else {
            Err(TmuxError::new(format!(
                "tmux exited with status {code}: {detail}"
            )))
        }
    }
}

fn validate_tmux_program(program: &std::ffi::OsStr) -> Result<(), TmuxError> {
    let display = program.to_string_lossy();
    if display.is_empty() || display.trim() != display || display.starts_with('-') {
        Err(TmuxError::new(
            "tmux program must be non-empty, unpadded, and not start with '-'",
        ))
    } else {
        Ok(())
    }
}

fn validate_tmux_option_values(args: &[OsString]) -> Result<(), TmuxError> {
    let mut previous_wants_target = false;
    for arg in args {
        let value = arg.to_string_lossy();
        if previous_wants_target {
            if value.is_empty() || value.trim() != value || value.starts_with('-') {
                return Err(TmuxError::new(
                    "tmux target/session must be non-empty, unpadded, and not start with '-'",
                ));
            }
            previous_wants_target = false;
            continue;
        }
        previous_wants_target = matches!(value.as_ref(), "-t" | "-s");
    }
    if previous_wants_target {
        return Err(TmuxError::new("tmux target/session option missing value"));
    }
    Ok(())
}

fn tmux_program_io_error(
    action: &str,
    program: &std::ffi::OsStr,
    error: &std::io::Error,
) -> TmuxError {
    TmuxError::new(format!(
        "failed to {action} {}: {error}",
        program.to_string_lossy()
    ))
}
