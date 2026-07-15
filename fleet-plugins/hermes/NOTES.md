# hermes - port status (issue #72)

Reference impl (source of truth): `laris-co/hermes-oracle/.maw/plugins/hermes/`.

Hermes ships a read-only Discord REST core as a pinned WASM artifact. The broader Bun
implementation remains as reference and development source for mutation, API-server,
and LINE behaviors that are not part of the shipped slice.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | ship (wasm) | active manifest and artifact pin |
| `plugin.wasm` | ship (wasm) | Extism artifact implementing the read-only core |
| `plugin.source.json` | ship (wasm) | build manifest for `src/plugin.as.ts` |
| `src/plugin.as.ts` | ship (wasm) | AssemblyScript source for the read-only core |
| `src/plugin.ts` | dev/reference (Bun) | broader TypeScript command surface |
| `NOTES.md` | docs | tier status and parity boundary |

## Runtime ladder

| tier | status | notes |
|------|--------|-------|
| wasm ship | ACTIVE | `whoami`, `channels`, `read`, and `threads list/read`; host-mediated bot secret and GET-only Discord REST access. |
| Bun reference | ACTIVE | Retains the broader command surface for development and later ports. |

## Parity notes

- The active artifact implements `whoami`, `channels`, `read <channel-id> [n]`,
  `threads list <guild-id>`, and `threads read <channel-id> [n]`.
- The host injects the Discord bot secret and restricts the artifact to declared GET
  endpoints; no credential is committed or obtained by spawning `pass` from WASM.
- `hermes_fleet_artifact_invokes_discord_read_only_verbs` installs and invokes the real
  artifact against deterministic host responses and checks the golden output.
- The manifest pin matches the committed `plugin.wasm` and is covered by the fleet pin
  check.

## Broader reference surface

`send`, `chat`, `sessions`, `health`, `server-api`, `api`, and `line` remain in
`src/plugin.ts`. Porting those behaviors requires separately scoped mutation, API-server,
webhook, persistence, or subprocess capabilities; none blocks the shipped read-only core.
