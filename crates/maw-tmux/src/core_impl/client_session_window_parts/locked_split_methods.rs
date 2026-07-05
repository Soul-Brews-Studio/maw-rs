impl<R> TmuxClient<R>
where
    R: TmuxRunner,
{

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
