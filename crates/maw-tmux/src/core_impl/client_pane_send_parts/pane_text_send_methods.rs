impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{

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

    /// Deliver arbitrary text as data through a short-lived tmux buffer.
    ///
    /// The text is written to `load-buffer` over stdin, pasted verbatim, and the
    /// buffer is deleted by `paste-buffer -d`. When `submit` is true, exactly one
    /// Enter key is sent after the paste; trailing newlines have no special meaning.
    ///
    /// # Errors
    ///
    /// Returns the first runner error from loading, pasting, or submitting.
    pub fn paste_text(
        &mut self,
        target: &str,
        text: &str,
        submit: bool,
    ) -> Result<(), TmuxError> {
        self.load_buffer(text)?;
        self.runner.run(
            "paste-buffer",
            &["-d".to_owned(), "-t".to_owned(), target.to_owned()],
        )?;
        if submit {
            self.send_enter(target)?;
        }
        Ok(())
    }

    /// Smart text sending: buffer for multiline/long payloads, literal send otherwise, then submit-confirm.
    ///
    /// # Errors
    ///
    /// Returns a tmux error or refuses preexisting/unconfirmed pending input.
    pub fn send_text(&mut self, target: &str, text: &str) -> Result<SendTextReport, TmuxError> {
        self.send_text_with_sleeper(target, text, std::thread::sleep)
    }

    /// Run [`Self::send_text`] with an injected sleep function for deterministic
    /// callers. Production callbacks must honor every requested delay.
    ///
    /// # Errors
    ///
    /// Returns the same fail-closed errors as [`Self::send_text`].
    pub fn send_text_with_sleeper<F>(
        &mut self,
        target: &str,
        text: &str,
        mut sleep: F,
    ) -> Result<SendTextReport, TmuxError>
    where
        F: FnMut(std::time::Duration),
    {
        self.exit_mode_if_needed(target)?;
        self.ensure_composer_empty(target)?;
        let used_buffer = text.contains('\n') || text.len() > 500;
        if used_buffer {
            self.load_buffer(text)?;
            self.paste_buffer(target)?;
        } else {
            self.send_keys_literal(target, text)?;
        }
        sleep(std::time::Duration::from_millis(SEND_SETTLE_MS));
        // Snapshot an opaque TUI rendering before Enter; capture failure leaves
        // no baseline, preserving the fail-stop behavior for different input.
        let buffered_pending = if used_buffer {
            self.capture_pending_input(target).ok().flatten()
        } else {
            None
        };
        let enter_attempts = self.submit_with_confirm(
            target,
            text,
            buffered_pending.as_deref(),
            &mut sleep,
        )?;
        Ok(SendTextReport {
            used_buffer,
            enter_attempts,
            warned_pending: false,
        })
    }

    fn ensure_composer_empty(&mut self, target: &str) -> Result<(), TmuxError> {
        if self.capture_pending_input(target)?.is_some() {
            return Err(TmuxError::new(format!(
                "refusing to append text: pane '{target}' already has pending input; submit or clear it first"
            )));
        }
        Ok(())
    }

    fn submit_with_confirm<F>(
        &mut self,
        target: &str,
        text: &str,
        buffered_pending: Option<&str>,
        sleep: &mut F,
    ) -> Result<u32, TmuxError>
    where
        F: FnMut(std::time::Duration),
    {
        for attempt in 1..=MAX_SUBMIT_ATTEMPTS {
            self.send_enter(target)?;
            sleep(std::time::Duration::from_millis(SUBMIT_CONFIRM_MS));
            let state = self
                .submit_pending_state_after_grace(target, text, buffered_pending, sleep)
                .map_err(|error| Self::pending_confirm_error(target, &error))?;
            match state {
                PendingInputState::Cleared => return Ok(attempt),
                PendingInputState::DifferentInput => return Err(Self::pending_submit_error(target)),
                PendingInputState::MatchesSent => {}
            }
        }
        Err(Self::pending_submit_error(target))
    }

    fn submit_pending_state_after_grace<F>(
        &mut self,
        target: &str,
        text: &str,
        buffered_pending: Option<&str>,
        sleep: &mut F,
    ) -> Result<PendingInputState, TmuxError>
    where
        F: FnMut(std::time::Duration),
    {
        let confirm_state = self.pending_input_state(target, text, buffered_pending)?;
        sleep(std::time::Duration::from_millis(SUBMIT_GRACE_MS));
        let grace_state = self.pending_input_state(target, text, buffered_pending)?;
        if confirm_state == PendingInputState::DifferentInput {
            Ok(confirm_state)
        } else {
            Ok(grace_state)
        }
    }

    fn pending_input_state(
        &mut self,
        target: &str,
        text: &str,
        buffered_pending: Option<&str>,
    ) -> Result<PendingInputState, TmuxError> {
        Ok(self.capture_pending_input(target)?.map_or(
            PendingInputState::Cleared,
            |pending| {
                if pending_input_matches_sent(&pending, text)
                    || buffered_pending
                        .is_some_and(|baseline| pending_input_matches_sent(&pending, baseline))
                {
                    PendingInputState::MatchesSent
                } else {
                    PendingInputState::DifferentInput
                }
            },
        ))
    }

    fn capture_pending_input(&mut self, target: &str) -> Result<Option<String>, TmuxError> {
        self.runner
            .run(
                "capture-pane",
                &[
                    "-t".to_owned(),
                    target.to_owned(),
                    "-e".to_owned(),
                    "-p".to_owned(),
                    "-J".to_owned(),
                    "-S".to_owned(),
                    "-80".to_owned(),
                ],
            )
            .map(|content| pane_pending_input_from_capture(&content))
    }

    fn pending_submit_error(target: &str) -> TmuxError {
        TmuxError::new(format!(
            "pane '{target}' still has pending input after Enter retries; delivery could not be confirmed; inspect the pane before retrying"
        ))
    }

    fn pending_confirm_error(target: &str, error: &TmuxError) -> TmuxError {
        TmuxError::new(format!(
            "pane '{target}' delivery could not be confirmed after Enter: {}; inspect the pane before retrying",
            error.message
        ))
    }
}
