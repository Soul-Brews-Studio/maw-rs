# cross-team-queue — port status (issue #72 / #236)

Reference impl: `Soul-Brews-Studio/maw-cross-team-queue`.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | ship (wasm) | active manifest for the queue scanner |
| `plugin.wasm` | ship (wasm) | scanner artifact, pinned by `artifact.sha256` |
| `plugin.source.json` | ship (wasm) | source manifest for `maw plugin build` |
| `src/plugin.ts` | ship (wasm) | AssemblyScript source of the queue scanner |

## Shipped behavior

The artifact resolves the host-provided vault root, scans each oracle's `inbox/*.md`,
parses message metadata and body text, and returns the unified queue contract. Optional
`--recipient <name>` filters the scanned items.

The scanner uses only `fs:read:vault`: it does not write files, spawn processes, or use
the network. `cross_team_queue_fleet_artifact_installs_and_scans_vault` installs and
invokes the real artifact against a seeded vault, checking both the golden queue output
and recipient filtering. The fleet pin check covers the committed artifact hash.
