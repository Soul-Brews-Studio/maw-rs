//! Portable identity and naming helpers ported from maw-js.
//!
//! This crate mirrors pure functions from maw-js:
//! - `src/core/fleet/session-name.ts`
//! - `src/core/fleet/node-identity.ts`
//! - `src/core/fleet/validate.ts`
//!
//! Behavior is locked by the maw-js portable fixture files and parity tests:
//! - `test/spec/canonical-session-name.fixtures.json`
//! - `test/spec/canonical-node-identity.fixtures.json`
//! - `test/validate-oracle-name.test.ts`

/// Input for [`canonical_session_name`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CanonicalSessionNameInput {
    /// Resolved oracle/repo/window name, with or without `-oracle`.
    pub oracle: String,
    /// Optional numeric fleet slot. When present, returns `NN-<stem>`.
    pub slot: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSessionName {
    /// Optional numeric session prefix, if present.
    pub slot: Option<u32>,
    /// Name stem with any leading numeric prefix removed.
    pub stem: String,
}

/// Error returned when an oracle name is reserved or invalid at user-input
/// boundaries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OracleNameError {
    name: String,
    suggestion: String,
}

impl OracleNameError {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn suggestion(&self) -> &str {
        &self.suggestion
    }
}

impl std::fmt::Display for OracleNameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Oracle name cannot end in '-view' — reserved for ephemeral view sessions. Try '{}' instead.",
            self.suggestion
        )
    }
}

impl std::error::Error for OracleNameError {}

impl CanonicalSessionNameInput {
    #[must_use]
    pub fn new(oracle: impl Into<String>) -> Self {
        Self {
            oracle: oracle.into(),
            slot: None,
        }
    }

    #[must_use]
    pub fn with_slot(oracle: impl Into<String>, slot: u32) -> Self {
        Self {
            oracle: oracle.into(),
            slot: Some(slot),
        }
    }
}

/// Canonical readable tmux session name stem for an oracle.
///
/// This preserves maw-js behavior: sanitize, strip an existing numeric fleet
/// prefix, strip an optional `.git` suffix, then strip a trailing `-oracle`.
///
/// # Errors
///
/// Returns an error when `slot` is outside the maw-js fleet slot range `0..=99`.
pub fn canonical_session_name(input: &CanonicalSessionNameInput) -> Result<String, String> {
    let sanitized = sanitize_session_stem(&input.oracle);
    let without_slot = sanitized.strip_prefix_numeric_fleet_slot();
    let without_git = without_slot.strip_suffix(".git").unwrap_or(without_slot);
    let stem = without_git
        .strip_suffix("-oracle")
        .unwrap_or(without_git)
        .to_owned();

    let Some(slot) = input.slot else {
        return Ok(stem);
    };
    if slot > 99 {
        return Err(format!("invalid fleet slot '{slot}'"));
    }
    Ok(format!("{slot:02}-{stem}"))
}

/// Convenience wrapper for the common no-slot case.
///
/// # Errors
///
/// This wrapper has no fallible inputs, but returns `Result` to mirror
/// [`canonical_session_name`].
pub fn canonical_session_stem(oracle: &str) -> Result<String, String> {
    canonical_session_name(&CanonicalSessionNameInput::new(oracle))
}

/// Parse a canonical session name into optional slot + stem.
///
/// This is the inverse of [`canonical_session_name`]:
/// `"81-kru32"` -> `(slot: Some(81), stem: "kru32")`.
///
/// Degenerate names like `142` pass through as `(slot: None, stem: "142")`.
#[must_use]
pub fn parse_session_name(raw: &str) -> ParsedSessionName {
    let Some((prefix, stem)) = raw.split_once('-') else {
        return ParsedSessionName {
            slot: None,
            stem: raw.to_owned(),
        };
    };
    if prefix.is_empty() || !prefix.chars().all(|ch| ch.is_ascii_digit()) {
        return ParsedSessionName {
            slot: None,
            stem: raw.to_owned(),
        };
    }
    ParsedSessionName {
        slot: prefix.parse().ok(),
        stem: stem.to_owned(),
    }
}

/// Validate oracle names at creation/wake user-input boundaries.
///
/// This intentionally mirrors maw-js `assertValidOracleName`: today it only
/// reserves the `-view` suffix because view sessions use that suffix
/// internally and allowing oracle names like `foo-view` creates ambiguous
/// `foo-view-view` chains.
///
/// # Errors
///
/// Returns [`OracleNameError`] when `name` ends in the reserved `-view` suffix.
pub fn assert_valid_oracle_name(name: &str) -> Result<(), OracleNameError> {
    let Some(suggestion) = name.strip_suffix("-view") else {
        return Ok(());
    };

    Err(OracleNameError {
        name: name.to_owned(),
        suggestion: suggestion.to_owned(),
    })
}

/// Canonical service identity for federation-visible nodes.
#[must_use]
pub fn canonical_node_identity(host: &str, user: Option<&str>) -> String {
    let host = clean(host).unwrap_or("local");
    if host.contains('@') {
        return host.to_owned();
    }
    let Some(user) = user.and_then(clean) else {
        return host.to_owned();
    };
    if user == host {
        return host.to_owned();
    }
    format!("{user}@{host}")
}

fn clean(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

fn sanitize_session_stem(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut out = String::with_capacity(lower.len());
    let mut previous_was_space = false;

    for ch in lower.chars() {
        if ch.is_whitespace() {
            if !previous_was_space {
                out.push('-');
                previous_was_space = true;
            }
            continue;
        }
        previous_was_space = false;
        if ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-') {
            out.push(ch);
        }
    }

    while out.contains("..") {
        out = out.replace("..", ".");
    }

    let trimmed_start = out.trim_start_matches(['-', '.']);
    let trimmed = trim_trailing_dash_dot_run(trimmed_start);
    trimmed.chars().take(50).collect()
}

fn trim_trailing_dash_dot_run(value: &str) -> &str {
    let mut run_start = value.len();
    for (idx, ch) in value.char_indices().rev() {
        if matches!(ch, '-' | '.') {
            run_start = idx;
        } else {
            break;
        }
    }
    if run_start == value.len() {
        return value;
    }
    &value[..run_start]
}

trait StripNumericFleetSlot {
    fn strip_prefix_numeric_fleet_slot(&self) -> &str;
}

impl StripNumericFleetSlot for str {
    fn strip_prefix_numeric_fleet_slot(&self) -> &str {
        let Some((prefix, rest)) = self.split_once('-') else {
            return self;
        };
        if !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()) {
            rest
        } else {
            self
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numeric_fleet_slot_helper_leaves_non_numeric_prefixes_alone() {
        assert_eq!("mawjs".strip_prefix_numeric_fleet_slot(), "mawjs");
        assert_eq!("dev-mawjs".strip_prefix_numeric_fleet_slot(), "dev-mawjs");
        assert_eq!("47-mawjs".strip_prefix_numeric_fleet_slot(), "mawjs");
    }

    #[test]
    fn trims_trailing_dash_dot_runs() {
        assert_eq!(sanitize_session_stem(" foo-- "), "foo");
        assert_eq!(sanitize_session_stem("...foo.."), "foo");
    }
}
