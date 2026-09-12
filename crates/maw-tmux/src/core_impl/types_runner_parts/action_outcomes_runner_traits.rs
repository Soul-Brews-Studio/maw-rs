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

    /// Whether tmux reports that its server/socket has not been created yet.
    ///
    /// This classifies stderr only. Callers must additionally prove this is an
    /// initial, outside-tmux probe: an unlinked live socket has the same ENOENT
    /// text and must remain fatal (#860).
    #[must_use]
    pub fn is_cold_start(&self) -> bool {
        self.cold_start_socket().is_some()
    }

    #[must_use]
    pub(crate) fn cold_start_socket(&self) -> Option<&str> {
        let message = self.message.as_str();
        let detail = message.strip_prefix("tmux exited with status 1: ")?;
        detail
            .strip_prefix("no server running on ")
            .or_else(|| {
                detail
                    .strip_prefix("error connecting to ")?
                    .strip_suffix(" (No such file or directory)")
            })
            .filter(|socket| !socket.trim().is_empty() && !socket.chars().any(char::is_control))
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

    /// Whether `error` proves an initial cold server for this runner.
    ///
    /// The default is fail-closed because injected runners have no reliable
    /// caller/server history. The process runner overrides this with both.
    fn is_initial_cold_start(&self, _error: &TmuxError) -> bool {
        false
    }

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

impl<R: TmuxRunner + ?Sized> TmuxRunner for &mut R {
    fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError> {
        (**self).run(subcommand, args)
    }

    fn is_initial_cold_start(&self, error: &TmuxError) -> bool {
        (**self).is_initial_cold_start(error)
    }

    fn run_with_stdin(
        &mut self,
        subcommand: &str,
        args: &[String],
        stdin: &[u8],
    ) -> Result<String, TmuxError> {
        (**self).run_with_stdin(subcommand, args, stdin)
    }
}
