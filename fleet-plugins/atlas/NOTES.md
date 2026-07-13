# Atlas fleet plugin — ship status (#72)

Reference implementation: `nat-build-with-oracle/maw-atlas` at `f5cdb6f`. The earlier
full Bun parity port was reviewed in maw-rs PR #159; it was parked because native Atlas
owned the command. PR #346 renamed that native inventory surface to `discord-inv`, so
`atlas` is now plugin-owned.

## Shipped read-only state slice

| file | role |
| --- | --- |
| `plugin.json` | active WASM manifest and SHA-256 artifact pin |
| `plugin.wasm` | Extism artifact |
| `plugin.source.json` | AssemblyScript rebuild manifest |
| `src/plugin.ts` | ship source |

The artifact ports the reference `whoami`, `ls`, `read`, and active-thread listing
behaviors. It uses a host-injected Atlas bot token and a GET-only Discord endpoint
allowlist; token bytes never enter guest memory.

## Deliberate boundary

The reference's mutations, backfill/state writes, SQLite archive, subprocess-backed
commands, and long-running route/watch/server processes remain Bun/JS work. They need
separately scoped capabilities and do not block this useful read-only artifact.
