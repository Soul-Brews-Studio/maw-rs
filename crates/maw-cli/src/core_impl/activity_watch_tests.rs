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

fn parse_activity_cli(argv: &[String]) -> Result<(Option<String>, ActivityOptions), String> {
    let mut opts = ActivityOptions {
        all: false,
        watch: false,
        json: false,
        stuck_only: false,
        window: None,
        samples: None,
        sampler: None,
        watch_iterations: None,
    };
    let mut target: Option<String> = None;
    let mut index = 0;
    while index < argv.len() {
        let arg = &argv[index];
        match arg.as_str() {
            "--help" | "-h" => return Err(ACTIVITY_USAGE.to_owned()),
            "--all" => opts.all = true,
            "--watch" => opts.watch = true,
            "--json" => opts.json = true,
            "--stuck-only" => opts.stuck_only = true,
            "--window" => {
                index += 1;
                let Some(value) = argv.get(index) else { return Err(ACTIVITY_USAGE.to_owned()); };
                opts.window = Some(value.clone());
            }
            "--samples" => {
                index += 1;
                let Some(value) = argv.get(index) else { return Err(ACTIVITY_USAGE.to_owned()); };
                opts.samples = Some(value.parse::<u32>().map_err(|_| "activity: --samples must be an integer from 2 to 50".to_owned())?);
            }
            "--sampler" => {
                index += 1;
                let Some(value) = argv.get(index) else { return Err(ACTIVITY_USAGE.to_owned()); };
                opts.sampler = Some(value.clone());
            }
            _ if arg.starts_with("--window=") => opts.window = Some(arg[9..].to_owned()),
            _ if arg.starts_with("--samples=") => {
                opts.samples = Some(arg[10..].parse::<u32>().map_err(|_| "activity: --samples must be an integer from 2 to 50".to_owned())?);
            }
            _ if arg.starts_with("--sampler=") => opts.sampler = Some(arg[10..].to_owned()),
            _ if arg.starts_with('-') => return Err(ACTIVITY_USAGE.to_owned()),
            _ => {
                if target.replace(arg.clone()).is_some() {
                    return Err(ACTIVITY_USAGE.to_owned());
                }
            }
        }
        index += 1;
    }
    if opts.all && target.is_some() {
        return Err(ACTIVITY_USAGE.to_owned());
    }
    if !opts.all && target.is_none() {
        return Err(ACTIVITY_USAGE.to_owned());
    }
    if let Some(raw_target) = target.as_deref() {
        validate_activity_tmux_target(raw_target)?;
    }
    Ok((target, opts))
}

fn parse_activity_options(opts: &ActivityOptions) -> Result<ParsedActivityOptions, String> {
    let window_ms = match opts.window.as_deref() {
        None => 30_000,
        Some(value) => parse_activity_duration_ms(value)
            .ok_or_else(|| format!("activity: invalid --window duration: {value}"))?,
    };
    if window_ms == 0 {
        return Err(format!(
            "activity: invalid --window duration: {}",
            opts.window.as_deref().unwrap_or("")
        ));
    }
    let sample_count = opts.samples.unwrap_or(3);
    if !(2..=50).contains(&sample_count) {
        return Err("activity: --samples must be an integer from 2 to 50".to_owned());
    }
    let sampler_kind = match opts.sampler.as_deref().unwrap_or("peek") {
        "peek" => ActivitySampler::Peek,
        "follow" => ActivitySampler::Follow,
        _ => return Err("activity: --sampler must be peek or follow".to_owned()),
    };
    Ok(ParsedActivityOptions {
        window_ms,
        samples: sample_count,
        sampler: sampler_kind,
    })
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn parse_activity_duration_ms(value: &str) -> Option<u64> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed != value {
        return None;
    }
    let split = trimmed
        .find(|ch: char| !ch.is_ascii_digit() && ch != '.')
        .unwrap_or(trimmed.len());
    let (number, unit) = trimmed.split_at(split);
    if number.is_empty() {
        return None;
    }
    let amount = number.parse::<f64>().ok()?;
    if !amount.is_finite() || amount <= 0.0 {
        return None;
    }
    let multiplier = match unit {
        "" | "ms" => 1.0,
        "s" | "sec" | "secs" | "second" | "seconds" => 1_000.0,
        "m" | "min" | "mins" | "minute" | "minutes" => 60_000.0,
        "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000.0,
        _ => return None,
    };
    let ms = amount * multiplier;
    if ms > u64::MAX as f64 {
        return None;
    }
    Some(ms.round() as u64)
}

