# maw-discord-backfill

Standalone Discord channel backfill CLI — **Plan B** in maw-rs alpha workspace.

UX parity with [`discord-backfill-cli`](https://github.com/MEYD-605/discord-backfill-cli) @ `8f570de` (Act I Bun), ported from `maw-js/plugins/atlas/commands/backfill.ts`.

## Build

```bash
cargo build -p maw-discord-backfill
cargo test -p maw-discord-backfill
cargo clippy -p maw-discord-backfill -- -D warnings
```

## Usage

```bash
export DISCORD_STATE_DIR=~/.claude/channels/discord-gmgrok  # or DISCORD_BOT_TOKEN

discord-backfill whoami
discord-backfill list guilds
discord-backfill list channels --guild HUMAN
discord-backfill channel 1500775333283237970 --limit 50
discord-backfill guild --guild HUMAN --limit 100
discord-backfill guild --all
discord-backfill cursor 1500775333283237970
```

## Output paths

| Kind | Path |
|------|------|
| Messages JSON | `~/.discord/backfill/<guild>/<channel>.json` |
| Incremental state | `~/.discord/backfill-state/<channelId>.json` |

Override with `DISCORD_BACKFILL_DIR` / `DISCORD_BACKFILL_STATE`.

## Phase 2 (deferred)

`index` / `search` subcommands behind `--features index` (sqlx sqlite + FTS).

## Pair

- Act I: `MEYD-605/discord-backfill-cli@8f570de`
- Plan: gmgrok-oracle `ψ/inbox/2026-07-01_0825_discord-backfill-rs-plan-abc_from_gmgrok.md`