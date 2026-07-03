# fleet-plugins — shipped WASM artifact home

This directory is the versioned home for **shipped fleet plugin artifacts** — the
ship-tier rung of the runtime ladder (issue #72). Each plugin here is a compiled,
`sha256`-pinned WASM artifact that any oracle on the machine can install and run via
`maw plugin install`. It is the plugin analogue of `examples/wasm-parity`: artifacts
are committed, `sha256`-pinned, and CI-verified against their source.

Priority order for what lands here (per #72 canon): **squad** (reference impl, locked
model) → **hermes** → **atlas** → **team**.

> The dev-tier TS source is *not* the thing that ships. Athena's TypeScript stays the
> dev-tier source of truth; the artifact here is what production installs. See the
> runtime ladder below.

## Layout

```
fleet-plugins/
  <name>/
    plugin.json          # SHIP manifest: target=wasm, artifact.path + artifact.sha256
    plugin.wasm          # compiled artifact, byte-pinned by artifact.sha256
    plugin.source.json   # (optional) SOURCE manifest — lets CI rebuild from .ts and diff
    src/plugin.ts        # (optional) dev-tier AssemblyScript-subset source, kept alongside
```

The **ship manifest** (`plugin.json`) is the one the runtime loads. It must declare:

- `"target": "wasm"`
- `"artifact": { "path": "./plugin.wasm", "sha256": "sha256:<hex>" }`
- `"entry": { "kind": "wasm", "path": "plugin.wasm", "export": "handle" }`
- `"capabilities": [ … ]` — consumed by the runtime/registry capability gates
- `"cli": { "command": "<name>" }` — the user-facing `maw <name>` verb

Keep the dev-tier `src/` and (recommended) a `plugin.source.json` alongside so the
artifact can be rebuilt and its determinism proven. `plugin.source.json` is the
pre-build (source) form of the manifest — same convention as the `hostfn-probe` fixture
in `crates/maw-cli/tests/fixtures/hostfn-probe/`.

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
     toolchain-free `cargo test` path (so it gates every narrow run and CI). It scans
     every `fleet-plugins/<name>/plugin.json`, and for each `target=wasm` manifest hashes
     `plugin.wasm` and asserts it equals the declared `artifact.sha256`. A `wasm` manifest
     with no `artifact.sha256`, or a byte that drifts from the pin, fails the build.
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
