impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{

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

    /// Create a tmux window with its initial pane running `command`.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn new_window_with_command(
        &mut self,
        session: &str,
        name: &str,
        command: &str,
    ) -> Result<(), TmuxError> {
        let args = vec![
            "-t".to_owned(),
            format!("{session}:"),
            "-n".to_owned(),
            name.to_owned(),
            command.to_owned(),
        ];
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
}
