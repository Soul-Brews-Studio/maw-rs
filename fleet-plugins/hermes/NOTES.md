# hermes - port status (issue #72)

Reference impl (source of truth): `laris-co/hermes-oracle/.maw/plugins/hermes/`.

Hermes is a bridge plugin with two layers:

- Discord REST bot verbs: `whoami`, `send`, `read`, `channels`, `threads`, `line`.
- User-turn/API-server verbs: `chat`, `sessions`, `health`, `server-api`, `api`.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | dev (bun-dev) | **active manifest** - `maw hermes` runs `src/plugin.ts` with Bun |
| `src/plugin.ts` | dev (bun-dev) | TypeScript port of the reference command surface |
| `NOTES.md` | docs | tier status, parity notes, ship-tier blockers |

No `plugin.wasm` or `plugin.source.json` is committed yet. Per `fleet-plugins/README.md`,
`plugin.json` is the active manifest while Hermes remains at bun-dev tier.

## Runtime ladder

| tier | status | notes |
|------|--------|-------|
| bun-dev | ACTIVE | Matches the reference verbs and keeps secrets in env/pass. maw-rs runs bun-dev plugins with cwd = plugin dir, so any caller cwd must come from `$PWD`, not `process.cwd()`. |
| wasm ship | BLOCKED | Needs network capabilities for Discord REST, local API-server, and webhook-relay; also needs an alternative to `pass`/subprocess secret lookup and the `arra` subprocess delegation used by `line`. |

## Parity notes

- Bot-level Discord REST verbs are preserved: `whoami`, `send <channel-id> <text...>`,
  `read <channel-id> [n]`, `channels`, and `threads`.
- User-turn verbs are preserved: `chat <session-id> <text...> [--discord]`,
  `sessions`, `health`, and the nested `server-api`/`api` wrapper.
- `server-api` keeps the reference verb surface: `health`, `sessions`, `create`, `get`,
  `messages`, `chat`, `fork`, `patch`, `delete`, and `raw`.
- Secrets are never committed. The port reads:
  `DISCORD_BOT_TOKEN` or `pass show discord/hermes-nous-gateway-token`,
  `API_SERVER_KEY` or `pass show hermes/api-server-key`, and
  `pass show webhook-relay/api-token` for LINE relay delegation.
- Existing JSON config is not rewritten. The port reads Hermes session origin data from
  `$HERMES_HOME/sessions/sessions.json` and writes only its own seen cursor
  `$HERMES_HOME/maw-hermes-seen.json`. If a future Hermes command edits an existing JSON
  config, use the squad raw-text surgical edit pattern rather than a typed-struct round trip.

## Ship-tier blockers

- WASM currently has no approved network capability shape for Discord REST or webhook-relay.
- WASM cannot shell out to `pass`; ship tier needs host-mediated secret access or another
  explicit capability.
- WASM cannot shell out to `bun /opt/Code/github.com/laris-co/arra/src/arthur-cli.ts` for
  the LINE helper; that behavior needs a native host function or a separate ship-tier design.
