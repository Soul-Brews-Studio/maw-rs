const ACTIVITY_USAGE: &str = "usage: maw activity <pane> [--watch] [--json] [--stuck-only] [--window=<dur>] [--samples=N] [--sampler=peek|follow] | maw activity --all [--watch] [--json] [--stuck-only] [--window=<dur>] [--samples=N] [--sampler=peek|follow]";
const ACTIVITY_PEEK_LINES: u32 = 80;
const ACTIVITY_ALL_CONCURRENCY: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityState {
    Busy,
    Idle,
    Stuck,
}

impl ActivityState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Idle => "idle",
            Self::Stuck => "stuck",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivityConfidence {
    Low,
    Medium,
    High,
}

impl ActivityConfidence {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivitySampler {
    Peek,
    Follow,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct ActivityOptions {
    all: bool,
    watch: bool,
    json: bool,
    stuck_only: bool,
    window: Option<String>,
    samples: Option<u32>,
    sampler: Option<String>,
    watch_iterations: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
struct ParsedActivityOptions {
    window_ms: u64,
    samples: u32,
    sampler: ActivitySampler,
}

#[derive(Debug, Clone, PartialEq)]
struct ActivityResult {
    pane: String,
    state: ActivityState,
    confidence: ActivityConfidence,
    samples: u32,
    diff_samples: u32,
    last_change_ago_seconds: f64,
    sample_window_seconds: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivitySample {
    text: String,
    at_ms: u64,
}

trait ActivityTmux {
    fn capture(&mut self, target: &str, lines: u32) -> Result<String, String>;
    fn list_all(&mut self) -> Vec<TmuxSession>;
}

struct LocalActivityTmux {
    client: TmuxClient<maw_tmux::CommandTmuxRunner>,
}

impl LocalActivityTmux {
    fn new() -> Self {
        Self {
            client: TmuxClient::local(),
        }
    }
}

impl ActivityTmux for LocalActivityTmux {
    fn capture(&mut self, target: &str, lines: u32) -> Result<String, String> {
        validate_activity_tmux_target(target)?;
        self.client
            .capture(target, Some(lines))
            .map_err(|error| error.message)
    }

    fn list_all(&mut self) -> Vec<TmuxSession> {
        self.client.list_all()
    }
}

trait ActivityClock {
    fn now_ms(&mut self) -> u64;
    fn sleep_ms(&mut self, ms: u64);
}

#[derive(Default)]
struct RealActivityClock;

impl ActivityClock for RealActivityClock {
    fn now_ms(&mut self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
    }

    fn sleep_ms(&mut self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
}

fn run_activity_command(argv: &[String]) -> CliOutput {
    let parsed = match parse_activity_cli(argv) {
        Ok(parsed) => parsed,
        Err(message) => {
            let code = if message == ACTIVITY_USAGE { 2 } else { 1 };
            return CliOutput {
                code,
                stdout: String::new(),
                stderr: format!("{message}\n"),
            };
        }
    };
    let mut tmux = LocalActivityTmux::new();
    let mut clock = RealActivityClock;
    match cmd_activity(parsed.0.as_deref(), &parsed.1, &mut tmux, &mut clock) {
        Ok(output) => CliOutput {
            code: 0,
            stdout: output.stdout,
            stderr: output.stderr,
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("activity: {message}\n"),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ActivityOutput {
    stdout: String,
    stderr: String,
}

