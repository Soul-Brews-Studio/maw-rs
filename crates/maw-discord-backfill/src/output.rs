use std::{
    env, fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::api::Message;
use crate::error::{Error, Result};

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
            if c.is_ascii_alphanumeric()
                || c == '_'
                || c == '-'
                || ('\u{0E00}'..='\u{0E7F}').contains(&c)
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

pub fn channel_json_path(out_root: &Path, guild: &str, channel: &str) -> PathBuf {
    out_root
        .join(sanitize(guild))
        .join(format!("{}.json", sanitize(channel)))
}

pub fn load_channel_json(path: &Path) -> Result<Vec<Message>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = fs::read_to_string(path)?;
    serde_json::from_str(&raw).map_err(Error::Json)
}

pub fn write_channel_json(
    out_root: &Path,
    guild: &str,
    channel: &str,
    messages: &[Message],
) -> Result<PathBuf> {
    let dir = out_root.join(sanitize(guild));
    fs::create_dir_all(&dir)?;
    let path = channel_json_path(out_root, guild, channel);
    let body = serde_json::to_string_pretty(messages)? + "\n";
    fs::write(&path, body)?;
    Ok(path)
}

/// Incremental delta merge — keep existing history, append new ids, sort.
pub fn merge_channel_json(
    out_root: &Path,
    guild: &str,
    channel: &str,
    delta: &[Message],
) -> Result<(PathBuf, usize)> {
    let path = channel_json_path(out_root, guild, channel);
    if delta.is_empty() {
        return Ok((path, 0));
    }
    let mut merged = load_channel_json(&path)?;
    let mut seen: std::collections::HashSet<String> = merged.iter().map(|m| m.id.clone()).collect();
    let mut added = 0usize;
    for msg in delta {
        if seen.insert(msg.id.clone()) {
            merged.push(msg.clone());
            added += 1;
        }
    }
    merged.sort_by(|a, b| a.id.cmp(&b.id));
    fs::create_dir_all(path.parent().expect("parent"))?;
    let body = serde_json::to_string_pretty(&merged)? + "\n";
    fs::write(&path, body)?;
    Ok((path, added))
}

fn chrono_lite_now() -> String {
    use time::format_description::well_known::Rfc3339;
    use time::OffsetDateTime;
    OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Message;

    #[test]
    fn sanitize_replaces_spaces_and_truncates() {
        let s = sanitize("road to dev #general 🎉 extra padding that goes well beyond sixty chars");
        assert!(s.len() <= 60);
        assert!(!s.contains(' '));
        assert!(s.contains('_'));
    }

    #[test]
    fn merge_appends_without_wipe() {
        let dir = std::env::temp_dir().join(format!("backfill-merge-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let m1 = Message {
            id: "1".into(),
            channel_id: None,
            author: None,
            content: None,
            timestamp: None,
            edited_timestamp: None,
            attachments: None,
        };
        write_channel_json(&dir, "g", "ch", std::slice::from_ref(&m1)).expect("write");
        let (path, added) = merge_channel_json(&dir, "g", "ch", &[]).expect("merge empty");
        assert_eq!(added, 0);
        assert_eq!(load_channel_json(&path).expect("load").len(), 1);
        let m2 = Message {
            id: "2".into(),
            ..m1.clone()
        };
        let (_, added) = merge_channel_json(&dir, "g", "ch", &[m2]).expect("merge one");
        assert_eq!(added, 1);
        assert_eq!(load_channel_json(&path).expect("load").len(), 2);
        let _ = fs::remove_dir_all(&dir);
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
