use std::time::Duration;

use maw_discord::DiscordRest;
use tokio::time::sleep;

use crate::api::{self, slim_message, Channel, Guild};
use crate::error::Result;
use crate::output::{self, CursorState};

#[derive(Debug, Clone, Default)]
pub struct BackfillOptions {
    pub guild_filter: Option<String>,
    pub channel_id: Option<String>,
    pub limit: usize,
    pub list_only: bool,
    pub incremental: bool,
    pub out_dir: Option<std::path::PathBuf>,
    pub state_dir: Option<std::path::PathBuf>,
}

pub struct LogSink<'a>(pub &'a mut dyn FnMut(&str));

impl<'a> LogSink<'a> {
    fn log(&mut self, line: &str) {
        (self.0)(line);
    }
}

fn guild_matches(g: &Guild, filter: &str) -> bool {
    g.id.contains(filter) || g.name.to_lowercase().contains(&filter.to_lowercase())
}

fn is_channel_forbidden(err: &crate::error::Error) -> bool {
    matches!(err, crate::error::Error::Api(msg) if msg.contains("403"))
}

fn tally_channel_result(
    ch_name: &str,
    result: Result<usize>,
    log: &mut LogSink<'_>,
    grand_total: &mut usize,
    channel_count: &mut usize,
) -> Result<()> {
    match result {
        Ok(n) => {
            *grand_total += n;
            *channel_count += 1;
            Ok(())
        }
        Err(e) if is_channel_forbidden(&e) => {
            log.log(&format!("  ✗ #{ch_name}: access denied (403)"));
            Ok(())
        }
        Err(e) => Err(e),
    }
}

pub async fn backfill_channel(
    rest: &dyn DiscordRest,
    token: &str,
    channel_id: &str,
    channel_name: &str,
    guild_name: &str,
    opts: &BackfillOptions,
    log: &mut LogSink<'_>,
) -> Result<usize> {
    let out_root = opts.out_dir.clone().unwrap_or_else(output::default_out_dir);
    let state_root = opts
        .state_dir
        .clone()
        .unwrap_or_else(output::default_state_dir);
    let limit = if opts.limit == 0 { 10_000 } else { opts.limit };

    let cur = if opts.incremental {
        output::load_cursor(&state_root, channel_id)
    } else {
        CursorState::default()
    };
    let stop = cur.live_newest_id.as_deref();
    let outcome = api::fetch_messages(rest, token, channel_id, limit, stop).await?;
    let msgs = outcome.messages;
    if outcome.cap_hit {
        log.log(&format!(
            "  ⚠ #{channel_name}: hit limit {limit} — older history may remain (re-run with higher --limit or --all)"
        ));
    }
    let slim: Vec<_> = msgs.iter().map(slim_message).collect();
    let (path, written) = if opts.incremental {
        output::merge_channel_json(&out_root, guild_name, channel_name, &slim)?
    } else {
        let path = output::write_channel_json(&out_root, guild_name, channel_name, &slim)?;
        (path, msgs.len())
    };

    if !msgs.is_empty() {
        let oldest = if opts.incremental {
            cur.backfill_oldest_id
                .unwrap_or_else(|| msgs.first().expect("non-empty").id.clone())
        } else {
            msgs.first().expect("non-empty").id.clone()
        };
        output::save_cursor(
            &state_root,
            channel_id,
            CursorState {
                live_newest_id: Some(msgs.last().expect("non-empty").id.clone()),
                backfill_oldest_id: Some(oldest),
                updated_at: None,
            },
        )?;
    }

    if opts.incremental && msgs.is_empty() {
        log.log(&format!(
            "  ✓ #{channel_name}: 0 new (kept existing) → {}",
            path.display()
        ));
    } else if opts.incremental {
        log.log(&format!(
            "  ✓ #{channel_name}: +{written} new ({} fetched) → {}",
            msgs.len(),
            path.display()
        ));
    } else {
        log.log(&format!(
            "  ✓ #{channel_name}: {} msgs → {}",
            msgs.len(),
            path.display()
        ));
    }
    Ok(if opts.incremental {
        written
    } else {
        msgs.len()
    })
}

pub async fn run_backfill(
    rest: &dyn DiscordRest,
    token: &str,
    opts: &BackfillOptions,
    log: &mut LogSink<'_>,
) -> Result<(usize, usize)> {
    let limit = if opts.list_only {
        0
    } else if opts.limit == 0 {
        10_000
    } else {
        opts.limit
    };

    if let Some(ref channel_id) = opts.channel_id {
        if !opts.list_only {
            let guild = opts
                .guild_filter
                .clone()
                .unwrap_or_else(|| "direct".to_owned());
            let n =
                backfill_channel(rest, token, channel_id, channel_id, &guild, opts, log).await?;
            return Ok((n, 1));
        }
    }

    let guilds = api::list_guilds(rest, token).await?;
    log.log(&format!("{} guild(s)", guilds.len()));

    let mut grand_total = 0usize;
    let mut channel_count = 0usize;

    for g in guilds {
        if let Some(ref filter) = opts.guild_filter {
            if !guild_matches(&g, filter) {
                continue;
            }
        }

        let channels = match api::guild_channels(rest, token, &g.id).await {
            Ok(chs) => chs,
            Err(_) => {
                log.log(&format!("  ✗ {}: access denied", g.name));
                continue;
            }
        };

        let text = api::filter_text_channels(&channels);
        log.log(&format!("\n{} — {} text channel(s)", g.name, text.len()));

        if opts.list_only {
            for ch in &text {
                log.log(&format!("  💬 #{} ({})", ch.name, ch.id));
            }
            continue;
        }

        let targets: Vec<Channel> = if let Some(ref cid) = opts.channel_id {
            text.into_iter().filter(|c| c.id == *cid).collect()
        } else {
            text
        };

        if opts.channel_id.is_some() && targets.is_empty() {
            if let Some(ref cid) = opts.channel_id {
                let mut channel_opts = opts.clone();
                channel_opts.limit = limit;
                tally_channel_result(
                    cid,
                    backfill_channel(rest, token, cid, cid, &g.name, &channel_opts, log).await,
                    log,
                    &mut grand_total,
                    &mut channel_count,
                )?;
            }
            continue;
        }

        let mut guild_subtotal = 0usize;
        for ch in targets {
            let mut channel_opts = opts.clone();
            channel_opts.limit = limit;
            let before = grand_total;
            tally_channel_result(
                &ch.name,
                backfill_channel(rest, token, &ch.id, &ch.name, &g.name, &channel_opts, log).await,
                log,
                &mut grand_total,
                &mut channel_count,
            )?;
            guild_subtotal += grand_total.saturating_sub(before);
            sleep(Duration::from_millis(1000)).await;
        }

        if !opts.list_only {
            log.log(&format!("  guild total: {guild_subtotal} msgs"));
        }
    }

    Ok((grand_total, channel_count))
}
