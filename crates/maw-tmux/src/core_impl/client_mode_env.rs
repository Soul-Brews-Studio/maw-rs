impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{

    /// Leave tmux copy-mode when the target reports `#{pane_in_mode} == 1`.
    ///
    /// # Errors
    ///
    /// Returns non-`not in a mode` cancellation errors from tmux. Probe failures return `Ok(false)`.
    pub fn exit_mode_if_needed(&mut self, target: &str) -> Result<bool, TmuxError> {
        let probe = self.runner.run(
            "display-message",
            &[
                "-t".to_owned(),
                target.to_owned(),
                "-p".to_owned(),
                "#{pane_in_mode}".to_owned(),
            ],
        );
        if probe.is_ok_and(|raw| raw.trim() == "1") {
            return match self.runner.run(
                "send-keys",
                &[
                    "-t".to_owned(),
                    target.to_owned(),
                    "-X".to_owned(),
                    "cancel".to_owned(),
                ],
            ) {
                Ok(_) => Ok(true),
                Err(error) if error.message.contains("not in a mode") => Ok(false),
                Err(error) => Err(error),
            };
        }
        Ok(false)
    }

    /// Check whether captured pane text still appears to contain unsubmitted input.
    pub fn pane_input_pending(&mut self, target: &str) -> bool {
        self.pane_pending_input(target).is_some()
    }

    /// Return the captured pending prompt text, when the current pane prompt has any.
    pub fn pane_pending_input(&mut self, target: &str) -> Option<String> {
        self.capture(target, Some(5))
            .ok()
            .and_then(|content| pane_pending_input_from_capture(&content))
    }

    /// Set a tmux environment variable.
    ///
    /// # Errors
    ///
    /// Returns the runner error when tmux rejects the request.
    pub fn set_environment(
        &mut self,
        session: &str,
        key: &str,
        value: &str,
    ) -> Result<(), TmuxError> {
        self.runner
            .run(
                "set-environment",
                &[
                    "-t".to_owned(),
                    session.to_owned(),
                    key.to_owned(),
                    value.to_owned(),
                ],
            )
            .map(|_| ())
    }

    /// Set a tmux option best-effort.
    pub fn set_option(&mut self, target: &str, option: &str, value: &str) {
        self.try_run(
            "set-option",
            &[
                "-t".to_owned(),
                target.to_owned(),
                option.to_owned(),
                value.to_owned(),
            ],
        );
    }

    /// Set a tmux value best-effort.
    pub fn set(&mut self, target: &str, option: &str, value: &str) {
        self.try_run(
            "set",
            &[
                "-t".to_owned(),
                target.to_owned(),
                option.to_owned(),
                value.to_owned(),
            ],
        );
    }

    fn try_run(&mut self, subcommand: &str, args: &[String]) -> String {
        self.runner.run(subcommand, args).unwrap_or_default()
    }
}
