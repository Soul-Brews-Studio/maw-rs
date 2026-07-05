/// Testable tmux client that delegates all execution to [`TmuxRunner`].
pub struct TmuxClient<R> {
    runner: R,
}

impl TmuxClient<CommandTmuxRunner> {
    /// Create a client backed by the local `tmux` binary.
    #[must_use]
    pub fn local() -> Self {
        Self::new(CommandTmuxRunner::new())
    }

    /// Create a client backed by the local `tmux` binary on a specific socket.
    #[must_use]
    pub fn local_with_socket(socket: impl Into<OsString>) -> Self {
        Self::new(CommandTmuxRunner::new().with_socket(socket))
    }
}

