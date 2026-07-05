
/// Concrete tmux runner backed by `std::process::Command`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandTmuxRunner {
    program: OsString,
    socket: Option<OsString>,
}

impl Default for CommandTmuxRunner {
    fn default() -> Self {
        Self {
            program: OsString::from("tmux"),
            socket: None,
        }
    }
}

impl CommandTmuxRunner {
    /// Create a runner that invokes the default `tmux` binary.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a runner that invokes a custom tmux-compatible program.
    #[must_use]
    pub fn with_program(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            socket: None,
        }
    }

    /// Set the tmux socket passed as `-S <socket>`.
    #[must_use]
    pub fn with_socket(mut self, socket: impl Into<OsString>) -> Self {
        self.socket = Some(socket.into());
        self
    }

    /// Return the exact argv vector this runner will execute.
    ///
    /// This keeps runtime command construction testable without requiring a live tmux server.
    #[must_use]
    pub fn argv(&self, subcommand: &str, tmux_args: &[String]) -> Vec<OsString> {
        let mut command_line = vec![self.program.clone()];
        if let Some(socket) = &self.socket {
            command_line.push(OsString::from("-S"));
            command_line.push(socket.clone());
        }
        command_line.push(OsString::from(subcommand));
        command_line.extend(tmux_args.iter().map(OsString::from));
        command_line
    }
}

impl TmuxRunner for CommandTmuxRunner {
    fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, TmuxError> {
        self.run_command(subcommand, args, None)
    }

    fn run_with_stdin(
        &mut self,
        subcommand: &str,
        args: &[String],
        stdin: &[u8],
    ) -> Result<String, TmuxError> {
        self.run_command(subcommand, args, Some(stdin))
    }
}

impl CommandTmuxRunner {
    fn run_command(
        &self,
        subcommand: &str,
        args: &[String],
        stdin: Option<&[u8]>,
    ) -> Result<String, TmuxError> {
        let command_line = self.argv(subcommand, args);
        let (program, rest) = command_line
            .split_first()
            .expect("tmux command line always includes a program");
        validate_tmux_program(program)?;
        validate_tmux_option_values(rest)?;
        let mut command = Command::new(program);
        command.args(rest);
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
        if stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        let mut child = command.spawn().map_err(|error| {
            TmuxError::new(format!(
                "failed to execute {}: {error}",
                program.to_string_lossy()
            ))
        })?;
        if let Some(stdin) = stdin {
            let mut child_stdin = child
                .stdin
                .take()
                .ok_or_else(|| TmuxError::new("failed to open tmux stdin"))?;
            child_stdin
                .write_all(stdin)
                .map_err(|error| tmux_program_io_error("write stdin for", program, &error))?;
        }
        let output = child
            .wait_with_output()
            .map_err(|error| tmux_program_io_error("collect output from", program, &error))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let detail = if stderr.is_empty() { stdout } else { stderr };
        let code = output
            .status
            .code()
            .map_or_else(|| "signal".to_owned(), |code| code.to_string());
        if detail.is_empty() {
            Err(TmuxError::new(format!("tmux exited with status {code}")))
        } else {
            Err(TmuxError::new(format!(
                "tmux exited with status {code}: {detail}"
            )))
        }
    }
}

fn validate_tmux_program(program: &std::ffi::OsStr) -> Result<(), TmuxError> {
    let display = program.to_string_lossy();
    if display.is_empty() || display.trim() != display || display.starts_with('-') {
        Err(TmuxError::new(
            "tmux program must be non-empty, unpadded, and not start with '-'",
        ))
    } else {
        Ok(())
    }
}

fn validate_tmux_option_values(args: &[OsString]) -> Result<(), TmuxError> {
    let mut previous_wants_target = false;
    for arg in args {
        let value = arg.to_string_lossy();
        if previous_wants_target {
            if value.is_empty() || value.trim() != value || value.starts_with('-') {
                return Err(TmuxError::new(
                    "tmux target/session must be non-empty, unpadded, and not start with '-'",
                ));
            }
            previous_wants_target = false;
            continue;
        }
        previous_wants_target = matches!(value.as_ref(), "-t" | "-s");
    }
    if previous_wants_target {
        return Err(TmuxError::new("tmux target/session option missing value"));
    }
    Ok(())
}

fn tmux_program_io_error(
    action: &str,
    program: &std::ffi::OsStr,
    error: &std::io::Error,
) -> TmuxError {
    TmuxError::new(format!(
        "failed to {action} {}: {error}",
        program.to_string_lossy()
    ))
}
