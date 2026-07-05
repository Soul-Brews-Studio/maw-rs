# squad — port status (issue #72)

Reference impl (source of truth): `laris-co/athena-oracle/.maw/plugins/squad/impl.ts`.
The locked model — "the lead IS the team; team = repo name minus `-oracle`; start/join/say/ls
implicit to the repo you run from" — is preserved exactly, including every guard.

## Files

| file | tier | role |
|------|------|------|
| `plugin.json` | ship (wasm) | **active manifest** — `maw squad` runs `plugin.wasm` on Extism (graduated in #145) |
| `plugin.wasm` | ship (wasm) | built Extism artifact, `sha256:8118879a0415dbdf49db4824fee12afc8d8ce18968ec6d7ab9ebb87d42da5415` |
| `plugin.source.json` | ship (wasm) | source manifest for `maw plugin build` (entry = `src/plugin.ts`) |
| `src/plugin.ts` | ship (wasm) | AssemblyScript source of the artifact |
| `impl.ts` | dev (bun-dev) | node/Bun dev-tier fallback of the reference, kept for iteration |

## Runtime ladder (issue #72 policy)

- **Ship tier (ACTIVE — graduated in #145):** `src/plugin.ts` → `plugin.wasm` via
  `maw plugin build`. The WASM talks to the host only through capability-gated host fns
  (`fs:read:teams` / `fs:write:teams` / `tmux:read` / `proc:exec:date`) and derives cwd/home
  from the InvokeContext the dispatch injects (cwd = the invoking process cwd — *not* `$PWD`;
  tests that steer the team must chdir, see `crates/maw-cli/tests/squad_acceptance.rs`). It is
  byte-compatible with athena's roster format (config keys `[name, members, createdAt,
  leadSessionId, leadRepo]`, member keys `[agentId, name, color, repo, joinedAt]`, message keys
  `[from, text, timestamp, color, type, read]`) and append-never-clobbers inboxes. The
  `artifact.sha256` pin is enforced at load; `fleet_plugins_pin_check` covers it in CI.

- **Dev tier (fallback):** flip `plugin.json` back to `"runtime": "bun-dev"` + `entry: impl.ts`
  to iterate on the TS with a loud `⚠ [dev-tier: bun]` banner. Dev-tier note: maw-rs runs
  bun-dev plugins with cwd = the plugin dir, so `impl.ts` derives the lead repo from `$PWD`
  (inherited from the invoking shell), not `process.cwd()`.

### Graduation history

The ship tier was blocked from real `maw <cmd>` dispatch by one missing grant —
`dispatcher.rs` built `ExtismWasmInvokeRuntime::default()` without `.with_manifest_fs_roots()`,
so every `maw.fs.*` call was denied (`filesystem path outside declared write roots`) while the
internal `plugin-manifest invoke` path worked. The exec-only acceptance probe missed it because
it never exercises `maw.fs.*`. Fixed in **#144** (CLI dispatch now grants manifest fs roots),
graduated in **#145** (active manifest = wasm), and the fs-exercising acceptance suite that
would have caught it landed as `squad_acceptance.rs` (**#142**).

Graduation recipe for the next fleet plugin (hermes/atlas/team): land bun-dev-active first if
the wasm tier isn't proven, pin the pre-staged wasm in `plugin.source.json`, then graduate by
making `plugin.json` the wasm ship manifest (`target: wasm`, `entry: {kind: wasm, path:
plugin.wasm, export: handle}`, `artifact.sha256`). `plugin.json` is always the active manifest;
every committed `.wasm` must be pinned by some committed manifest (see `fleet-plugins/README.md`).

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
