# cross-team-queue — port status (issue #72 / #236)

Reference impl: `Soul-Brews-Studio/maw-cross-team-queue`.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | ship (wasm) | active manifest — `cross-team-queue` scaffold artifact |
| `plugin.wasm` | ship (wasm) | built Extism artifact, pinned by `artifact.sha256` |
| `plugin.source.json` | ship (wasm) | source manifest for `maw plugin build` |
| `src/plugin.ts` | ship (wasm) | AssemblyScript source of the scaffold artifact |

## Slice 1 behavior

The artifact returns the scaffold queue contract only:
`{items:[], stats:{...}, errors:[], schemaVersion:1}`. It accepts `--json` and optional
`--recipient <name>` without mutation or filtering because there is no scanning yet.

## Deferred to slice 2

Read-only inbox scanning is intentionally out of scope until the vault-root capability is
settled (`MAW_VAULT_ROOT` input vs a named manifest root). No write, spawn, network, or
filesystem capability is used in this slice.
