// Watching a pane until the engine is really up.
//
// Sending the launch line is not the same as the agent running: the shell may
// still be initialising, or the engine may stop on a trust prompt waiting for a
// keypress nobody is there to give. These poll the pane's command and visible
// screen to tell those apart, so wake reports started only when it started.

/// Bounded backoff schedule (ms) between launch-confirmation polls — ~2.5s total.
const WAKE_LAUNCH_CONFIRM_BACKOFF_MS: &[u64] = &[50, 100, 200, 300, 400, 500, 500, 450];

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
    let mut observed = None;
    let mut delays = WAKE_LAUNCH_CONFIRM_BACKOFF_MS.iter().copied();
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
    observed.map_or(Ok(()), |observed| {
        Err(format!("wake: engine did not start in {target} (pane still running '{observed}') — sent: {command}"))
    })
}

fn wake_wait_for_shell_ready(tmux: &mut impl WakeTmuxNative, target: &str) {
    let mut delays = WAKE_LAUNCH_CONFIRM_BACKOFF_MS.iter().copied();
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
