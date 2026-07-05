
    /// Run the high-level maw-js `maw tmux split` mutation against an already-resolved pane.
    ///
    /// # Errors
    ///
    /// Returns validation or runner errors.
    pub fn split_pane_action(
        &mut self,
        resolved: &str,
        options: &TmuxSplitActionOptions,
    ) -> Result<(), TmuxError> {
        self.runner
            .run("split-window", &tmux_split_action_args(resolved, options)?)
            .map(|_| ())
    }

    /// Run maw-js `cmdTmuxSplit` against a resolved target with command-style error wrapping.
    ///
    /// # Errors
    ///
    /// Returns pct validation or wrapped runner errors.
    pub fn split_target_action(
        &mut self,
        target: &TmuxKillTarget,
        options: &TmuxSplitActionOptions,
    ) -> Result<(), TmuxError> {
        self.runner
            .run(
                "split-window",
                &tmux_split_action_args(&target.resolved, options)?,
            )
            .map(|_| ())
            .map_err(|error| {
                TmuxError::new(format!(
                    "split-window failed for '{}' (from {}): {}",
                    target.resolved, target.source, error.message
                ))
            })
    }

    /// Select a pane, optionally setting its title.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn select_pane(
        &mut self,
        target: &str,
        options: &SelectPaneOptions,
    ) -> Result<(), TmuxError> {
        let mut args = vec!["-t".to_owned(), target.to_owned()];
        if let Some(title) = &options.title {
            args.extend(["-T".to_owned(), title.clone()]);
        }
        self.runner.run("select-pane", &args).map(|_| ())
    }

    /// Set pane title and/or tmux `@custom` metadata.
    ///
    /// # Errors
    ///
    /// Returns the first runner error from title or metadata writes.
    pub fn tag_pane(
        &mut self,
        target: &str,
        title: Option<&str>,
        meta: &[(String, String)],
    ) -> Result<(), TmuxError> {
        write_pane_title(&mut self.runner, target, title)?;
        for (raw_key, value) in meta {
            let key = normalize_pane_tag_key(raw_key);
            self.runner.run(
                "set-option",
                &[
                    "-p".to_owned(),
                    "-t".to_owned(),
                    target.to_owned(),
                    key,
                    value.clone(),
                ],
            )?;
        }
        Ok(())
    }

    /// Read pane title and tmux `@custom` metadata.
    ///
    /// # Errors
    ///
    /// Returns the runner error when the title probe fails. Metadata probe is best-effort.
    pub fn read_pane_tags(&mut self, target: &str) -> Result<PaneTags, TmuxError> {
        let title = self
            .runner
            .run(
                "display-message",
                &[
                    "-p".to_owned(),
                    "-t".to_owned(),
                    target.to_owned(),
                    "#{pane_title}".to_owned(),
                ],
            )?
            .trim()
            .to_owned();
        let raw = self.try_run(
            "show-options",
            &["-p".to_owned(), "-t".to_owned(), target.to_owned()],
        );
        Ok(PaneTags {
            title,
            meta: parse_pane_tag_options(&raw),
        })
    }

    /// Select a tmux layout.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn select_layout(&mut self, target: &str, layout: &str) -> Result<(), TmuxError> {
        self.runner
            .run(
                "select-layout",
                &["-t".to_owned(), target.to_owned(), layout.to_owned()],
            )
            .map(|_| ())
    }

    /// Apply a maw-js `maw tmux layout` preset after validating the allowed set.
    ///
    /// # Errors
    ///
    /// Returns validation or runner errors.
    pub fn select_valid_layout(&mut self, resolved: &str, preset: &str) -> Result<(), TmuxError> {
        validate_layout_preset(preset)?;
        let window = tmux_window_target(resolved);
        self.select_layout(&window, preset)
    }

    /// Run maw-js `cmdTmuxLayout` against a resolved target with command-style error wrapping.
    ///
    /// # Errors
    ///
    /// Returns preset validation or wrapped runner errors.
    pub fn select_layout_action(
        &mut self,
        target: &TmuxKillTarget,
        preset: &str,
    ) -> Result<(), TmuxError> {
        validate_layout_preset(preset)?;
        let window = tmux_window_target(&target.resolved);
        self.runner
            .run(
                "select-layout",
                &["-t".to_owned(), window.clone(), preset.to_owned()],
            )
            .map(|_| ())
            .map_err(|error| {
                TmuxError::new(format!(
                    "select-layout failed for '{}' (from {}): {}",
                    window, target.source, error.message
                ))
            })
    }

    /// Send tmux keys to a target.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn send_keys(&mut self, target: &str, keys: &[String]) -> Result<(), TmuxError> {
        let mut args = vec!["-t".to_owned(), target.to_owned()];
        args.extend(keys.iter().cloned());
        self.runner.run("send-keys", &args).map(|_| ())
    }
