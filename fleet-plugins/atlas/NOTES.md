# Atlas Fleet Plugin

Atlas is active at the **bun-dev** rung only. This directory is a parity port of
`/opt/Code/github.com/nat-build-with-oracle/maw-atlas`, with the reference command
modules copied under `src/commands` and `src/lib`.

## Tier Table

| File | Tier | Notes |
| --- | --- | --- |
| `plugin.json` | dev (bun-dev) | Active manifest. Runs `bun src/plugin.ts ...` with the maw-rs dev-tier banner. |
| `src/plugin.ts` | dev (bun-dev) | CLI dispatcher adapted from reference `index.ts`; same command surface. |
| `src/commands/*` | dev (bun-dev) | Reference command implementations for Discord REST, routing, watchers, dashboard, STT, and inbox. |
| `src/lib/*` | dev (bun-dev) | Reference helpers plus maw-rs cwd adaptation: plugin cwd is this directory, caller cwd is `$PWD`. |
| `plugin.wasm` | ship (wasm) | Not present yet. |
| `plugin.source.json` | ship source/pin | Not present yet; add when a verified WASM artifact exists. |

## Runtime Ladder

| Rung | Status | Reason |
| --- | --- | --- |
| bun-dev | Active | Reference Atlas depends on Bun/Node APIs, Discord network calls, sqlite, local filesystem state, and subprocess orchestration. |
| wasm ship | Blocked | Needs host support or rewrites for network, sqlite/state storage, subprocess execution, and caller-relative filesystem access. |

## Ship-Tier Blockers

- **Network:** Discord REST calls to `https://discord.com/api/v10` and CDN URLs need a declared/hosted network capability path.
- **Filesystem:** Atlas reads and writes `.discord/*`, `.maw/atlas-route/*`, `.maw/atlas-watch/*`, sqlite archives, inbox markdown, avatar image files, and backfill JSON. The wasm port needs scoped fs roots equivalent to the caller repo and Atlas repo.
- **Subprocess:** Commands call `pass`, `maw`, `mawjs`, `bun`, `uv`, `ffmpeg`, `ffprobe`, `git`, and sometimes `tmux`-adjacent maw verbs. Ship tier needs host functions or native rewrites for those paths.
- **SQLite:** `lib/discord-db.ts` uses `bun:sqlite`; ship tier needs a host storage API or a different archive backend.
- **Long-running daemons:** `route start`, `route daemon`, and `watch` spawn or run polling loops. Ship tier needs an explicit daemon/process lifecycle contract.
- **Repo assets:** `serve`, `transcribe`, `inbox`, and some route/oracle helpers still locate `atlas-oracle`/`maw-atlas` repo assets (`parliament`, scripts, registry, inbox). A self-contained ship artifact needs those assets ported or those commands split.

## Parity Notes

- `src/plugin.ts` keeps the reference verb surface: `ls`, `read`, `backfill`, `serve`, `transcribe`, `check`, `wake`, `vesicle`, `add-guild`, `threads`, `slash`, `avatar`, `app`, `team-threads`, `route`, `watch`, `spawn-session`, `inbox`, and `whoami`.
- `check`, `wake`, and `vesicle` remain listed but unresolved by the dispatcher, matching the reference `index.ts` behavior.
- No token or secret material is committed. Token lookup remains runtime-only via `DISCORD_BOT_TOKEN` or `pass show discord/atlas-oracle-token`.
