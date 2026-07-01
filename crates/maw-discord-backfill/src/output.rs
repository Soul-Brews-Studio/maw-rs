use std::{env, fs, path::{Path, PathBuf}};

use serde::{Deserialize, Serialize};

use crate::api::Message;
use crate::error::Result;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CursorState {
    pub live_newest_id: Option<String>,
    pub backfill_oldest_id: Option<String>,
    pub updated_at: Option<String>,
}

pub fn default_out_dir() -> PathBuf {
    env::var("DISCORD_BACKFILL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|h| PathBuf::from(h).join(".discord").join("backfill"))
                .unwrap_or_else(|_| PathBuf::from(".discord/backfill"))
        })
}

pub fn default_state_dir() -> PathBuf {
    env::var("DISCORD_BACKFILL_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|h| PathBuf::from(h).join(".discord").join("backfill-state"))
                .unwrap_or_else(|_| PathBuf::from(".discord/backfill-state"))
        })
}

pub fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || ('\u{0E00}'..='\u{0E7F}').contains(&c)
            {
                c
            } else {
                '_'
            }
        })
        .take(60)
        .collect()
}

pub fn load_cursor(state_dir: &Path, channel_id: &str) -> CursorState {
    let path = state_dir.join(format!("{channel_id}.json"));
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save_cursor(state_dir: &Path, channel_id: &str, mut cur: CursorState) -> Result<()> {
    fs::create_dir_all(state_dir)?;
    cur.updated_at = Some(chrono_lite_now());
    let path = state_dir.join(format!("{channel_id}.json"));
    let body = serde_json::to_string_pretty(&cur)? + "\n";
    fs::write(path, body)?;
    Ok(())
}

pub fn write_channel_json(out_root: &Path, guild: &str, channel: &str, messages: &[Message]) -> Result<PathBuf> {
    let dir = out_root.join(sanitize(guild));
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", sanitize(channel)));
    let body = serde_json::to_string_pretty(messages)? + "\n";
    fs::write(&path, body)?;
    Ok(path)
}

fn chrono_lite_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_spaces_and_truncates() {
        let s = sanitize("road to dev #general 🎉 extra padding that goes well beyond sixty chars");
        assert!(s.len() <= 60);
        assert!(!s.contains(' '));
        assert!(s.contains('_'));
    }

    #[test]
    fn cursor_roundtrip() {
        let dir = std::env::temp_dir().join(format!("backfill-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        save_cursor(
            &dir,
            "chan1",
            CursorState {
                live_newest_id: Some("99".into()),
                backfill_oldest_id: Some("1".into()),
                updated_at: None,
            },
        )
        .expect("save");
        let loaded = load_cursor(&dir, "chan1");
        assert_eq!(loaded.live_newest_id.as_deref(), Some("99"));
        let _ = fs::remove_dir_all(&dir);
    }
}