# squad — port status (issue #72)

Reference impl (source of truth): `laris-co/athena-oracle/.maw/plugins/squad/impl.ts`.
The locked model — "the lead IS the team; team = repo name minus `-oracle`; start/join/say/ls
implicit to the repo you run from" — is preserved exactly, including every guard.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | dev (bun-dev) | **active manifest** — runs `impl.ts` via Bun through `maw squad` today |
| `impl.ts` | dev (bun-dev) | node/Bun port of the reference, verified live via real `maw squad` |
| `src/plugin.ts` | ship (wasm) | AssemblyScript rewrite, compiled to `plugin.wasm` |
| `plugin.source.json` | ship (wasm) | source manifest for `maw plugin build` (entry = `src/plugin.ts`) |
| `plugin.wasm` | ship (wasm) | built Extism artifact, `sha256:8118879a0415dbdf49db4824fee12afc8d8ce18968ec6d7ab9ebb87d42da5415` |

## Runtime ladder (issue #72 policy)

- **Dev tier (active):** `plugin.json` opts into `"runtime": "bun-dev"` so the CLI dispatch runs
  the real TS via Bun. Loud `⚠ [dev-tier: bun]` banner on every invocation. This is what
  `maw squad` runs today, and it is fully working (verified live: start/adopt/say/ls + guards).
  Dev-tier note: maw-rs runs bun-dev plugins with cwd = the plugin dir, so `impl.ts` derives the
  lead repo from `$PWD` (inherited from the invoking shell), not `process.cwd()`.

- **Ship tier (built, verified, gated on one runtime line):** `src/plugin.ts` → `plugin.wasm`
  via `maw plugin build`. The WASM talks to the host only through capability-gated host fns
  (`fs:read:teams` / `fs:write:teams` / `tmux:read` / `proc:exec:date`) and derives cwd/home
  from the InvokeContext the dispatch injects. It is byte-compatible with athena's roster
  format (config keys `[name, members, createdAt, leadSessionId, leadRepo]`, member keys
  `[agentId, name, color, repo, joinedAt]`, message keys `[from, text, timestamp, color, type,
  read]`) and append-never-clobbers inboxes.

### Why the ship tier is not the active manifest yet

`maw squad` (real CLI dispatch) builds the Extism runtime **without** granting manifest fs roots:

- `crates/maw-cli/src/core_impl/dispatcher.rs:483`
  `let mut runtime = ExtismWasmInvokeRuntime::default();`  ← missing `.with_manifest_fs_roots()`

The grant is wired only into the internal invoke path
(`crates/maw-cli/src/core_impl/plugin_plan.rs:1029`:
`ExtismWasmInvokeRuntime::default().with_manifest_fs_roots()`). Without it, every `maw.fs.*`
call from a plugin dispatched via `maw <cmd>` is denied with
`filesystem path outside declared write roots`, so the wasm cannot touch `~/.claude/teams`.

The wasm is proven correct on the path that *does* grant roots:

```
maw plugin-manifest invoke --scan-dir <dir> --plugin squad --source cli --arg start
maw plugin-manifest invoke --scan-dir <dir> --plugin squad --source cli --arg ls
maw plugin-manifest invoke --scan-dir <dir> --plugin squad --source cli --arg say --arg digger --arg "hi"
```

all succeed and write byte-compatible roster files.

**Fix (one line, sanctioned mechanism — the same call the internal path already uses):**
change `dispatcher.rs:483` to `ExtismWasmInvokeRuntime::default().with_manifest_fs_roots()`.
This is a runtime change outside the plugin surface, flagged for the runtime owners rather than
applied here. The exec-only acceptance probe (`probe_runs_via_cli_dispatch`) passed because it
never exercises `maw.fs.*`, so the gap slipped through — a fs-exercising acceptance case would
catch it.

### Graduation (once the runtime grants CLI-dispatch fs roots)

Swap the active manifest to the ship tier: rebuild from `plugin.source.json`
(`maw plugin build` with `entry = src/plugin.ts`) and point `plugin.json` at `plugin.wasm`
(`target: wasm`, `entry: {kind: wasm, path: plugin.wasm, export: handle}`, `artifact.sha256`).
`impl.ts` stays as the documented dev-tier fallback.

## join

Per canon, `join` (session-create) stays on the sanctioned `mawjs` interim until maw-rs grows a
native session-create verb. Both tiers implement it as a loud pointer
(`run: mawjs squad join <oracle> <color>`), never a broken half-spawn. `impl.ts` retains the full
reference `join` (tmux clash pre-check, `maw locate`, roster append) behind the `mawjs` shell-out.

## Wall clock in WASM

The Extism host exposes no time host fn, so `src/plugin.ts` gets timestamps via `proc:exec:date`
(`date +%s` for millis ids, `date -u +…` for ISO). This matches the reference's real-timestamp
behavior while staying inside the capability model (`/bin/date` resolves under the sandbox
`PATH=/usr/bin:/bin`; `maw` does not, which is why the session id falls back to a timestamp per
the reference's own fallback rather than shelling out to `maw team-agent uuid`).
