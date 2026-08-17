use maw_tmux::{CommandTmuxRunner, TmuxClient, TmuxRunner, TmuxSession};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    hash::{Hash, Hasher},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const AGENTSTATUS_POLL_INTERVAL_ENV: &str = "MAW_WS_STATUS_INTERVAL_MS";
const AGENTSTATUS_POLL_INTERVAL_DEFAULT_MS: u64 = 3_000;
const AGENTSTATUS_POLL_INTERVAL_MIN_MS: u64 = 250;
const AGENTSTATUS_POLL_INTERVAL_MAX_MS: u64 = 30_000;
const AGENTSTATUS_READY_MS: u64 = 15_000;
const AGENTSTATUS_BUSY_HEARTBEAT_MS: u64 = 30_000;
const AGENTSTATUS_REAL_FEED_TTL_MS: u64 = 60_000;
const AGENTSTATUS_FEED_HISTORY_MAX_AGE_MS: u64 = 60_000;
const AGENTSTATUS_REAL_FEED_PRUNE_MS: u64 = 3_600_000;
const AGENTSTATUS_FEED_CAP: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AgentStatusKind {
    Busy,
    Ready,
    Idle,
    Crashed,
}

impl AgentStatusKind {
    fn agentstatus_as_str(self) -> &'static str {
        match self {
            Self::Busy => "busy",
            Self::Ready => "ready",
            Self::Idle => "idle",
            Self::Crashed => "crashed",
        }
    }

    fn agentstatus_event(self) -> &'static str {
        match self {
            Self::Busy => "PreToolUse",
            Self::Ready => "Stop",
            Self::Idle => "SessionEnd",
            Self::Crashed => "Error",
        }
    }

    fn agentstatus_message(self) -> &'static str {
        match self {
            Self::Busy => "working",
            Self::Ready => "waiting",
            Self::Idle => "idle",
            Self::Crashed => "crashed",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentStatusSnapshot {
    statuses: BTreeMap<String, AgentStatusKind>,
    agent_targets: BTreeSet<String>,
}

impl AgentStatusSnapshot {
    pub(crate) fn agentstatus_status(&self, target: &str) -> Option<&'static str> {
        self.statuses
            .get(target)
            .map(|status| status.agentstatus_as_str())
    }

    pub(crate) fn agentstatus_is_agent_target(&self, target: &str) -> bool {
        self.agent_targets.contains(target)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AgentStatusSession {
    name: String,
    windows: Vec<AgentStatusWindow>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentStatusWindow {
    index: u32,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AgentStatusFeedEvent {
    timestamp: String,
    oracle: String,
    host: String,
    event: String,
    project: String,
    #[serde(rename = "sessionId")]
    session_id: String,
    message: String,
    ts: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentStatusFeedEntry {
    seq: u64,
    event: AgentStatusFeedEvent,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentStatusFeedRing {
    next_seq: u64,
    entries: VecDeque<AgentStatusFeedEntry>,
}

impl AgentStatusFeedRing {
    pub(crate) fn agentstatus_push(&mut self, event: AgentStatusFeedEvent) {
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push_back(AgentStatusFeedEntry {
            seq: self.next_seq,
            event,
        });
        while self.entries.len() > AGENTSTATUS_FEED_CAP {
            self.entries.pop_front();
        }
    }

    #[cfg(test)]
    pub(crate) fn agentstatus_history(&self) -> Vec<AgentStatusFeedEvent> {
        self.entries
            .iter()
            .map(|entry| entry.event.clone())
            .collect()
    }

    pub(crate) fn agentstatus_history_since(
        &self,
        now_ms: u64,
        max_age_ms: u64,
    ) -> Vec<AgentStatusFeedEvent> {
        self.entries
            .iter()
            .filter(|entry| now_ms.saturating_sub(entry.event.ts) <= max_age_ms)
            .map(|entry| entry.event.clone())
            .collect()
    }

    pub(crate) fn agentstatus_cursor(&self) -> u64 {
        self.next_seq
    }

    pub(crate) fn agentstatus_drain_after(&self, cursor: u64) -> (Vec<AgentStatusFeedEvent>, u64) {
        let events = self
            .entries
            .iter()
            .filter(|entry| entry.seq > cursor)
            .map(|entry| entry.event.clone())
            .collect();
        (events, self.next_seq)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct AgentStatusState {
    hash: String,
    changed_at: u64,
    status: AgentStatusKind,
    was_running: bool,
    emitted_at: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct AgentStatusDetector {
    state: BTreeMap<String, AgentStatusState>,
    real_feed_last_seen: BTreeMap<String, u64>,
}

pub(crate) trait AgentStatusSource {
    fn agentstatus_pane_commands(&mut self) -> BTreeMap<String, String>;
    fn agentstatus_capture(&mut self, target: &str) -> Option<String>;
}

#[derive(Default)]
struct AgentStatusGlobal {
    detector: AgentStatusDetector,
    feed: AgentStatusFeedRing,
    last_sweep_ms: Option<u64>,
    snapshot: AgentStatusSnapshot,
}

struct AgentStatusTmuxSource;

impl AgentStatusSource for AgentStatusTmuxSource {
    fn agentstatus_pane_commands(&mut self) -> BTreeMap<String, String> {
        let mut runner = CommandTmuxRunner::new();
        let args = vec![
            "-a".to_owned(),
            "-F".to_owned(),
            "#{session_name}:#{window_index}|||#{pane_current_command}".to_owned(),
        ];
        let Ok(raw) = runner.run("list-panes", &args) else {
            return BTreeMap::new();
        };
        agentstatus_parse_pane_commands(&raw)
    }

    fn agentstatus_capture(&mut self, target: &str) -> Option<String> {
        let mut tmux = TmuxClient::local();
        let content = tmux.capture(target, Some(20)).ok()?;
        Some(agentstatus_last_lines(&content, 20))
    }
}

static AGENTSTATUS_GLOBAL: OnceLock<Mutex<AgentStatusGlobal>> = OnceLock::new();

pub(crate) fn agentstatus_sessions_from_tmux(sessions: &[TmuxSession]) -> Vec<AgentStatusSession> {
    sessions
        .iter()
        .map(|session| AgentStatusSession {
            name: session.name.clone(),
            windows: session
                .windows
                .iter()
                .map(|window| AgentStatusWindow {
                    index: window.index,
                    name: window.name.clone(),
                })
                .collect(),
        })
        .collect()
}

pub(crate) fn agentstatus_poll_global(sessions: &[AgentStatusSession]) -> AgentStatusSnapshot {
    let now_ms = agentstatus_now_millis();
    let poll_interval_ms = agentstatus_poll_interval_ms();
    let configured_bins = agentstatus_configured_bins();
    let lock = AGENTSTATUS_GLOBAL.get_or_init(|| Mutex::new(AgentStatusGlobal::default()));
    let mut global = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if global
        .last_sweep_ms
        .is_some_and(|last| now_ms.saturating_sub(last) < poll_interval_ms)
        && agentstatus_snapshot_matches_sessions(&global.snapshot, sessions)
    {
        return global.snapshot.clone();
    }
    let mut source = AgentStatusTmuxSource;
    let mut feed = std::mem::take(&mut global.feed);
    let snapshot = global.detector.agentstatus_detect(
        sessions,
        &mut source,
        &configured_bins,
        now_ms,
        &mut feed,
    );
    global.feed = feed;
    global.last_sweep_ms = Some(now_ms);
    global.snapshot = snapshot.clone();
    snapshot
}

fn agentstatus_snapshot_matches_sessions(
    snapshot: &AgentStatusSnapshot,
    sessions: &[AgentStatusSession],
) -> bool {
    // #889: the throttle may reuse status values, but never for a different
    // session/window set. The UI combines this snapshot with the fresh
    // `sessions` argument, so a shape mismatch would serialize null statuses.
    let expected_len = sessions
        .iter()
        .map(|session| session.windows.len())
        .sum::<usize>();
    expected_len == snapshot.statuses.len()
        && sessions.iter().all(|session| {
            session.windows.iter().all(|window| {
                snapshot
                    .statuses
                    .contains_key(&agentstatus_target(&session.name, window.index))
            })
        })
}

pub(crate) fn agentstatus_feed_history_and_cursor() -> (Vec<AgentStatusFeedEvent>, u64) {
    let now_ms = agentstatus_now_millis();
    agentstatus_feed_history_and_cursor_at(now_ms)
}

fn agentstatus_feed_history_and_cursor_at(now_ms: u64) -> (Vec<AgentStatusFeedEvent>, u64) {
    let lock = AGENTSTATUS_GLOBAL.get_or_init(|| Mutex::new(AgentStatusGlobal::default()));
    let global = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    (
        global
            .feed
            .agentstatus_history_since(now_ms, AGENTSTATUS_FEED_HISTORY_MAX_AGE_MS),
        global.feed.agentstatus_cursor(),
    )
}

pub(crate) fn agentstatus_drain_feed(cursor: u64) -> (Vec<AgentStatusFeedEvent>, u64) {
    let lock = AGENTSTATUS_GLOBAL.get_or_init(|| Mutex::new(AgentStatusGlobal::default()));
    let global = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    global.feed.agentstatus_drain_after(cursor)
}

pub(crate) fn agentstatus_mark_real_feed_event(oracle: &str) {
    let now_ms = agentstatus_now_millis();
    let lock = AGENTSTATUS_GLOBAL.get_or_init(|| Mutex::new(AgentStatusGlobal::default()));
    let mut global = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    global
        .detector
        .agentstatus_mark_real_feed_event(oracle, now_ms);
}

#[cfg(test)]
pub(crate) fn agentstatus_reset_global() {
    let lock = AGENTSTATUS_GLOBAL.get_or_init(|| Mutex::new(AgentStatusGlobal::default()));
    *lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = AgentStatusGlobal::default();
}

pub(crate) fn agentstatus_oracle_from_feed_payload(payload: &[u8]) -> Option<String> {
    let value = serde_json::from_slice::<Value>(payload).ok()?;
    value
        .get("oracle")
        .and_then(Value::as_str)
        .filter(|oracle| !oracle.trim().is_empty())
        .map(str::to_owned)
}

/// #813: the hardcoded arms (claude / codex / version shape / exact `node`)
/// moved into the shared `maw_tmux::is_agent_pane_command` — this file was one
/// of only two that already had the version arm, and its argv0-exact rule is
/// the one the shared predicate adopted for every generic launcher name. The
/// per-deployment `configured_bins` arm is genuinely local and stays here.
pub(crate) fn agentstatus_is_agent_command(cmd: &str, configured_bins: &[String]) -> bool {
    let command = cmd.trim();
    if command.is_empty() {
        return false;
    }
    let lower = command.to_ascii_lowercase();
    if maw_tmux::is_agent_pane_command(Some(command)) {
        return true;
    }
    configured_bins.iter().any(|bin| {
        bin.split_whitespace()
            .next()
            .is_some_and(|first| first != "default" && lower == first.to_ascii_lowercase())
    })
}

pub(crate) fn agentstatus_strip_status_bar(content: &str) -> String {
    content
        .split('\n')
        .map(agentstatus_strip_status_bar_line)
        .filter(|plain| !agentstatus_drop_status_line(plain))
        .collect::<Vec<_>>()
        .join("\n")
}

impl AgentStatusDetector {
    pub(crate) fn agentstatus_detect(
        &mut self,
        sessions: &[AgentStatusSession],
        source: &mut dyn AgentStatusSource,
        configured_bins: &[String],
        now_ms: u64,
        feed: &mut AgentStatusFeedRing,
    ) -> AgentStatusSnapshot {
        let mut snapshot = AgentStatusSnapshot::default();
        let commands = source.agentstatus_pane_commands();
        for (target, name, session) in agentstatus_targets(sessions) {
            let command = commands.get(&target).map_or("", String::as_str);
            let command_lower = command.trim().to_ascii_lowercase();
            let is_agent = agentstatus_is_agent_command(command, configured_bins);
            let is_shell = matches!(command_lower.as_str(), "zsh" | "bash" | "sh" | "fish");
            let content = source.agentstatus_capture(&target).unwrap_or_default();
            let hash = agentstatus_hash(&agentstatus_strip_status_bar(&content));
            let prev = self.state.get(&target);
            let first_or_changed = prev.is_none_or(|state| state.hash != hash);
            let status = if !is_agent && is_shell && prev.is_some_and(|state| state.was_running) {
                AgentStatusKind::Crashed
            } else if !is_agent {
                AgentStatusKind::Idle
            } else if first_or_changed
                || now_ms.saturating_sub(prev.map_or(now_ms, |state| state.changed_at))
                    < AGENTSTATUS_READY_MS
            {
                AgentStatusKind::Busy
            } else {
                AgentStatusKind::Ready
            };
            let changed_at = if first_or_changed {
                now_ms
            } else {
                prev.map_or(now_ms, |state| state.changed_at)
            };
            let was_running = is_agent || prev.is_some_and(|state| state.was_running);
            let emitted_at = prev.and_then(|state| state.emitted_at);
            let mut entry = AgentStatusState {
                hash,
                changed_at,
                status,
                was_running,
                emitted_at,
            };
            let transitioned = prev.is_some_and(|state| state.status != status);
            let heartbeat = status == AgentStatusKind::Busy
                && now_ms.saturating_sub(entry.emitted_at.unwrap_or_default())
                    > AGENTSTATUS_BUSY_HEARTBEAT_MS;
            if (transitioned || heartbeat)
                && !self.agentstatus_has_recent_real_feed(&name, sessions, now_ms)
            {
                entry.emitted_at = Some(now_ms);
                feed.agentstatus_push(agentstatus_feed_event(&name, &session, status, now_ms));
            }
            self.state.insert(target.clone(), entry);
            snapshot.statuses.insert(target.clone(), status);
            if is_agent {
                snapshot.agent_targets.insert(target);
            }
        }
        self.agentstatus_prune(sessions, now_ms);
        snapshot
    }

    pub(crate) fn agentstatus_mark_real_feed_event(&mut self, oracle: &str, now_ms: u64) {
        self.real_feed_last_seen.insert(oracle.to_owned(), now_ms);
    }

    fn agentstatus_has_recent_real_feed(
        &self,
        window_name: &str,
        _sessions: &[AgentStatusSession],
        now_ms: u64,
    ) -> bool {
        let oracle = agentstatus_oracle_name(window_name);
        self.real_feed_last_seen
            .get(&oracle)
            .is_some_and(|last| now_ms.saturating_sub(*last) < AGENTSTATUS_REAL_FEED_TTL_MS)
    }

    fn agentstatus_prune(&mut self, sessions: &[AgentStatusSession], now_ms: u64) {
        let mut active_targets = BTreeSet::new();
        let mut active_oracles = BTreeSet::new();
        for session in sessions {
            for window in &session.windows {
                active_targets.insert(agentstatus_target(&session.name, window.index));
                active_oracles.insert(agentstatus_oracle_name(&window.name));
            }
        }
        self.state
            .retain(|target, _state| active_targets.contains(target));
        self.real_feed_last_seen.retain(|oracle, seen| {
            active_oracles.contains(oracle)
                && now_ms.saturating_sub(*seen) <= AGENTSTATUS_REAL_FEED_PRUNE_MS
        });
    }
}

fn agentstatus_targets(
    sessions: &[AgentStatusSession],
) -> impl Iterator<Item = (String, String, String)> + '_ {
    sessions.iter().flat_map(|session| {
        session.windows.iter().map(|window| {
            (
                agentstatus_target(&session.name, window.index),
                window.name.clone(),
                session.name.clone(),
            )
        })
    })
}

fn agentstatus_target(session: &str, index: u32) -> String {
    format!("{session}:{index}")
}

fn agentstatus_parse_pane_commands(raw: &str) -> BTreeMap<String, String> {
    raw.lines()
        .filter_map(|line| {
            let (target, command) = line.split_once("|||")?;
            Some((target.to_owned(), command.to_owned()))
        })
        .collect()
}

fn agentstatus_last_lines(content: &str, count: usize) -> String {
    let mut lines = content.lines().rev().take(count).collect::<Vec<_>>();
    lines.reverse();
    lines.join("\n")
}

fn agentstatus_configured_bins() -> Vec<String> {
    let config = maw_xdg::load_merged_config(&agentstatus_xdg_env()).config;
    config
        .get("commands")
        .and_then(Value::as_object)
        .map(|commands| {
            commands
                .values()
                .filter_map(|value| value.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn agentstatus_xdg_env() -> maw_xdg::MawXdgEnv {
    let vars = [
        "MAW_HOME",
        "MAW_CONFIG_DIR",
        "MAW_XDG",
        "XDG_CONFIG_HOME",
        "XDG_STATE_HOME",
        "MAW_STATE_DIR",
        "XDG_DATA_HOME",
        "MAW_DATA_DIR",
        "XDG_CACHE_HOME",
        "MAW_CACHE_DIR",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    maw_xdg::MawXdgEnv::with_vars(agentstatus_home_dir(), vars)
}

fn agentstatus_home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
}

fn agentstatus_poll_interval_ms() -> u64 {
    std::env::var(AGENTSTATUS_POLL_INTERVAL_ENV)
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|millis| {
            (AGENTSTATUS_POLL_INTERVAL_MIN_MS..=AGENTSTATUS_POLL_INTERVAL_MAX_MS).contains(millis)
        })
        .unwrap_or(AGENTSTATUS_POLL_INTERVAL_DEFAULT_MS)
}

fn agentstatus_strip_status_bar_line(line: &str) -> String {
    let no_ansi = agentstatus_strip_sgr(line);
    let mut start = no_ansi.len();
    for (index, ch) in no_ansi.char_indices() {
        if !ch.is_whitespace() && !agentstatus_is_spinner(ch) {
            start = index;
            break;
        }
    }
    if start == no_ansi.len() {
        String::new()
    } else {
        no_ansi[start..].to_owned()
    }
}

fn agentstatus_strip_sgr(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            let mut valid = true;
            for code in chars.by_ref() {
                if code == 'm' {
                    valid = false;
                    break;
                }
                if !code.is_ascii_digit() && code != ';' {
                    break;
                }
            }
            if !valid {
                continue;
            }
        }
        out.push(ch);
    }
    out
}

fn agentstatus_is_spinner(ch: char) -> bool {
    matches!(
        ch,
        '●' | '○'
            | '◐'
            | '◑'
            | '◒'
            | '◓'
            | '✶'
            | '✻'
            | '✽'
            | '✢'
            | '✳'
            | '∗'
            | '·'
            | '˙'
            | '⋆'
            | '*'
    ) || ('⠁'..='⣿').contains(&ch)
}

fn agentstatus_drop_status_line(plain: &str) -> bool {
    let lower = plain.to_ascii_lowercase();
    plain
        .chars()
        .all(|ch| ch.is_whitespace() || ch == '─' || ch == '━')
        || plain.contains('📁')
        || plain.contains('📡')
        || plain.contains('⏵')
        || plain.trim() == "❯"
        || agentstatus_contains_current_latest(plain)
        || lower.contains("bypass permissions")
        || lower.contains("auto-accept")
        || plain.trim().is_empty()
}

fn agentstatus_hash(content: &str) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    content.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn agentstatus_contains_current_latest(plain: &str) -> bool {
    let Some(current) = plain.find("current:") else {
        return false;
    };
    plain.find("latest:").is_some_and(|latest| current < latest)
}

fn agentstatus_feed_event(
    window_name: &str,
    session: &str,
    status: AgentStatusKind,
    now_ms: u64,
) -> AgentStatusFeedEvent {
    AgentStatusFeedEvent {
        timestamp: agentstatus_iso_millis(now_ms),
        oracle: agentstatus_oracle_name(window_name),
        host: "local".to_owned(),
        event: status.agentstatus_event().to_owned(),
        project: session.to_owned(),
        session_id: String::new(),
        message: status.agentstatus_message().to_owned(),
        ts: now_ms,
    }
}

fn agentstatus_oracle_name(window_name: &str) -> String {
    window_name
        .strip_suffix("-oracle")
        .unwrap_or(window_name)
        .to_owned()
}

fn agentstatus_now_millis() -> u64 {
    u64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

fn agentstatus_iso_millis(millis: u64) -> String {
    let seconds = millis / 1000;
    let millis_part = millis % 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let (year, month, day) = agentstatus_civil_from_days(days);
    let hour = seconds_of_day / 3600;
    let minute = (seconds_of_day % 3600) / 60;
    let second = seconds_of_day % 60;
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis_part:03}Z")
}

fn agentstatus_civil_from_days(days_since_epoch: i64) -> (i64, u32, u32) {
    let days = days_since_epoch.saturating_add(719_468);
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year_day = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * year_day + 2) / 153;
    let day = year_day - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (
        year,
        u32::try_from(month).unwrap_or(1),
        u32::try_from(day).unwrap_or(1),
    )
}

#[cfg(test)]
pub(crate) fn agentstatus_test_session(
    session: &str,
    windows: &[(&str, u32)],
) -> AgentStatusSession {
    AgentStatusSession {
        name: session.to_owned(),
        windows: windows
            .iter()
            .map(|(name, index)| AgentStatusWindow {
                index: *index,
                name: (*name).to_owned(),
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeSource {
        commands: BTreeMap<String, String>,
        captures: BTreeMap<String, String>,
    }

    impl AgentStatusSource for FakeSource {
        fn agentstatus_pane_commands(&mut self) -> BTreeMap<String, String> {
            self.commands.clone()
        }

        fn agentstatus_capture(&mut self, target: &str) -> Option<String> {
            self.captures.get(target).cloned()
        }
    }

    #[test]
    fn agentstatus_is_agent_command_matches_keywords_versions_and_exact_bins() {
        let configured = vec!["omx --direct".to_owned()];
        for command in ["claude", "codex", "1.2.3", "omx"] {
            assert!(
                agentstatus_is_agent_command(command, &configured),
                "{command}"
            );
        }
        // `node` sits beside `nodemon` deliberately (#813): it reached maw-rs
        // through #770's port of maw-js, where it is a VENDORED substring regex
        // (`/claude|codex|node/i`) that matches `nodemon` too. No launcher in
        // either codebase reports `node` as pane_current_command.
        for command in ["bash", "zsh", "node", "nodemon", ""] {
            assert!(
                !agentstatus_is_agent_command(command, &configured),
                "{command}"
            );
        }
    }

    #[test]
    fn agentstatus_is_agent_command_uses_first_word_from_config_values() {
        let configured = vec!["gemini --foo".to_owned(), "default --bar".to_owned()];

        assert!(agentstatus_is_agent_command("gemini", &configured));
        assert!(!agentstatus_is_agent_command("geminix", &configured));
        assert!(!agentstatus_is_agent_command("default", &configured));
    }

    #[test]
    fn agentstatus_crashed_requires_a_previous_agent_run() {
        let sessions = vec![agentstatus_test_session(
            "142-athena",
            &[("athena-oracle", 1), ("plain-bash", 2)],
        )];
        let mut detector = AgentStatusDetector::default();
        let mut feed = AgentStatusFeedRing::default();
        let mut source = FakeSource::default();
        source
            .commands
            .insert("142-athena:1".to_owned(), "claude".to_owned());
        source
            .commands
            .insert("142-athena:2".to_owned(), "bash".to_owned());
        source
            .captures
            .insert("142-athena:1".to_owned(), "working".to_owned());
        source
            .captures
            .insert("142-athena:2".to_owned(), "$ ".to_owned());
        detector.agentstatus_detect(&sessions, &mut source, &[], 1_000, &mut feed);

        source
            .commands
            .insert("142-athena:1".to_owned(), "bash".to_owned());
        let snapshot = detector.agentstatus_detect(&sessions, &mut source, &[], 2_000, &mut feed);

        assert_eq!(snapshot.agentstatus_status("142-athena:1"), Some("crashed"));
        assert_eq!(snapshot.agentstatus_status("142-athena:2"), Some("idle"));
    }

    #[test]
    fn agentstatus_strip_status_bar_normalizes_blinking_spinner_indentation() {
        let blocked_a = "old\n   ✻  waiting\nreal output";
        let blocked_b = "old\n      waiting\nreal output";
        let changed = "old\n      waiting\nreal output\nnew token";

        assert_eq!(
            agentstatus_hash(&agentstatus_strip_status_bar(blocked_a)),
            agentstatus_hash(&agentstatus_strip_status_bar(blocked_b))
        );
        assert_ne!(
            agentstatus_hash(&agentstatus_strip_status_bar(blocked_b)),
            agentstatus_hash(&agentstatus_strip_status_bar(changed))
        );
    }

    #[test]
    fn agentstatus_feed_ring_keeps_independent_client_cursors() {
        let mut ring = AgentStatusFeedRing::default();
        let cursor_a = ring.agentstatus_cursor();
        let cursor_b = ring.agentstatus_cursor();
        ring.agentstatus_push(agentstatus_feed_event(
            "athena-oracle",
            "142-athena",
            AgentStatusKind::Busy,
            1_704_067_200_123,
        ));

        let (events_a, next_a) = ring.agentstatus_drain_after(cursor_a);
        let (events_b, next_b) = ring.agentstatus_drain_after(cursor_b);

        assert_eq!(events_a.len(), 1);
        assert_eq!(events_b.len(), 1);
        assert_eq!(events_a[0].event, "PreToolUse");
        assert_eq!(events_b[0].message, "working");
        assert_eq!(next_a, next_b);
    }

    #[test]
    fn agentstatus_feed_history_drops_entries_older_than_sixty_seconds() {
        agentstatus_reset_global();
        let lock = AGENTSTATUS_GLOBAL.get_or_init(|| Mutex::new(AgentStatusGlobal::default()));
        let mut global = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        global.feed.agentstatus_push(agentstatus_feed_event(
            "just-outside",
            "142-athena",
            AgentStatusKind::Busy,
            59_999,
        ));
        global.feed.agentstatus_push(agentstatus_feed_event(
            "on-boundary",
            "142-athena",
            AgentStatusKind::Ready,
            60_000,
        ));
        global.feed.agentstatus_push(agentstatus_feed_event(
            "just-inside",
            "142-athena",
            AgentStatusKind::Crashed,
            60_001,
        ));
        drop(global);

        // Feed history keeps events whose age is <= 60_000 ms.
        let (history, _cursor) = agentstatus_feed_history_and_cursor_at(120_000);

        let oracles = history
            .into_iter()
            .map(|event| event.oracle)
            .collect::<Vec<_>>();
        assert_eq!(oracles, vec!["on-boundary", "just-inside"]);
    }

    #[test]
    fn agentstatus_recent_real_feed_suppresses_synthetic_event() {
        let sessions = vec![agentstatus_test_session(
            "142-athena",
            &[("athena-oracle", 1)],
        )];
        let mut detector = AgentStatusDetector::default();
        detector.agentstatus_mark_real_feed_event("athena", 1_000);
        let mut feed = AgentStatusFeedRing::default();
        let mut source = FakeSource::default();
        source
            .commands
            .insert("142-athena:1".to_owned(), "claude".to_owned());
        source
            .captures
            .insert("142-athena:1".to_owned(), "working".to_owned());

        detector.agentstatus_detect(&sessions, &mut source, &[], 60_999, &mut feed);

        assert!(feed.agentstatus_history().is_empty());
    }
}
