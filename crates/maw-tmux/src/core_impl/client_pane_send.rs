impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{

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

    /// Send literal text through `tmux send-keys -l`.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn send_keys_literal(&mut self, target: &str, text: &str) -> Result<(), TmuxError> {
        self.runner
            .run("send-keys", &tmux_send_keys_literal_args(target, text))
            .map(|_| ())
    }

    /// Send one Enter key through `tmux send-keys`.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn send_enter(&mut self, target: &str) -> Result<(), TmuxError> {
        self.runner
            .run("send-keys", &tmux_send_enter_args(target))
            .map(|_| ())
    }

    /// Run the high-level maw-js `maw tmux send` mutation against an already-resolved pane.
    ///
    /// The caller owns target resolution and user-facing output; this method ports the action
    /// gates and exact `send-keys` argument shape.
    ///
    /// # Errors
    ///
    /// Returns validation, safety, lookup, or runner errors.
    pub fn send_command_to_pane(
        &mut self,
        tracker: &mut TmuxSendTracker,
        resolved: &str,
        command: &str,
        options: &TmuxSendCommandOptions,
        now_ms: u64,
    ) -> Result<TmuxSendCommandOutcome, TmuxError> {
        if command.is_empty() {
            return Err(TmuxError::new(
                "usage: maw tmux send <target> <command> [--literal] [--allow-destructive] [--force]",
            ));
        }
        match tracker.check(resolved, now_ms, options.force) {
            SendThrottle::Allowed => {}
            throttle => return Ok(TmuxSendCommandOutcome::Throttled(throttle)),
        }

        let destructive = check_destructive(command);
        if destructive.destructive && !options.allow_destructive {
            return Err(TmuxError::new(format!(
                "refusing to send: command matches destructive patterns:\n{}\n  pass --allow-destructive to bypass (review carefully first)",
                destructive
                    .reasons
                    .iter()
                    .map(|reason| format!("  - {reason}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )));
        }

        let pane_current_command = self.display_pane_current_command(resolved)?;
        if is_claude_like_pane(Some(&pane_current_command)) && !options.force {
            return Err(TmuxError::new(format!(
                "refusing to send: pane '{resolved}' is running '{pane_current_command}' (claude-like).\n  injecting keys would collide with the AI's turn.\n  pass --force to override (you really want to type into a live claude pane)"
            )));
        }

        self.runner
            .run(
                "send-keys",
                &tmux_send_command_args(resolved, command, options.literal),
            )
            .map(|_| TmuxSendCommandOutcome::Sent)
    }

    /// Paste tmux buffer into a target.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn paste_buffer(&mut self, target: &str) -> Result<(), TmuxError> {
        self.runner
            .run("paste-buffer", &["-t".to_owned(), target.to_owned()])
            .map(|_| ())
    }

    /// Load text into tmux buffer via stdin.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the buffer load.
    pub fn load_buffer(&mut self, text: &str) -> Result<(), TmuxError> {
        self.runner
            .run_with_stdin("load-buffer", &["-".to_owned()], text.as_bytes())
            .map(|_| ())
    }

    /// Smart text sending: buffer for multiline/long payloads, literal send otherwise, then submit-confirm.
    ///
    /// # Errors
    ///
    /// Returns the first tmux error from mode exit, text placement, paste, or Enter send.
    pub fn send_text(&mut self, target: &str, text: &str) -> Result<SendTextReport, TmuxError> {
        self.send_text_with_sleeper(target, text, std::thread::sleep)
    }

    fn send_text_with_sleeper<F>(
        &mut self,
        target: &str,
        text: &str,
        mut sleep: F,
    ) -> Result<SendTextReport, TmuxError>
    where
        F: FnMut(std::time::Duration),
    {
        self.exit_mode_if_needed(target)?;
        let used_buffer = text.contains('\n') || text.len() > 500;
        if used_buffer {
            self.load_buffer(text)?;
            self.paste_buffer(target)?;
        } else {
            self.send_keys_literal(target, text)?;
        }
        sleep(std::time::Duration::from_millis(SEND_SETTLE_MS));
        let (enter_attempts, warned_pending) =
            self.submit_with_confirm(target, text, &mut sleep)?;
        Ok(SendTextReport {
            used_buffer,
            enter_attempts,
            warned_pending,
        })
    }

    fn submit_with_confirm<F>(
        &mut self,
        target: &str,
        text: &str,
        sleep: &mut F,
    ) -> Result<(u32, bool), TmuxError>
    where
        F: FnMut(std::time::Duration),
    {
        for attempt in 1..=MAX_SUBMIT_ATTEMPTS {
            self.send_enter(target)?;
            sleep(std::time::Duration::from_millis(SUBMIT_CONFIRM_MS));
            match self.submit_pending_state_after_grace(target, text, sleep) {
                PendingInputState::Cleared => return Ok((attempt, false)),
                PendingInputState::DifferentInput => return Ok((attempt, true)),
                PendingInputState::MatchesSent => {}
            }
        }
        Ok((MAX_SUBMIT_ATTEMPTS, true))
    }

    fn submit_pending_state_after_grace<F>(
        &mut self,
        target: &str,
        text: &str,
        sleep: &mut F,
    ) -> PendingInputState
    where
        F: FnMut(std::time::Duration),
    {
        let _confirm_state = self.pending_input_state(target, text);
        sleep(std::time::Duration::from_millis(SUBMIT_GRACE_MS));
        self.pending_input_state(target, text)
    }

    fn pending_input_state(&mut self, target: &str, text: &str) -> PendingInputState {
        self.pane_pending_input(target).map_or(
            PendingInputState::Cleared,
            |pending| {
                if pending_input_matches_sent(&pending, text) {
                    PendingInputState::MatchesSent
                } else {
                    PendingInputState::DifferentInput
                }
            },
        )
    }

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
}

fn write_pane_title(
    runner: &mut dyn TmuxRunner,
    target: &str,
    title: Option<&str>,
) -> Result<(), TmuxError> {
    let Some(title) = title else {
        return Ok(());
    };
    runner.run(
        "select-pane",
        &[
            "-t".to_owned(),
            target.to_owned(),
            "-T".to_owned(),
            title.to_owned(),
        ],
    )?;
    Ok(())
}
