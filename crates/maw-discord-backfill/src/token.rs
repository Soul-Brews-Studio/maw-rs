use std::{env, fs, path::Path, process::Command};

use crate::error::{Error, Result};

/// Resolve bot token — parity `discord-backfill-cli` / atlas.
pub fn resolve_token() -> Result<String> {
    if let Ok(token) = env::var("DISCORD_BOT_TOKEN") {
        let trimmed = token.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_owned());
        }
    }

    if let Ok(state_dir) = env::var("DISCORD_STATE_DIR") {
        let env_path = Path::new(&state_dir).join(".env");
        if let Ok(raw) = fs::read_to_string(env_path) {
            for line in raw.lines() {
                if let Some(value) = line.strip_prefix("DISCORD_BOT_TOKEN=") {
                    let trimmed = value.trim().trim_matches('"').trim_matches('\'');
                    if !trimmed.is_empty() {
                        return Ok(trimmed.to_owned());
                    }
                }
            }
        }
    }

    let output = Command::new("pass")
        .args(["show", "discord/atlas-oracle-token"])
        .output()
        .map_err(|e| Error::Token(format!("pass failed: {e}")))?;
    if output.status.success() {
        let tok = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if !tok.is_empty() {
            return Ok(tok);
        }
    }

    Err(Error::Token(
        "set DISCORD_BOT_TOKEN, DISCORD_STATE_DIR/.env, or pass discord/atlas-oracle-token".into(),
    ))
}
