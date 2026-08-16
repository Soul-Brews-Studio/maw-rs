// Watching a pane until the engine is really up.
//
// Sending the launch line is not the same as the agent running: the shell may
// still be initialising, or the engine may stop on a trust prompt waiting for a
// keypress nobody is there to give. These poll the pane's command and visible
// screen to tell those apart, so wake reports started only when it started.

/// Bounded backoff schedule (ms) between launch-confirmation polls — ~2.5s total.
///
/// The built-in default. Under load the engine can need much longer than this
/// (#751: false "engine did not start" at load 26–40 while the engine was up
/// seconds later), so the real schedule comes from
/// [`wake_launch_confirm_backoff`], which rebuilds this exact shape for the
/// default budget and stretches it when the budget is raised. Kept as the
/// regression anchor the generator is asserted against, so a refactor cannot
/// silently change default wake timing — hence test-only.
#[cfg(test)]
const WAKE_LAUNCH_CONFIRM_BACKOFF_MS: &[u64] = &[50, 100, 200, 300, 400, 500, 500, 450];

/// Total launch-confirmation poll budget (ms) — the sum of the default schedule.
const WAKE_LAUNCH_CONFIRM_BUDGET_MS: u64 = 2_500;

/// Longest single gap in the ramp; the budget is spent in steps of this once the
/// ramp is exhausted, so a raised budget polls steadily instead of sleeping long.
const WAKE_LAUNCH_CONFIRM_STEP_MS: u64 = 500;

/// Ramp the schedule climbs before settling into [`WAKE_LAUNCH_CONFIRM_STEP_MS`] steps.
const WAKE_LAUNCH_CONFIRM_RAMP_MS: &[u64] = &[50, 100, 200, 300, 400];

/// Grace re-check (ms) after the budget is spent, before reporting failure (#751).
///
/// The reported failure was often a poll landing at one bad moment on a busy
/// machine; crew-lab found that asking again immediately reproduces the same
/// false negative, so one deliberately spaced extra look is the cheap fix.
/// Set to `0` to disable the grace poll entirely.
const WAKE_LAUNCH_CONFIRM_GRACE_MS: u64 = 500;

/// Env override for the total poll budget (ms).
const WAKE_LAUNCH_CONFIRM_BUDGET_ENV: &str = "MAW_RS_WAKE_CONFIRM_BUDGET_MS";

/// Env override for the post-budget grace re-check (ms); `0` disables it.
const WAKE_LAUNCH_CONFIRM_GRACE_ENV: &str = "MAW_RS_WAKE_CONFIRM_GRACE_MS";