fn validate_activity_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        return Err("activity: tmux target must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if target.chars().all(|ch| ch.is_ascii_digit()) {
        return Err("activity: bare numeric tmux targets are refused; use session:window or %pane_id".to_owned());
    }
    if !target
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | ':' | '%' | '-'))
    {
        return Err("activity: tmux target contains unsupported characters".to_owned());
    }
    Ok(())
}

fn cmd_activity(
    target: Option<&str>,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityOutput, String> {
    if opts.watch {
        return cmd_activity_watch(target, opts, tmux, clock);
    }
    cmd_activity_once(target, opts, tmux, clock)
}

fn cmd_activity_once(
    target: Option<&str>,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityOutput, String> {
    let mut stderr = String::new();
    let results = if opts.all {
        if !opts.json {
            let _ = writeln!(stderr, "activity: surveying fleet ({})...", sampling_description(opts)?);
        }
        sample_all_activity(opts, tmux, clock)?
    } else {
        let target = target.ok_or_else(|| ACTIVITY_USAGE.to_owned())?;
        vec![sample_activity(target, opts, tmux, clock)?]
    };
    let visible = filter_activity_results(&results, opts);
    Ok(ActivityOutput {
        stdout: format_activity_output(&visible, opts),
        stderr,
    })
}

fn cmd_activity_watch(
    target: Option<&str>,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityOutput, String> {
    if !opts.all && target.is_none() {
        return Err(ACTIVITY_USAGE.to_owned());
    }
    let max = opts.watch_iterations.unwrap_or(u32::MAX);
    let mut stdout = String::new();
    let mut previous = BTreeMap::<String, ActivityState>::new();
    let mut transition_count = 0u32;
    let scope = if opts.all { "fleet" } else { target.unwrap_or("") };
    if !opts.json {
        stdout.push_str(&format_watch_table(scope, &[], opts, Some("sampling"), None)?);
    }
    for iteration in 0..max {
        let results = if opts.all {
            sample_all_activity(opts, tmux, clock)?
        } else {
            vec![sample_activity(target.unwrap_or(""), opts, tmux, clock)?]
        };
        let transitions = record_activity_transitions(&results, &mut previous);
        transition_count = transition_count.saturating_add(u32::try_from(transitions.len()).unwrap_or(u32::MAX));
        if opts.json {
            for result in transitions {
                if opts.stuck_only && result.state != ActivityState::Stuck {
                    continue;
                }
                stdout.push_str(&format_activity_json(&result));
                stdout.push('\n');
            }
            continue;
        }
        let visible = filter_activity_results(&results, opts);
        let footer = format!(
            "watching ({}) · last refresh: {} · transitions={transition_count}",
            sampling_description(opts)?,
            format_activity_time(clock.now_ms())
        );
        stdout.push_str(&format_watch_table(
            scope,
            &visible,
            opts,
            Some(&format!("refresh={}", iteration + 1)),
            Some(&footer),
        )?);
    }
    Ok(ActivityOutput {
        stdout,
        stderr: String::new(),
    })
}

fn sample_activity(
    target: &str,
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityResult, String> {
    validate_activity_tmux_target(target)?;
    let parsed = parse_activity_options(opts)?;
    sample_resolved_activity(target, target, &parsed, tmux, clock)
}

fn sample_all_activity(
    opts: &ActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<Vec<ActivityResult>, String> {
    let parsed = parse_activity_options(opts)?;
    let sessions = tmux.list_all();
    let targets = all_activity_targets(&load_native_fleet());
    let mut results = Vec::new();
    for target in targets.into_iter().take(ACTIVITY_ALL_CONCURRENCY.max(1) * 1_000) {
        let Some(snapshot_target) = resolve_activity_peek_target(&sessions, &target) else { continue; };
        if validate_activity_tmux_target(&snapshot_target).is_err() {
            continue;
        }
        if let Ok(result) = sample_resolved_activity(&target, &snapshot_target, &parsed, tmux, clock) {
            results.push(result);
        }
    }
    results.sort_by(|a, b| a.pane.cmp(&b.pane));
    Ok(results)
}

fn all_activity_targets(entries: &[NativeFleetSession]) -> Vec<String> {
    let mut targets = BTreeSet::new();
    for entry in entries {
        if entry.windows.is_empty() {
            targets.insert(entry.name.clone());
            continue;
        }
        for window in &entry.windows {
            let name = if window.name.is_empty() {
                entry.name.clone()
            } else {
                window.name.clone()
            };
            targets.insert(if name.contains(':') {
                name
            } else {
                format!("{}:{name}", entry.name)
            });
        }
    }
    targets.into_iter().collect()
}

fn resolve_activity_peek_target(sessions: &[TmuxSession], target: &str) -> Option<String> {
    let (session_name, window_part) = target.split_once(':')?;
    let window_name = window_part
        .rsplit_once('.')
        .and_then(|(window, pane)| pane.parse::<u32>().ok().map(|_| window))
        .unwrap_or(window_part);
    if window_name.parse::<u32>().is_ok() {
        return Some(target.to_owned());
    }
    let session = sessions.iter().find(|session| session.name == session_name)?;
    let window = session.windows.iter().find(|window| window.name == window_name)?;
    let base = format!("{}:{}", session.name, window.index);
    target
        .rsplit_once('.')
        .and_then(|(_, pane)| pane.parse::<u32>().ok().map(|_| pane.to_owned()))
        .map_or(Some(base.clone()), |pane| Some(format!("{base}.{pane}")))
}

fn sample_resolved_activity(
    pane: &str,
    snapshot_target: &str,
    parsed: &ParsedActivityOptions,
    tmux: &mut dyn ActivityTmux,
    clock: &mut dyn ActivityClock,
) -> Result<ActivityResult, String> {
    let interval_ms = if parsed.samples <= 1 {
        0
    } else {
        parsed.window_ms / u64::from(parsed.samples - 1)
    };
    let mut samples = Vec::new();
    for index in 0..parsed.samples {
        if index > 0 {
            clock.sleep_ms(interval_ms);
        }
        let lines = match parsed.sampler {
            ActivitySampler::Peek | ActivitySampler::Follow => ACTIVITY_PEEK_LINES,
        };
        let text = tmux.capture(snapshot_target, lines)?;
        let at_ms = clock.now_ms();
        samples.push(ActivitySample { text, at_ms });
    }
    Ok(classify_activity_snapshots(pane, &samples, parsed.window_ms))
}

#[allow(clippy::cast_precision_loss)]
fn classify_activity_snapshots(pane: &str, raw_samples: &[ActivitySample], window_ms: u64) -> ActivityResult {
    let normalized = raw_samples
        .iter()
        .map(|sample| normalize_activity_snapshot(&sample.text))
        .collect::<Vec<_>>();
    let mut changed_indexes = BTreeSet::new();
    let mut last_change_at = None;
    for index in 1..normalized.len() {
        if normalized[index] != normalized[index - 1] {
            changed_indexes.insert(index - 1);
            changed_indexes.insert(index);
            last_change_at = raw_samples.get(index).map(|sample| sample.at_ms);
        }
    }
    let end = raw_samples.last().map_or(0, |sample| sample.at_ms);
    let state = if changed_indexes.is_empty() {
        if raw_samples.last().is_some_and(|sample| is_stuck_activity_snapshot(&sample.text)) {
            ActivityState::Stuck
        } else {
            ActivityState::Idle
        }
    } else {
        ActivityState::Busy
    };
    let sample_window_seconds = round_seconds(window_ms as f64 / 1000.0);
    let last_change_ago_seconds = last_change_at.map_or(sample_window_seconds, |changed| {
        round_seconds(end.saturating_sub(changed) as f64 / 1000.0)
    });
    ActivityResult {
        pane: pane.to_owned(),
        state,
        confidence: confidence_for_activity(raw_samples.len()),
        samples: u32::try_from(raw_samples.len()).unwrap_or(u32::MAX),
        diff_samples: u32::try_from(changed_indexes.len()).unwrap_or(u32::MAX),
        last_change_ago_seconds,
        sample_window_seconds,
    }
}

fn normalize_activity_snapshot(input: &str) -> String {
    strip_activity_ansi(input)
        .replace('\r', "\n")
        .split('\n')
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

fn strip_activity_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\u{1b}' {
            out.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                let _ = chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                let _ = chars.next();
                while let Some(next) = chars.next() {
                    if next == '\u{7}' {
                        break;
                    }
                    if next == '\u{1b}' && chars.peek() == Some(&'\\') {
                        let _ = chars.next();
                        break;
                    }
                }
            }
            Some('(' | ')') => {
                let _ = chars.next();
                let _ = chars.next();
            }
            _ => {}
        }
    }
    out
}

fn is_stuck_activity_snapshot(input: &str) -> bool {
    let normalized = normalize_activity_snapshot(input);
    let lines = normalized
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .rev()
        .take(10)
        .collect::<Vec<_>>();
    if lines.iter().any(|line| {
        matches!(*line, ">" | "$" | "#" | "❯" | "›" | "λ")
            || matches!(*line, "> ▌" | "$ ▌" | "# ▌" | "❯ ▌" | "› ▌" | "λ ▌")
    }) {
        return true;
    }
    let lower = normalized.to_ascii_lowercase();
    lower.ends_with("type a message")
        || lower.ends_with("send a message")
        || lower.ends_with("what can i help with?")
        || lower.ends_with("what can i help with")
        || lower.contains("claude code") && lower.ends_with('>')
}

const fn confidence_for_activity(samples: usize) -> ActivityConfidence {
    if samples >= 3 {
        ActivityConfidence::High
    } else if samples == 2 {
        ActivityConfidence::Medium
    } else {
        ActivityConfidence::Low
    }
}

fn round_seconds(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

fn filter_activity_results(results: &[ActivityResult], opts: &ActivityOptions) -> Vec<ActivityResult> {
    results
        .iter()
        .filter(|result| !opts.stuck_only || result.state == ActivityState::Stuck)
        .cloned()
        .collect()
}

fn record_activity_transitions(
    results: &[ActivityResult],
    previous: &mut BTreeMap<String, ActivityState>,
) -> Vec<ActivityResult> {
    let mut changed = Vec::new();
    for result in results {
        let prev = previous.insert(result.pane.clone(), result.state);
        if prev.is_some_and(|prev| prev != result.state) {
            changed.push(result.clone());
        }
    }
    changed
}

fn format_activity_output(results: &[ActivityResult], opts: &ActivityOptions) -> String {
    if opts.json {
        if opts.all {
            format!("[{}]\n", results.iter().map(format_activity_json_object).collect::<Vec<_>>().join(","))
        } else {
            results.first().map_or_else(String::new, |result| format_activity_json(result) + "\n")
        }
    } else {
        let text = results
            .iter()
            .map(format_activity_human)
            .collect::<Vec<_>>()
            .join("\n");
        if text.is_empty() {
            String::new()
        } else {
            format!("{text}\n")
        }
    }
}

fn format_activity_json(result: &ActivityResult) -> String {
    format_activity_json_object(result)
}

fn format_activity_json_object(result: &ActivityResult) -> String {
    format!(
        "{{\"pane\":{},\"state\":{},\"confidence\":{},\"samples\":{},\"diff_samples\":{},\"last_change_ago_seconds\":{},\"sample_window_seconds\":{}}}",
        json_string(&result.pane),
        json_string(result.state.as_str()),
        json_string(result.confidence.as_str()),
        result.samples,
        result.diff_samples,
        json_number(result.last_change_ago_seconds),
        json_number(result.sample_window_seconds),
    )
}

fn format_activity_human(result: &ActivityResult) -> String {
    let icon = match result.state {
        ActivityState::Busy => "🟢",
        ActivityState::Idle => "🟡",
        ActivityState::Stuck => "🔴",
    };
    let age = match result.state {
        ActivityState::Busy => format!("last change {} ago", format_activity_duration(result.last_change_ago_seconds)),
        ActivityState::Stuck => format!("at prompt (no change in {})", format_activity_duration(result.last_change_ago_seconds)),
        ActivityState::Idle => format!("quiet (no change in {})", format_activity_duration(result.last_change_ago_seconds)),
    };
    format!(
        "{}: {icon} {} ({age}, {}/{} samples diff)",
        result.pane,
        result.state.as_str().to_ascii_uppercase(),
        result.diff_samples,
        result.samples,
    )
}

#[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)]
fn format_activity_duration(seconds: f64) -> String {
    if seconds < 60.0 {
        return format!("{}s", seconds.round() as u64);
    }
    let minutes = (seconds / 60.0).round() as u64;
    if minutes < 60 {
        return format!("{minutes}m");
    }
    format!("{}h", ((minutes as f64) / 60.0).round() as u64)
}

#[allow(clippy::cast_precision_loss)]
fn format_activity_duration_ms(ms: u64) -> String {
    if ms < 1_000 {
        return format!("{ms}ms");
    }
    if ms.is_multiple_of(1_000) {
        return format_activity_duration(ms as f64 / 1_000.0);
    }
    let mut text = format!("{:.1}", ms as f64 / 1_000.0);
    if text.ends_with(".0") {
        text.truncate(text.len() - 2);
    }
    format!("{text}s")
}

fn sampling_description(opts: &ActivityOptions) -> Result<String, String> {
    let parsed = parse_activity_options(opts)?;
    let sampler = match parsed.sampler {
        ActivitySampler::Peek => "peek",
        ActivitySampler::Follow => "follow",
    };
    Ok(format!(
        "window={}, samples={}, sampler={sampler}",
        format_activity_duration_ms(parsed.window_ms),
        parsed.samples
    ))
}

fn format_watch_table(
    scope: &str,
    results: &[ActivityResult],
    opts: &ActivityOptions,
    status: Option<&str>,
    footer: Option<&str>,
) -> Result<String, String> {
    let rows = results.iter().map(format_activity_human).collect::<Vec<_>>();
    let empty = if opts.stuck_only { "(no stuck panes)" } else { "(no panes resolved)" };
    let body = if rows.is_empty() {
        if status == Some("sampling") { "(sampling...)".to_owned() } else { empty.to_owned() }
    } else {
        rows.join("\n")
    };
    let description = if let Some(status) = status {
        format!("{}, {status}", sampling_description(opts)?)
    } else {
        sampling_description(opts)?
    };
    let footer_block = footer.map_or(String::new(), |footer| {
        format!("\n───────────────────────────────────────────────────────────────────────────────\n{footer}")
    });
    Ok(format!(
        "activity: watching {scope} ({description}); press Ctrl-C to stop\n{body}{footer_block}\n"
    ))
}

fn json_number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn format_activity_time(ms: u64) -> String {
    let seconds = (ms / 1_000) % 86_400;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

#[cfg(test)]
#[allow(clippy::redundant_closure_for_method_calls)]
mod activity_tests {
    use super::*;

    #[derive(Debug, Default)]
    struct FakeTmux {
        captures: Vec<String>,
        sessions: Vec<TmuxSession>,
        seen_targets: Vec<String>,
    }

    impl ActivityTmux for FakeTmux {
        fn capture(&mut self, target: &str, _lines: u32) -> Result<String, String> {
            self.seen_targets.push(target.to_owned());
            if self.captures.is_empty() {
                Ok(String::new())
            } else {
                Ok(self.captures.remove(0))
            }
        }

        fn list_all(&mut self) -> Vec<TmuxSession> {
            self.sessions.clone()
        }
    }

    #[derive(Debug)]
    struct FakeClock {
        now: u64,
        sleeps: Vec<u64>,
    }

    impl ActivityClock for FakeClock {
        fn now_ms(&mut self) -> u64 {
            let now = self.now;
            self.now += 1_000;
            now
        }

        fn sleep_ms(&mut self, ms: u64) {
            self.sleeps.push(ms);
            self.now += ms;
        }
    }

    #[test]
    fn activity_classifies_busy_idle_and_stuck_like_maw_js_shape() {
        let busy = classify_activity_snapshots(
            "s:1",
            &[
                ActivitySample { text: "hello".to_owned(), at_ms: 1_000 },
                ActivitySample { text: "hello world".to_owned(), at_ms: 2_000 },
                ActivitySample { text: "hello world".to_owned(), at_ms: 3_000 },
            ],
            30_000,
        );
        assert_eq!(busy.state, ActivityState::Busy);
        assert_eq!(busy.confidence, ActivityConfidence::High);
        assert_eq!(busy.diff_samples, 2);
        assert_eq!(format_activity_human(&busy), "s:1: 🟢 BUSY (last change 1s ago, 2/3 samples diff)");

        let idle = classify_activity_snapshots(
            "s:1",
            &[
                ActivitySample { text: "working".to_owned(), at_ms: 1_000 },
                ActivitySample { text: "working".to_owned(), at_ms: 2_000 },
            ],
            2_000,
        );
        assert_eq!(idle.state, ActivityState::Idle);
        assert_eq!(idle.confidence, ActivityConfidence::Medium);

        let stuck = classify_activity_snapshots(
            "s:1",
            &[
                ActivitySample { text: "> ▌".to_owned(), at_ms: 1_000 },
                ActivitySample { text: "> ▌".to_owned(), at_ms: 2_000 },
            ],
            2_000,
        );
        assert_eq!(stuck.state, ActivityState::Stuck);
    }

    #[test]
    fn activity_json_and_watch_single_shot_are_offline() {
        let opts = ActivityOptions {
            all: false,
            watch: true,
            json: false,
            stuck_only: false,
            window: Some("2s".to_owned()),
            samples: Some(2),
            sampler: Some("peek".to_owned()),
            watch_iterations: Some(1),
        };
        let mut tmux = FakeTmux {
            captures: vec!["old".to_owned(), "new".to_owned()],
            ..FakeTmux::default()
        };
        let mut clock = FakeClock { now: 0, sleeps: Vec::new() };
        let output = cmd_activity(Some("agent:main"), &opts, &mut tmux, &mut clock).expect("activity");
        assert!(output.stdout.contains("activity: watching agent:main"));
        assert!(output.stdout.contains("agent:main: 🟢 BUSY"));
        assert_eq!(clock.sleeps, vec![2_000]);
        assert_eq!(tmux.seen_targets, vec!["agent:main", "agent:main"]);
    }

    #[test]
    fn activity_all_resolves_fleet_window_names_to_numeric_tmux_targets() {
        let fleet = vec![NativeFleetSession {
            name: "s".to_owned(),
            windows: vec![NativeFleetWindow { name: "main".to_owned(), repo: String::new() }],
            ..NativeFleetSession::default()
        }];
        assert_eq!(all_activity_targets(&fleet), vec!["s:main".to_owned()]);
        let sessions = vec![TmuxSession {
            name: "s".to_owned(),
            windows: vec![maw_tmux::TmuxWindow { index: 2, name: "main".to_owned(), active: true, cwd: None }],
        }];
        assert_eq!(resolve_activity_peek_target(&sessions, "s:main"), Some("s:2".to_owned()));
        assert_eq!(resolve_activity_peek_target(&sessions, "s:main.1"), Some("s:2.1".to_owned()));
    }

    #[test]
    fn activity_parser_and_target_guard_match_plugin_contract() {
        assert!(parse_activity_cli(&["--all".to_owned(), "pane".to_owned()]).is_err());
        assert!(parse_activity_cli(&["--samples=1".to_owned(), "pane".to_owned()]).is_ok());
        let (_, opts) = parse_activity_cli(&[
            "pane".to_owned(),
            "--json".to_owned(),
            "--stuck-only".to_owned(),
            "--window=1.5s".to_owned(),
            "--samples=2".to_owned(),
            "--sampler=follow".to_owned(),
        ])
        .expect("parse");
        let parsed = parse_activity_options(&opts).expect("options");
        assert_eq!(parsed.window_ms, 1_500);
        assert_eq!(parsed.sampler, ActivitySampler::Follow);
        assert!(validate_activity_tmux_target("-pane").is_err());
        assert!(validate_activity_tmux_target("123").is_err());
        assert!(validate_activity_tmux_target("s:1.0").is_ok());
        assert!(validate_activity_tmux_target("%42").is_ok());
    }

    #[test]
    fn activity_json_matches_committed_golden_shape_without_ref_checkout() {
        let opts = ActivityOptions {
            all: false,
            watch: false,
            json: true,
            stuck_only: false,
            window: Some("2s".to_owned()),
            samples: Some(2),
            sampler: None,
            watch_iterations: None,
        };
        let mut tmux = FakeTmux {
            captures: vec!["ready".to_owned(), "ready".to_owned()],
            ..FakeTmux::default()
        };
        let mut clock = FakeClock { now: 0, sleeps: Vec::new() };
        let _guard = env_test_lock().lock().unwrap_or_else(|e| e.into_inner());
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let output = cmd_activity(Some("s:main"), &opts, &mut tmux, &mut clock).expect("activity");
        assert_eq!(
            output.stdout,
            "{\"pane\":\"s:main\",\"state\":\"idle\",\"confidence\":\"medium\",\"samples\":2,\"diff_samples\":0,\"last_change_ago_seconds\":2,\"sample_window_seconds\":2}\n"
        );
    }
}
