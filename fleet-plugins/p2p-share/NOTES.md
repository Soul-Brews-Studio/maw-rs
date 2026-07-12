# p2p-share - port status (issue #72)

Reference impl (source of truth): `~/.maw/plugins/p2p-share/`.

p2p-share is the WebRTC terminal-share path: it streams a tmux pane directly to a
browser viewer over peer-to-peer data channels. It does not use sshx, a tunnel, or a
relay server for terminal bytes. It does use a signaling WebSocket to exchange SDP/ICE.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | dev (bun-dev) | **active manifest** - `maw p2p-share` runs `src/plugin.ts` with Bun |
| `src/plugin.ts` | dev (bun-dev) | TypeScript port of reference `index.ts` plus `share-peer.ts` logic |
| `viewer.html` | dev asset | Browser viewer served on the local listen port |
| `package.json` | dev dependency | Declares `werift`; run `bun install` in this directory before starting a share |
| `NOTES.md` | docs | tier status, parity notes, ship-tier blockers |

No `plugin.wasm` or `plugin.source.json` is committed yet. Per `fleet-plugins/README.md`,
`plugin.json` is the active manifest while p2p-share remains at bun-dev tier.

## Runtime ladder

| tier | status | notes |
|------|--------|-------|
| bun-dev | ACTIVE | Matches the reference verb surface and uses Bun, `werift`, `Bun.serve`, WebSocket signaling, and tmux subprocesses. |
| wasm ship | BLOCKED | `werift` is a native Node/Bun WebRTC dependency stack. This cannot become WASM without a credible WebRTC-in-WASM or host-WebRTC design. |

## Parity notes

- Verb surface is preserved: `share <pane> [--signal <url>] [--name <name>] [--port <port>]`, plus `status`/`help`.
- Default signaling URL is preserved: `wss://phd-signaling.laris.workers.dev/ws`.
- Default viewer port is preserved: `7742`.
- Auth key lookup is runtime-only: `P2P_SHARE_KEY` or `AUTH_KEY`. No secrets are committed.
- Without an auth key, startup is blocked unless the operator passes
  `--i-understand-the-risk`; the override prints an authentication-disabled warning.
- The viewer asset is copied from the reference and served from `viewer.html`.
- `src/plugin.ts` lazy-loads `werift` so `status` and argument validation work before `bun install`; starting a share still requires the dependency.

## Ship-tier blockers

- **WebRTC dependency:** `werift` is a native Node/Bun library stack with DTLS/SCTP/ICE behavior. A ship-tier port needs host WebRTC functions or a WASM-compatible WebRTC story.
- **Networking:** The plugin needs a local HTTP listen port for the viewer, outbound WebSocket signaling, STUN/TURN/WebRTC UDP behavior, and browser-side CDN access for xterm assets.
- **Subprocess/PTY:** The plugin shells out to `tmux display-message`, `tmux capture-pane`, `tmux pipe-pane`, `mkfifo`, and `cat` to stream pane bytes.
- **Filesystem:** The plugin reads committed `viewer.html` and writes a temporary FIFO under `/tmp`.
- **Long-running lifecycle:** `share` intentionally runs until interrupted, reconnecting signaling and pausing/resuming the tmux stream as viewers arrive or leave. Ship tier needs an explicit lifecycle/cleanup contract.
