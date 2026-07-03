//! Phase 2 — workshop-05 SQLite index (feature `index`).

use crate::error::{Error, Result};

pub fn index_channel(_channel_id: &str) -> Result<()> {
    Err(Error::Other(
        "index subcommand requires --features index (sqlx) — Phase 2".into(),
    ))
}

pub fn search(_query: &str) -> Result<()> {
    Err(Error::Other(
        "search subcommand requires --features index (sqlx) — Phase 2".into(),
    ))
}