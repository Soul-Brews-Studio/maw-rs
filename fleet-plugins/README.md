# fleet-plugins — shipped WASM artifact home

This directory is the versioned home for **shipped fleet plugin artifacts** — the
ship-tier rung of the runtime ladder (issue #72). Each plugin here is a compiled,
`sha256`-pinned WASM artifact that any oracle on the machine can install and run via
`maw plugin install`. It is the plugin analogue of `examples/wasm-parity`: artifacts
are committed, `sha256`-pinned, and CI-verified against their source.

Priority order for what lands here (per #72 canon): **squad** (reference impl, locked
model) → **hermes** → **atlas** → **team**.

## Plugin catalog

Facts below come from each plugin's `plugin.json` and `NOTES.md`.

| plugin | tier | verbs | needs | tests | ship-tier blocker |
| ------ | ---- | ----- | ----- | ----- | ----------------- |
| `squad` | ship-wasm | `start`, `join <oracle> [color]`, `say <member> <text>`, `ls` | No creds; team roster fs read/write, tmux read, `proc:exec:date` wall-clock subprocess. | Rust acceptance suite (`crates/maw-cli/tests/squad_acceptance.rs`) plus pin check (`crates/maw-cli/tests/fleet_plugins_pin_check.rs`). | None; active manifest is `target=wasm` with `plugin.wasm` pinned by `artifact.sha256`. |
| `hermes` | bun-dev | Discord REST: `whoami`, `send`, `read`, `channels`, `threads`, `line`; user/API turns: `chat`, `sessions`, `health`, `server-api`, `api`. | Creds from env or `pass`; Discord/API/webhook-relay network; `pass` subprocess secret lookup; `arra` subprocess for `line`. | `bun test fleet-plugins/hermes/src/plugin.test.ts` | Network capability shape, host-mediated secrets instead of `pass`, and replacing the `arra` subprocess delegation. |
| `share` | bun-dev | `start`, `ls`, `url`, `stop` with optional `--name <label>`. | `sshx` subprocess dependency, optional `MAW_SHARE_SERVER`/`MAW_SHARE_SSHX_BIN`, local `~/.maw/share` state, pid signal management. | `bun test fleet-plugins/share/src/plugin.test.ts` | Long-lived child process lifecycle, process spawn/signal delivery, and local private state need host capability design. |
| `p2p-share` | bun-dev | `share <pane> [--signal <url>] [--name <name>] [--port <port>]`; `status`/`help`. | Optional `P2P_SHARE_KEY`/`AUTH_KEY`; `werift` from `bun install`; local HTTP listen, WebSocket signaling, WebRTC UDP/STUN, tmux/`mkfifo`/`cat`, temp FIFO. | `bun test fleet-plugins/p2p-share/src/plugin.test.ts` | `werift` is a native Node/Bun WebRTC dependency; needs WebRTC-in-WASM or host-WebRTC, plus networking, PTY, fs, and lifecycle contracts. |

> The dev-tier TS source is *not* the thing that ships. Athena's TypeScript stays the
> dev-tier source of truth; the artifact here is what production installs. See the
> runtime ladder below.

## Layout

```
fleet-plugins/
  <name>/
    plugin.json          # active manifest (ship-tier wasm, OR bun-dev dev-tier — see below)
    plugin.wasm          # compiled artifact, byte-pinned by artifact.sha256
    plugin.source.json   # SOURCE manifest (entry=src/plugin.ts) — rebuild + pin the wasm
    src/plugin.ts        # AssemblyScript-subset ship source
    impl.ts              # (optional) bun-dev dev-tier source, for a still-bun-dev plugin
```

A **ship manifest** — the one the runtime loads at the wasm tier — declares:

- `"target": "wasm"`
- `"artifact": { "path": "./plugin.wasm", "sha256": "sha256:<hex>" }`
- `"entry": { "kind": "wasm", "path": "plugin.wasm", "export": "handle" }`
- `"capabilities": [ … ]` — consumed by the runtime/registry capability gates
- `"cli": { "command": "<name>" }` — the user-facing `maw <name>` verb

**Which file carries the pin depends on the active tier:**

- **Ship-tier active** (e.g. `hostfn-probe`): `plugin.json` *is* the ship manifest and
  carries `target=wasm` + `artifact.sha256`.
- **Still bun-dev active** (e.g. squad, whose wasm is built and verified but gated on a
  runtime fs-roots grant): `plugin.json` stays the `runtime=bun-dev` dev manifest, and the
  pre-staged `plugin.wasm` is pinned in **`plugin.source.json`** (`target=wasm` +
  `artifact.path` + `artifact.sha256`) instead. This keeps `maw <name>` on the working
  dev tier while committing a machine-readable pin for the artifact.

**Rule: every committed `plugin.wasm` must be pinned by one of these manifests.** The pin
check (below) refuses an unpinned artifact — the pin must live in a manifest, never only
in prose. `plugin.source.json` also names the source entry (`src/plugin.ts`), so one file
says both "source" and "built artifact + pin" — same convention as the `hostfn-probe`
fixture in `crates/maw-cli/tests/fixtures/hostfn-probe/`.

## Rebuilding an artifact

Build against the **pinned** `@maw-rs/wasm-sdk` (in `packages/wasm-sdk`; override the SDK
location with `MAW_WASM_SDK_DIR`). The pipeline compiles the AS-compatible `.ts` to WASM
and emits the ship manifest with a fresh `artifact.sha256`:

```bash
maw plugin build fleet-plugins/<name>
# -> "ship tier ready: plugin.wasm" + plugin.json rewritten with target=wasm + artifact.sha256
```

`maw plugin build` requires the AssemblyScript toolchain — run `npm ci` in
`packages/wasm-sdk` once first. Arbitrary Bun/Node TS that isn't in the AS subset needs a
prebuilt artifact instead (see `PLUGIN_AS_TS_BOUNDARY`). `join`-style plugins that need
native process spawn stay on the sanctioned `mawjs` path until native spawn lands.

## How artifacts are verified (the pin check)

The `artifact.sha256` in the manifest is the pin. It is enforced at **two** points:

1. **Load time (runtime, security checkpoint).** On discovery the runtime hashes the
   artifact and refuses to load the plugin if the bytes don't match the manifest
   `sha256` — "artifact hash mismatch — refusing to load". This catches tampering *at
   rest* in the install root, not just at build time. (See
   `maw_plugin_manifest::hash_file` and the discovery refusal path.)

2. **CI / test time (integrity + determinism).**
   `crates/maw-cli/tests/fleet_plugins_pin_check.rs`:
   - `fleet_plugins_artifacts_match_manifest_sha256` — runs on the **default**,
     toolchain-free `cargo test` path (so it gates every narrow run and CI). For each
     `fleet-plugins/<name>/`, it reads the `artifact` pin from **both** `plugin.json` and
     `plugin.source.json`, hashes the referenced artifact, and asserts it equals the
     declared `artifact.sha256`. It then enforces coverage: a committed `plugin.wasm` that
     no manifest pins **fails the build**, so no artifact ships unverified. A byte that
     drifts from the pin fails too.
   - `fleet_plugins_rebuild_is_deterministic` — `#[ignore]`, gated on the AS toolchain
     exactly like `plugin_hostfn_probe_acceptance::probe_builds_via_pipeline`. Where a
     `plugin.source.json` is present it rebuilds via `maw plugin build` and asserts the
     rebuilt `plugin.wasm` reproduces the committed pin. Run it explicitly:
     `cargo test -p maw-cli --test fleet_plugins_pin_check -- --ignored` (after
     `npm ci` in `packages/wasm-sdk`).

This mirrors how `examples/wasm-parity` fixtures are CI-verified
(`npm --prefix packages/wasm-sdk run check:fixtures` rebuilds and `git diff --exit-code`s
the goldens). The Rust pin-check keeps the fast default gate toolchain-free; the ignored
rebuild test adds byte-for-byte determinism when `asc` is available.

## Installing

`maw plugin install` copies a built plugin dir into the plugin root as `<root>/<name>/`:

```bash
maw plugin install fleet-plugins/<name>
# -> "installed <name>@<version> <root>/<name>"
```

**Default root** (verified from `plugin_plan.rs` → `maw_data_dir`):

| condition                     | plugin root                       |
| ----------------------------- | --------------------------------- |
| default (no XDG opt-in)       | `~/.maw/plugins`                  |
| `MAW_XDG` truthy              | `~/.local/share/maw/plugins`      |
| `MAW_DATA_DIR=<dir>`          | `<dir>/plugins`                   |
| `MAW_HOME=<dir>`              | `<dir>/plugins`                   |

Target an explicit root with `--root`:

```bash
maw plugin install fleet-plugins/<name> --root ~/.maw/plugins
```

Notes on the install contract:

- Install **copies as-is** (skipping `.git`, `node_modules`, `target`); it does *not*
  re-hash at copy time. The `sha256` pin is enforced at **load** — the first
  `maw <command>` after install verifies the artifact and refuses on mismatch. This is
  the right checkpoint: it also defends the installed copy against later tampering.
- Because the artifact lands in the shared plugin root, `maw <name>` (e.g. `maw squad`)
  resolves for **any** oracle on the machine once installed.
- Co-version each artifact with the runtime it depends on — the host-fn ABI is
  unblocked as of v26.7.5 (#131/#132/#133/#136/#137).

## Runtime ladder (context)

- **Ship tier** — WASM via Extism, `sha256`-verified. This directory.
- **Dev tier** — TS on Bun via explicit `"runtime": "bun-dev"` (loud unsandboxed banner);
  for iteration/migration, never the shipped form.

Print the authoritative contract with `maw plugin-artifact contract`.
