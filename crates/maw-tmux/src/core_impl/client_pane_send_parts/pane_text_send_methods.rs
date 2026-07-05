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
}
