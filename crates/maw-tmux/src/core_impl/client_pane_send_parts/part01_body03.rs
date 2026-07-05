
    /// Capture recent pane contents using `tmux capture-pane`.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux cannot capture the target.
    pub fn capture(&mut self, target: &str, lines: Option<u32>) -> Result<String, TmuxError> {
        let lines = lines.unwrap_or(DEFAULT_CAPTURE_LINES);
        self.runner.run(
            "capture-pane",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-e".to_owned(),
                "-p".to_owned(),
                "-S".to_owned(),
                format!("-{lines}"),
            ],
        )
    }

    /// Resize a pane best-effort, clamping to maw-js default pty limits.
    pub fn resize_pane(&mut self, target: &str, cols: u32, rows: u32) {
        let cols = clamp_pty(cols, DEFAULT_PTY_COLS_LIMIT);
        let rows = clamp_pty(rows, DEFAULT_PTY_ROWS_LIMIT);
        self.try_run(
            "resize-pane",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-x".to_owned(),
                cols.to_string(),
                "-y".to_owned(),
                rows.to_string(),
            ],
        );
    }

    /// Resize a window best-effort, clamping to maw-js default pty limits.
    pub fn resize_window(&mut self, target: &str, cols: u32, rows: u32) {
        let cols = clamp_pty(cols, DEFAULT_PTY_COLS_LIMIT);
        let rows = clamp_pty(rows, DEFAULT_PTY_ROWS_LIMIT);
        self.try_run(
            "resize-window",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-x".to_owned(),
                cols.to_string(),
                "-y".to_owned(),
                rows.to_string(),
            ],
        );
    }