/// Resolve a wake confirmation timing knob: env, then `wake.<key>` in the merged
/// config, then the built-in default. Garbage parses fall back rather than fail —
/// a typo in config must not make wake unusable.
fn wake_launch_confirm_ms(env_key: &str, config_key: &str, default_ms: u64) -> u64 {
    if let Some(value) = std::env::var(env_key).ok().and_then(|raw| raw.trim().parse::<u64>().ok()) {
        return value;
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    merged_config_value_in_dir(&cwd)
        .get("wake")
        .and_then(|wake| wake.get(config_key))
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(default_ms)
}

/// Build the launch-confirmation backoff schedule for the configured budget.
///
/// At the default budget this reproduces [`WAKE_LAUNCH_CONFIRM_BACKOFF_MS`]
/// exactly, so existing behavior is unchanged unless an operator opts in.
fn wake_launch_confirm_backoff() -> Vec<u64> {
    let budget = wake_launch_confirm_ms(WAKE_LAUNCH_CONFIRM_BUDGET_ENV, "confirmBudgetMs", WAKE_LAUNCH_CONFIRM_BUDGET_MS);
    let mut schedule = Vec::new();
    let mut spent = 0;
    for delay in WAKE_LAUNCH_CONFIRM_RAMP_MS.iter().copied() {
        if spent + delay > budget { break; }
        schedule.push(delay);
        spent += delay;
    }
    while spent < budget {
        let delay = WAKE_LAUNCH_CONFIRM_STEP_MS.min(budget - spent);
        schedule.push(delay);
        spent += delay;
    }
    schedule
}

/// True when a tmux `pane_current_command` still looks like an interactive shell.
///
/// Deliberately detects "pane has NOT left the shell" instead of matching
/// engine process names: a running claude engine can report a bare version
/// string like `2.1.207` rather than `claude` (#520), so engine-name
/// predicates silently break.
fn wake_pane_command_is_shell(command: &str) -> bool {
    let name = command.trim().trim_start_matches('-');
    let name = name.rsplit('/').next().unwrap_or(name);
    matches!(name, "" | "sh" | "bash" | "zsh" | "fish" | "dash" | "ash" | "ksh" | "tcsh" | "csh" | "nu" | "pwsh")
}

/// Engine first-run directory-trust dialog markers (#616).
///
/// Case-sensitive substrings of the interactive trust prompts the engines
/// show on their first run in an untrusted directory (codex-family and
/// claude-family respectively). Bypass flags like
/// `--dangerously-bypass-approvals-and-sandbox` skip *approvals*, not this
/// first-run trust gate, so a headless wake can hang on it forever.
const WAKE_TRUST_PROMPT_MARKERS: &[&str] = &[
    "Do you trust the contents of this directory",
    "Do you trust the files in this folder",
];

/// Extra trust-prompt captures after the immediate one (#616): the prompt can
/// render slightly after the engine process appears, so re-capture a couple of
/// times before declaring the pane healthy.
const WAKE_TRUST_PROMPT_SETTLE_POLLS: usize = 2;

/// Delay (ms) between trust-prompt settle captures — with
/// [`WAKE_TRUST_PROMPT_SETTLE_POLLS`] this bounds the latency added to a
/// healthy wake at ~400ms.
const WAKE_TRUST_PROMPT_SETTLE_MS: u64 = 200;

/// True when a captured pane screen shows an engine directory-trust dialog.
fn wake_pane_capture_shows_trust_prompt(screen: &str) -> bool {
    WAKE_TRUST_PROMPT_MARKERS.iter().any(|marker| screen.contains(marker))
}

/// Fail fast when the launched engine is stuck at the directory-trust prompt (#616).
///
/// Called only after the pane has left the shell (the engine process IS
/// running). Captures the visible screen, re-capturing over a short settle
/// window because the prompt can render after the process starts. The pane is
/// deliberately left untouched — a human can still attach and answer; this
/// only changes what wake REPORTS. An unreadable capture keeps the legacy
/// success, mirroring the #580 principle of never failing a healthy wake on a
/// readback error.
fn wake_confirm_no_trust_prompt(tmux: &mut impl WakeTmuxNative, target: &str, command: &str) -> Result<(), String> {
    for attempt in 0..=WAKE_TRUST_PROMPT_SETTLE_POLLS {
        let Ok(screen) = tmux.wake_pane_capture(target) else { return Ok(()) };
        if wake_pane_capture_shows_trust_prompt(&screen) {
            let session = target.split(':').next().unwrap_or(target);
            return Err(format!(
                "wake: engine is stuck at the directory-trust prompt in {target} — attach (maw a {session}) and answer once, or pre-seed trust — sent: {command}"
            ));
        }
        if attempt < WAKE_TRUST_PROMPT_SETTLE_POLLS {
            tmux.wake_confirm_poll_sleep(std::time::Duration::from_millis(WAKE_TRUST_PROMPT_SETTLE_MS));
        }
    }
    Ok(())
}

/// Confirm the sent launch command actually left the shell (#580).
///
/// Polls `pane_current_command` with a bounded backoff (~2.5s total), exiting
/// as soon as the pane runs something that is not a shell. If the pane still
/// runs a shell after the poll budget, the launch is reported as failed. If
/// pane state was never readable, the legacy fire-and-forget behavior is kept
/// rather than failing an otherwise healthy wake on a readback error.
///
/// Leaving the shell is not enough (#616): an engine stuck at its first-run
/// directory-trust prompt IS running, so the screen is additionally checked
/// for trust-prompt markers before reporting success.
fn wake_confirm_engine_launch(tmux: &mut impl WakeTmuxNative, target: &str, command: &str) -> Result<(), String> {
    let backoff = wake_launch_confirm_backoff();
    let grace_ms = wake_launch_confirm_ms(WAKE_LAUNCH_CONFIRM_GRACE_ENV, "confirmGraceMs", WAKE_LAUNCH_CONFIRM_GRACE_MS);
    let mut observed = None;
    // The grace poll is appended to the schedule so the budget loop and the extra
    // look share one code path: the last sleep is simply spaced further out.
    let mut delays = backoff.iter().copied().chain(std::iter::once(grace_ms).filter(|ms| *ms > 0));
    loop {
        if let Ok(current) = tmux.wake_pane_current_command(target) {
            if !wake_pane_command_is_shell(&current) {
                return wake_confirm_no_trust_prompt(tmux, target, command);
            }
            observed = Some(current);
        }
        let Some(delay_ms) = delays.next() else { break };
        tmux.wake_confirm_poll_sleep(std::time::Duration::from_millis(delay_ms));
    }
    let waited_ms = backoff.iter().sum::<u64>() + grace_ms;
    observed.map_or(Ok(()), |observed| {
        // #751: callers act on this message. Say the engine has not started YET —
        // a pane still in the shell after the budget may still be booting under
        // load, and treating that as "will not come up" tears down healthy teams.
        Err(format!(
            "wake: engine has not started yet in {target} after {waited_ms}ms (pane still running '{observed}') — it may still be booting under load; re-check the pane, or raise {WAKE_LAUNCH_CONFIRM_BUDGET_ENV} / wake.confirmBudgetMs — sent: {command}"
        ))
    })
}

fn wake_wait_for_shell_ready(tmux: &mut impl WakeTmuxNative, target: &str) {
    let backoff = wake_launch_confirm_backoff();
    let mut delays = backoff.iter().copied();
    loop {
        match tmux.wake_pane_current_command(target) {
            Ok(current) if wake_pane_command_is_shell(&current) => return,
            Ok(_) => {}
            Err(_) => return,
        }
        let Some(delay_ms) = delays.next() else { return };
        tmux.wake_confirm_poll_sleep(std::time::Duration::from_millis(delay_ms));
    }
}

fn wake_target_is_current_pane(tmux: &mut impl WakeTmuxNative, target: &str) -> bool {
    let Ok(current_pane) = std::env::var("TMUX_PANE") else { return false };
    let current_pane = current_pane.trim();
    if current_pane.is_empty() { return false; }
    tmux.wake_target_pane_id(target).is_ok_and(|target_pane| target_pane.trim() == current_pane)
}

#[cfg(test)]
mod wake_launch_timing_tests751 {
    use super::*;

    fn with_budget_env<T>(budget: Option<&str>, grace: Option<&str>, body: impl FnOnce() -> T) -> T {
        let _guard = env_test_lock();
        let _budget = EnvVarRestore::capture(WAKE_LAUNCH_CONFIRM_BUDGET_ENV);
        let _grace = EnvVarRestore::capture(WAKE_LAUNCH_CONFIRM_GRACE_ENV);
        // HOME is redirected so the merged-config layer cannot leak a real
        // wake.confirmBudgetMs from the developer's machine into the assertion.
        let _home = EnvVarRestore::capture("HOME");
        std::env::set_var("HOME", std::env::temp_dir().join(format!("maw-rs-wake-751-{}", std::process::id())));
        match budget {
            Some(value) => std::env::set_var(WAKE_LAUNCH_CONFIRM_BUDGET_ENV, value),
            None => std::env::remove_var(WAKE_LAUNCH_CONFIRM_BUDGET_ENV),
        }
        match grace {
            Some(value) => std::env::set_var(WAKE_LAUNCH_CONFIRM_GRACE_ENV, value),
            None => std::env::remove_var(WAKE_LAUNCH_CONFIRM_GRACE_ENV),
        }
        body()
    }

    /// The knob must be opt-in: with nothing configured the generated schedule is
    /// byte-for-byte the historical one, so #751 changes no default timing.
    #[test]
    fn default_budget_reproduces_the_historical_schedule_exactly() {
        with_budget_env(None, None, || {
            assert_eq!(wake_launch_confirm_backoff(), WAKE_LAUNCH_CONFIRM_BACKOFF_MS.to_vec());
            assert_eq!(wake_launch_confirm_backoff().iter().sum::<u64>(), WAKE_LAUNCH_CONFIRM_BUDGET_MS);
        });
    }

    /// The whole point of #751: a busy machine can buy more wait without a rebuild.
    #[test]
    fn raised_budget_keeps_the_ramp_then_polls_in_steady_steps() {
        with_budget_env(Some("10000"), None, || {
            let schedule = wake_launch_confirm_backoff();
            assert_eq!(schedule.iter().sum::<u64>(), 10_000, "budget must be spent exactly: {schedule:?}");
            assert_eq!(&schedule[..WAKE_LAUNCH_CONFIRM_RAMP_MS.len()], WAKE_LAUNCH_CONFIRM_RAMP_MS, "ramp must survive");
            assert!(schedule[WAKE_LAUNCH_CONFIRM_RAMP_MS.len()..].iter().all(|ms| *ms <= WAKE_LAUNCH_CONFIRM_STEP_MS),
                "a raised budget must keep polling, not sleep in one long gap: {schedule:?}");
            assert!(schedule.len() > WAKE_LAUNCH_CONFIRM_BACKOFF_MS.len(), "a bigger budget must mean more looks: {schedule:?}");
        });
    }

    /// A budget below the ramp must still poll — never collapse to zero looks.
    #[test]
    fn tiny_and_garbage_budgets_stay_usable() {
        with_budget_env(Some("120"), None, || {
            let schedule = wake_launch_confirm_backoff();
            assert_eq!(schedule.iter().sum::<u64>(), 120, "{schedule:?}");
            assert!(!schedule.is_empty(), "a small budget must still poll");
        });
        with_budget_env(Some("not-a-number"), None, || {
            assert_eq!(wake_launch_confirm_backoff(), WAKE_LAUNCH_CONFIRM_BACKOFF_MS.to_vec(), "garbage falls back, never panics");
        });
        with_budget_env(Some("0"), None, || {
            assert!(wake_launch_confirm_backoff().is_empty(), "a zero budget means no waiting, not a hang");
        });
    }

    /// Grace re-check is on by default and can be turned off with `0`.
    #[test]
    fn grace_recheck_defaults_on_and_is_disableable() {
        with_budget_env(None, None, || {
            assert_eq!(wake_launch_confirm_ms(WAKE_LAUNCH_CONFIRM_GRACE_ENV, "confirmGraceMs", WAKE_LAUNCH_CONFIRM_GRACE_MS), WAKE_LAUNCH_CONFIRM_GRACE_MS);
        });
        with_budget_env(None, Some("0"), || {
            assert_eq!(wake_launch_confirm_ms(WAKE_LAUNCH_CONFIRM_GRACE_ENV, "confirmGraceMs", WAKE_LAUNCH_CONFIRM_GRACE_MS), 0);
        });
        with_budget_env(None, Some("3000"), || {
            assert_eq!(wake_launch_confirm_ms(WAKE_LAUNCH_CONFIRM_GRACE_ENV, "confirmGraceMs", WAKE_LAUNCH_CONFIRM_GRACE_MS), 3_000);
        });
    }
}
