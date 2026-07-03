use clap::{Parser, Subcommand};
use maw_discord_backfill::api::new_rest;
use maw_discord_backfill::engine::{backfill_channel, run_backfill, BackfillOptions, LogSink};
use maw_discord_backfill::output::{default_state_dir, load_cursor};
use maw_discord_backfill::token::resolve_token;

#[derive(Parser)]
#[command(
    name = "discord-backfill",
    about = "Discord channel backfill — atlas snowflake cursor (Plan B / maw-rs)",
    after_help = "env: DISCORD_BOT_TOKEN | DISCORD_STATE_DIR/.env | pass discord/atlas-oracle-token\nout: ~/.discord/backfill/<guild>/<channel>.json\nstate: ~/.discord/backfill-state/<channelId>.json"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Bot identity check
    Whoami,
    /// List guilds or channels
    List {
        #[command(subcommand)]
        target: ListTarget,
    },
    /// Show incremental watermark
    Cursor { channel_id: String },
    /// Backfill one channel
    Channel {
        channel_id: String,
        #[arg(long, default_value_t = 1000)]
        limit: usize,
        #[arg(long)]
        guild: Option<String>,
        #[arg(long)]
        no_incremental: bool,
    },
    /// Backfill guild sweep
    Guild {
        #[arg(long)]
        guild: Option<String>,
        #[arg(long)]
        all: bool,
        #[arg(long)]
        limit: Option<usize>,
        #[arg(long)]
        list: bool,
        #[arg(long)]
        no_incremental: bool,
    },
    #[cfg(feature = "index")]
    /// Upsert channel messages into sqlite (Phase 2)
    Index { channel_id: String },
    #[cfg(feature = "index")]
    /// FTS search indexed messages (Phase 2)
    Search { query: String },
}

#[derive(Subcommand)]
enum ListTarget {
    Guilds,
    /// List text channels (optional guild filter — parity Bun 8f570de)
    Channels {
        /// Filter guild by name or id
        #[arg(long)]
        guild: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[allow(clippy::too_many_lines)]
async fn run() -> maw_discord_backfill::Result<()> {
    let cli = Cli::parse();
    let token = resolve_token()?;
    let rest = new_rest()?;

    match cli.command {
        Commands::Whoami => {
            let me = maw_discord_backfill::api::whoami(&rest, &token).await?;
            let username = me.get("username").and_then(|v| v.as_str()).unwrap_or("?");
            let id = me.get("id").and_then(|v| v.as_str()).unwrap_or("?");
            println!("✓ @{username} ({id})");
        }
        Commands::List { target } => {
            let mut log = |line: &str| println!("{line}");
            let mut sink = LogSink(&mut log);
            match target {
                ListTarget::Guilds => {
                    let guilds = maw_discord_backfill::api::list_guilds(&rest, &token).await?;
                    for g in &guilds {
                        println!("  {} ({})", g.name, g.id);
                    }
                    println!("{} guild(s)", guilds.len());
                }
                ListTarget::Channels { guild } => {
                    let opts = BackfillOptions {
                        guild_filter: guild,
                        list_only: true,
                        ..Default::default()
                    };
                    run_backfill(&rest, &token, &opts, &mut sink).await?;
                }
            }
        }
        Commands::Cursor { channel_id } => {
            let state = default_state_dir();
            let cur = load_cursor(&state, &channel_id);
            println!("{}", serde_json::to_string_pretty(&cur)?);
        }
        Commands::Channel {
            channel_id,
            limit,
            guild,
            no_incremental,
        } => {
            println!("backfill channel {channel_id} limit={limit}");
            let incremental = !no_incremental;
            if incremental {
                let state = default_state_dir();
                let cur = load_cursor(&state, &channel_id);
                if let Some(ref newest) = cur.live_newest_id {
                    println!("incremental since {newest}");
                }
            } else {
                println!("full fetch (no incremental watermark)");
            }
            let mut log = |line: &str| println!("{line}");
            let mut sink = LogSink(&mut log);
            let opts = BackfillOptions {
                guild_filter: guild.clone(),
                limit,
                incremental,
                ..Default::default()
            };
            let guild_name = guild.as_deref().unwrap_or("direct");
            let n = backfill_channel(
                &rest,
                &token,
                &channel_id,
                &channel_id,
                guild_name,
                &opts,
                &mut sink,
            )
            .await?;
            println!("done {n} messages");
        }
        Commands::Guild {
            guild,
            all,
            limit,
            list,
            no_incremental,
        } => {
            let limit = if all { 10_000 } else { limit.unwrap_or(100) };
            let mut log = |line: &str| println!("{line}");
            let mut sink = LogSink(&mut log);
            let opts = BackfillOptions {
                guild_filter: guild,
                limit,
                list_only: list,
                incremental: !no_incremental,
                ..Default::default()
            };
            run_backfill(&rest, &token, &opts, &mut sink).await?;
        }
        #[cfg(feature = "index")]
        Commands::Index { channel_id } => {
            maw_discord_backfill::index::index_channel(&channel_id)?;
        }
        #[cfg(feature = "index")]
        Commands::Search { query } => {
            maw_discord_backfill::index::search(&query)?;
        }
    }

    Ok(())
}
