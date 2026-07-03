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

impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{
    #[must_use]
    pub const fn new(runner: R) -> Self {
        Self { runner }
    }

    /// List session names; tmux-unavailable errors are fail-soft and return an empty list.
    pub fn list_session_names(&mut self) -> Vec<String> {
        self.runner
            .run(
                "list-sessions",
                &["-F".to_owned(), "#{session_name}".to_owned()],
            )
            .map(|raw| parse_session_names(&raw))
            .unwrap_or_default()
    }

    /// List all sessions/windows in a single tmux call; tmux-unavailable errors return empty.
    pub fn list_all(&mut self) -> Vec<TmuxSession> {
        self.runner
            .run(
                "list-windows",
                &[
                    "-a".to_owned(),
                    "-F".to_owned(),
                    "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}".to_owned(),
                ],
            )
            .map(|raw| parse_list_all_windows(&raw))
            .unwrap_or_default()
    }

    /// List one session's windows.
    ///
    /// # Errors
    ///
    /// Returns the injected runner error when tmux rejects the session target.
    pub fn list_windows(&mut self, session: &str) -> Result<Vec<TmuxWindow>, TmuxError> {
        let raw = self.runner.run(
            "list-windows",
            &[
                "-t".to_owned(),
                session.to_owned(),
                "-F".to_owned(),
                "#{window_index}:#{window_name}:#{window_active}".to_owned(),
            ],
        )?;
        Ok(parse_list_windows(&raw))
    }

    /// Get all pane IDs; tmux-unavailable errors return empty.
    pub fn list_pane_ids(&mut self) -> BTreeSet<String> {
        self.runner
            .run(
                "list-panes",
                &["-a".to_owned(), "-F".to_owned(), "#{pane_id}".to_owned()],
            )
            .map(|raw| parse_pane_ids(&raw))
            .unwrap_or_default()
    }

    /// Get structured pane information; tmux-unavailable errors return empty.
    pub fn list_panes(&mut self) -> Vec<TmuxPane> {
        self.runner
            .run(
                "list-panes",
                &[
                    "-a".to_owned(),
                    "-F".to_owned(),
                    "#{pane_id}|||#{pane_current_command}|||#{session_name}:#{window_name}.#{pane_index}|||#{pane_title}|||#{pane_pid}|||#{pane_current_path}|||#{window_activity}".to_owned(),
                ],
            )
            .map(|raw| parse_list_panes(&raw))
            .unwrap_or_default()
    }

    /// Check whether a tmux session exists.
    pub fn has_session(&mut self, name: &str) -> bool {
        self.runner
            .run("has-session", &["-t".to_owned(), name.to_owned()])
            .is_ok()
    }

    /// Create a tmux session, then enable window renumbering like maw-js.
    ///
    /// # Errors
    ///
    /// Returns the runner error when `new-session` fails. `set-option` remains best-effort.
    pub fn new_session(
        &mut self,
        name: &str,
        options: &NewSessionOptions,
    ) -> Result<String, TmuxError> {
        let mut args = Vec::new();
        if options.detached {
            args.push("-d".to_owned());
        }
        if let Some(print_format) = &options.print_format {
            args.extend(["-P".to_owned(), "-F".to_owned(), print_format.clone()]);
        }
        args.extend(["-s".to_owned(), name.to_owned()]);
        if let Some(window) = &options.window {
            args.extend(["-n".to_owned(), window.clone()]);
        }
        if let Some(cwd) = &options.cwd {
            args.extend(["-c".to_owned(), cwd.clone()]);
        }
        if let Some(command) = &options.command {
            args.push(command.clone());
        }
        let out = self.runner.run("new-session", &args)?;
        self.set_option(name, "renumber-windows", "on");
        Ok(out)
    }

    /// Return the first pane ID for a target; errors return `None`.
    pub fn first_pane_id(&mut self, target: &str) -> Option<String> {
        self.runner
            .run(
                "list-panes",
                &[
                    "-t".to_owned(),
                    target.to_owned(),
                    "-F".to_owned(),
                    "#{pane_id}".to_owned(),
                ],
            )
            .ok()
            .and_then(|raw| {
                raw.lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(str::to_owned)
            })
    }

    /// Create a grouped session sharing windows with `parent`.
    ///
    /// # Errors
    ///
    /// Returns the runner error when the `new-session -t` call fails.
    pub fn new_grouped_session(
        &mut self,
        parent: &str,
        name: &str,
        options: &GroupedSessionOptions,
    ) -> Result<(), TmuxError> {
        let mut args = vec![
            "-d".to_owned(),
            "-t".to_owned(),
            parent.to_owned(),
            "-s".to_owned(),
            name.to_owned(),
        ];
        if let Some(cols) = options.cols {
            args.extend(["-x".to_owned(), cols.to_string()]);
        }
        if let Some(rows) = options.rows {
            args.extend(["-y".to_owned(), rows.to_string()]);
        }
        self.runner.run("new-session", &args)?;
        if let Some(window_size) = &options.window_size {
            self.set_option(name, "window-size", window_size);
        }
        if let Some(window) = &options.window {
            self.select_window(&format!("{name}:{window}"));
        }
        Ok(())
    }

    /// Kill a tmux session best-effort.
    pub fn kill_session(&mut self, name: &str) {
        self.try_run("kill-session", &["-t".to_owned(), name.to_owned()]);
    }

    /// Create a tmux window.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn new_window(
        &mut self,
        session: &str,
        name: &str,
        cwd: Option<&str>,
    ) -> Result<(), TmuxError> {
        let mut args = vec![
            "-t".to_owned(),
            format!("{session}:"),
            "-n".to_owned(),
            name.to_owned(),
        ];
        if let Some(cwd) = cwd {
            args.extend(["-c".to_owned(), cwd.to_owned()]);
        }
        self.runner.run("new-window", &args).map(|_| ())
    }

    /// Select a tmux window best-effort.
    pub fn select_window(&mut self, target: &str) {
        self.try_run("select-window", &["-t".to_owned(), target.to_owned()]);
    }

    /// Switch the current tmux client best-effort.
    pub fn switch_client(&mut self, session: &str) {
        self.try_run("switch-client", &["-t".to_owned(), session.to_owned()]);
    }

    /// Kill a tmux window best-effort.
    pub fn kill_window(&mut self, target: &str) {
        self.try_run("kill-window", &["-t".to_owned(), target.to_owned()]);
    }

    /// Kill a tmux pane best-effort.
    pub fn kill_pane(&mut self, target: &str) {
        self.try_run("kill-pane", &["-t".to_owned(), target.to_owned()]);
    }

    /// Run maw-js `cmdTmuxKill` against an already-resolved/fallback-adjusted target.
    ///
    /// # Errors
    ///
    /// Returns safety refusal or runner errors.
    pub fn kill_target_action(
        &mut self,
        target: &TmuxKillTarget,
        fleet_sessions: &BTreeSet<String>,
        options: &TmuxKillCommandOptions,
    ) -> Result<TmuxKillOutcome, TmuxError> {
        let session = tmux_session_from_target(&target.resolved);
        if is_fleet_or_view_session(&session, fleet_sessions) && !options.force {
            return Err(TmuxError::new(format!(
                "refusing to kill: session '{session}' is fleet or view.\n  killing would terminate a live oracle (or its mirror).\n  pass --force to override (you really want to kill a fleet session)"
            )));
        }

        if options.session {
            self.runner
                .run("kill-session", &["-t".to_owned(), session.clone()])
                .map_err(|error| {
                    TmuxError::new(format!(
                        "kill failed for '{}' (from {}): {}",
                        target.resolved, target.source, error.message
                    ))
                })?;
            Ok(TmuxKillOutcome::Session { session })
        } else {
            self.runner
                .run("kill-pane", &["-t".to_owned(), target.resolved.clone()])
                .map_err(|error| {
                    TmuxError::new(format!(
                        "kill failed for '{}' (from {}): {}",
                        target.resolved, target.source, error.message
                    ))
                })?;
            Ok(TmuxKillOutcome::Pane {
                target: target.resolved.clone(),
            })
        }
    }

    /// Return the command running in a pane.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux cannot inspect the target.
    pub fn get_pane_command(&mut self, target: &str) -> Result<String, TmuxError> {
        let raw = self.runner.run(
            "list-panes",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-F".to_owned(),
                "#{pane_current_command}".to_owned(),
            ],
        )?;
        Ok(raw.lines().next().unwrap_or_default().to_owned())
    }

    /// Return the current command for a pane through tmux `display-message`.
    ///
    /// This matches the safety lookup used by maw-js `cmdTmuxSend`.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux cannot inspect the target.
    pub fn display_pane_current_command(&mut self, target: &str) -> Result<String, TmuxError> {
        self.runner
            .run(
                "display-message",
                &[
                    "-p".to_owned(),
                    "-t".to_owned(),
                    target.to_owned(),
                    "#{pane_current_command}".to_owned(),
                ],
            )
            .map(|raw| raw.trim().to_owned())
    }

    /// Return command and cwd for a pane.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux cannot inspect the target.
    pub fn get_pane_info(&mut self, target: &str) -> Result<(String, String), TmuxError> {
        let raw = self.runner.run(
            "list-panes",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-F".to_owned(),
                "#{pane_current_command}\t#{pane_current_path}".to_owned(),
            ],
        )?;
        let first = raw.lines().next().unwrap_or_default();
        let (command, cwd) = first.split_once('\t').unwrap_or((first, ""));
        Ok((command.to_owned(), cwd.to_owned()))
    }

    /// Create a tmux pane split.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn split_window(
        &mut self,
        target: Option<&str>,
        options: &SplitWindowOptions,
    ) -> Result<String, TmuxError> {
        let mut args = Vec::new();
        if let Some(print_format) = &options.print_format {
            args.extend(["-P".to_owned(), "-F".to_owned(), print_format.clone()]);
        }
        if let Some(target) = target {
            args.extend(["-t".to_owned(), target.to_owned()]);
        }
        if let Some(cwd) = &options.cwd {
            args.extend(["-c".to_owned(), cwd.clone()]);
        }
        if let Some(command) = &options.command {
            args.push(command.clone());
        }
        self.runner.run("split-window", &args)
    }

    /// Build and run the tmux args used by maw-js `splitWindowLocked`.
    ///
    /// This method does not sleep; callers that need cross-call settling own scheduling/locking.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the split.
    pub fn split_window_locked(
        &mut self,
        target: &str,
        options: &SplitWindowLockedOptions,
    ) -> Result<(), TmuxError> {
        let mut args = vec!["-t".to_owned(), target.to_owned()];
        match options.vertical {
            Some(true) => args.push("-v".to_owned()),
            Some(false) => args.push("-h".to_owned()),
            None => {}
        }
        if let Some(pct) = options.pct {
            args.extend(["-l".to_owned(), format!("{pct}%")]);
        }
        if let Some(shell_command) = &options.shell_command {
            args.push(shell_command.clone());
        }
        self.runner.run("split-window", &args).map(|_| ())
    }
}

