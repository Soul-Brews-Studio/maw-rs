# share - port status

Share is the minimal maw-rs bridge to the sshx-fork terminal-share server used by
`maw-board` / Oracle maw share.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | dev (bun-dev) | **active manifest** - `maw share` runs `src/plugin.ts` with Bun |
| `src/plugin.ts` | dev (bun-dev) | Minimal sshx start/list/url/stop command surface |
| `NOTES.md` | docs | tier status, security notes, and v1 roadmap |

No `plugin.wasm` or `plugin.source.json` is committed yet. Per `fleet-plugins/README.md`,
`plugin.json` is the active manifest while Share remains at bun-dev tier.

## Runtime ladder

| tier | status | notes |
|------|--------|-------|
| bun-dev | ACTIVE | Spawns `sshx` with Bun, reads one stdout URL line, stores local state in `~/.maw/share/<label>.json`, and manages the child pid. |
| wasm ship | NOT STARTED | No wasm artifact yet; process spawn, signal delivery, and local private state need a host capability design before graduation. |

## v0 behavior

- `share start [--name <label>]` runs `<sshx_bin> --quiet --server <server>`, stores
  `{name,url,pid,startedAt}`, and prints the URL.
- `share ls` lists local state files and whether each stored pid is still alive.
- `share url [label]` prints the stored URL for the label.
- `share stop [label]` sends SIGINT to the stored pid and removes the state file.
- Server and binary are configured with `MAW_SHARE_SERVER` and `MAW_SHARE_SSHX_BIN`.
  Defaults are `https://ssh.clubsxai.com` and `sshx`.

## Security notes

- The sshx URL contains the E2E secret in the `#fragment`.
- The plugin does not print URLs from `share ls`; only `share start` and `share url`
  reveal them because those commands explicitly return the share URL.
- The state directory is chmodded to `700`; state files are written `600`.
- No secrets, URLs, or keys are committed to this repository.
- The deployed server `ssh.clubsxai.com` currently has a Cloudflare EDGE gate that
  returns 403 to the sshx gRPC Open call. This is separate from
  `SSHX_BOARD_PASSWORD`; real use needs a self-hosted server or an edge allowlist.

## v1 roadmap stubs

- Board add: connect to the maw-board CBOR WebSocket protocol and send the share entry
  with `encrypted_zeros`.
- Publish via ssh-write: write the URL to `SSHX_ORACLE_URL_FILE` so callers can publish
  without scraping stdout.
