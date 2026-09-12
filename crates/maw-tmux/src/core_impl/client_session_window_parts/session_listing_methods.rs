impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{
    #[must_use]
    pub const fn new(runner: R) -> Self {
        Self { runner }
    }

    /// List session names.
    ///
    /// # Errors
    ///
    /// Returns the injected runner error when tmux is unreachable (e.g. a stale/orphaned
    /// socket) — this is distinct from a reachable server with zero sessions, which returns
    /// `Ok(vec![])`. Callers must not collapse the two; see #860.
    pub fn list_session_names(&mut self) -> Result<Vec<String>, TmuxError> {
        self.runner
            .run(
                "list-sessions",
                &["-F".to_owned(), "#{session_name}".to_owned()],
            )
            .map(|raw| parse_session_names(&raw))
    }

    /// List all sessions/windows in a single tmux call.
    ///
    /// # Errors
    ///
    /// Returns the injected runner error when tmux is unreachable (e.g. a stale/orphaned
    /// socket) — this is distinct from a reachable server with zero sessions, which returns
    /// `Ok(vec![])`. Callers must not collapse the two; see #860.
    pub fn list_all(&mut self) -> Result<Vec<TmuxSession>, TmuxError> {
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
    }

    /// List topology for a creation-capable command's first probe.
    ///
    /// Unlike observer listings, this maps only a runner-proven cold start to
    /// empty. Call it once, before mutation; subsequent reads must use
    /// [`Self::list_all`] so server loss stays fatal (#860, #940).
    ///
    /// # Errors
    ///
    /// Returns every error that is not proven to be an initial cold start.
    pub fn list_all_for_initial_creation(&mut self) -> Result<Vec<TmuxSession>, TmuxError> {
        match self.list_all() {
            Err(error) if self.runner.is_initial_cold_start(&error) => Ok(Vec::new()),
            result => result,
        }
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

    /// Get all pane IDs.
    ///
    /// # Errors
    ///
    /// Returns the injected runner error when tmux is unreachable (e.g. a stale/orphaned
    /// socket) — this is distinct from a reachable server with zero panes, which returns
    /// `Ok(BTreeSet::new())`. Callers must not collapse the two; see #860.
    pub fn list_pane_ids(&mut self) -> Result<BTreeSet<String>, TmuxError> {
        self.runner
            .run(
                "list-panes",
                &["-a".to_owned(), "-F".to_owned(), "#{pane_id}".to_owned()],
            )
            .map(|raw| parse_pane_ids(&raw))
    }

    /// Get structured pane information.
    ///
    /// # Errors
    ///
    /// Returns the injected runner error when tmux is unreachable (e.g. a stale/orphaned
    /// socket) — this is distinct from a reachable server with zero panes, which returns
    /// `Ok(vec![])`. Callers must not collapse the two; see #860.
    pub fn list_panes(&mut self) -> Result<Vec<TmuxPane>, TmuxError> {
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
}
