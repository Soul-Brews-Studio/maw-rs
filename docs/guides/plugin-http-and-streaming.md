# Plugins that serve HTTP, or stream output live

Two things a plugin author will otherwise rediscover the hard way. Both were
rediscovered independently by multiple oracles before this page existed; one of
them was also stated backwards, with confidence, by the oracle who maintains this
repo. Hence the page.

## 1. `engine.serve` — maw will host your HTTP server for you

**It is wired.** A plugin does *not* need to bind its own port, pick a free one,
supervise a background process, or write a pid/state file. Declare it and `maw
serve` does the rest.

```json
{
  "name": "example",
  "engine": {
    "serve": {
      "command": "bun run server/index.ts",
      "prefix": "/api/example",
      "health": "/api/example/health",
      "eventPath": "/api/example/events"
    }
  }
}
```

What `maw serve` then does — `crates/maw-cli/src/core_impl/serve_plugin_proxy.rs`:

| step | where |
|---|---|
| reads `plugin.manifest.engine.serve` | `:33` |
| validates `prefix` (must start `/api/`), `health`, `eventPath` | `:36–48` |
| refuses a `prefix` that collides with a core route, and says so on stderr | `:70` |
| **lazily spawns** `command` on a loopback `127.0.0.1:0` port | `:189` |
| passes `PORT`, `MAW_ENGINE_SERVE_PORT`, `MAW_ENGINE_SERVE_PREFIX` to it | `:189` |
| routes `prefix` and `prefix/*path` onto maw serve's own router | `:105–106` |
| upgrades WebSockets through the same proxy | `:142` |

Your server reads `PORT` from the environment and binds loopback. Callers reach it
at `http://127.0.0.1:3456<prefix>` — maw serve's port, not yours.

Live examples: `people` (`/api/v1`), `synapse` (`/api/synapse`).

### If your plugin is silently not mounted

This on stderr at boot is the collision guard doing its job, not a bug:

```
maw serve: skipping plugin messages: engine.serve prefix /api/message-ledger collides with core route
```

Pick a prefix that does not shadow a native route. Nothing else is wrong.

### `api` is NOT the same field

`plugin.json`'s top-level `api` **is** metadata-only — its three consumers are all
display (`plugins.rs`, `plugin_manifest_bind_host.rs`, `plugin_ls_render.rs`). It
declares "I have an HTTP surface" for discovery; it mounts nothing. Do not reach
for it expecting `engine.serve` behaviour.

## 2. Streaming output — maw buffers your stdout until you exit

A long-running or streaming plugin that writes progress to stdout **looks frozen**.
`dispatch_bun_dev_plugin` gives the child `Stdio::piped()` and reads it to
completion, so nothing surfaces until the process ends.

To stream live, write to the controlling terminal directly:

```ts
import { openSync, writeSync, closeSync } from "node:fs";

let tty: number | null = null;
try { tty = openSync("/dev/tty", "a"); } catch { tty = null; }

if (tty === null) {
  // headless / piped / cron — no terminal to stream to. Say so; do not
  // silently produce nothing.
  console.log("no terminal to stream to — run `tail -f <logfile>` yourself");
} else {
  try { writeSync(tty, "live line\n"); } finally { closeSync(tty); }
}
```

Always handle the `null` case. Under cron, CI, or a captured pipe there is no
`/dev/tty`, and a plugin that assumes one will throw where it used to work.

Live examples: `hall` (`index.ts`, the `logs` command) and `citation`.

### Keep machine output on stdout

Stream *human* progress to `/dev/tty`; keep the parseable result on stdout, and
diagnostics on stderr. A caller merging streams (`2>&1`, `capture_output=True`)
must still get valid JSON — that exact mistake broke every dev-tier plugin's JSON
output until the dev-tier banner was moved off stderr-by-default (#778/#780).

## Related

- `docs/guides/adding-a-plugin-artifact.md` — manifest and artifact contract
- `crates/maw-cli/src/core_impl/serve_plugin_proxy.rs` — the proxy itself
