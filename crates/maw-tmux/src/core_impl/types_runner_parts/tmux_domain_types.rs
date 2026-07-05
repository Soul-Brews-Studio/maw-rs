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

