const DISPATCH_107: &[DispatcherEntry] = &[
    DispatcherEntry { command: "activity", handler: Handler::Sync(activity_run_command) },
];

#[derive(Debug, Default)]
struct ActivityEnvTmux {
    captures: Vec<String>,
}

impl ActivityTmux for ActivityEnvTmux {
    fn capture(&mut self, _target: &str, _lines: u32) -> Result<String, String> {
        if self.captures.is_empty() {
            Ok(String::new())
        } else {
            Ok(self.captures.remove(0))
        }
    }

    fn list_all(&mut self) -> Vec<TmuxSession> { Vec::new() }
}

#[derive(Debug)]
struct ActivityEnvClock {
    now: u64,
    step: u64,
}

impl ActivityClock for ActivityEnvClock {
    fn now_ms(&mut self) -> u64 {
        let now = self.now;
        self.now = self.now.saturating_add(self.step);
        now
    }

    fn sleep_ms(&mut self, ms: u64) { self.now = self.now.saturating_add(ms); }
}

fn activity_run_command(argv: &[String]) -> CliOutput {
    if std::env::var_os("MAW_RS_ACTIVITY_FAKE_CAPTURE").is_some() {
        return activity_run_fake_command(argv);
    }
    run_activity_command(argv)
}

fn activity_run_fake_command(argv: &[String]) -> CliOutput {
    let parsed = match parse_activity_cli(argv) {
        Ok(parsed) => parsed,
        Err(message) => return activity_parse_error(&message),
    };
    let mut tmux = ActivityEnvTmux { captures: activity_fake_captures() };
    let mut clock = ActivityEnvClock { now: 0, step: 1_000 };
    match cmd_activity(parsed.0.as_deref(), &parsed.1, &mut tmux, &mut clock) {
        Ok(output) => CliOutput { code: 0, stdout: output.stdout, stderr: output.stderr },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("activity: {message}\n") },
    }
}

fn activity_parse_error(message: &str) -> CliOutput {
    let code = if message == ACTIVITY_USAGE { 2 } else { 1 };
    CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") }
}

fn activity_fake_captures() -> Vec<String> {
    std::env::var("MAW_RS_ACTIVITY_FAKE_CAPTURE")
        .unwrap_or_default()
        .split("\n---sample---\n")
        .map(ToOwned::to_owned)
        .collect()
}
