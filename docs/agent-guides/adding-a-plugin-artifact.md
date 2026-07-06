# Adding a plugin artifact

This is the dev-Bun → ship-WASM ladder used by `fleet-plugins/`, including the squad
path through #145, #149, and #235. The full reference remains `fleet-plugins/README.md`.

## Source layout

A fleet plugin directory normally contains:

```text
fleet-plugins/<name>/
  plugin.json          # active manifest
  plugin.wasm          # committed WASM artifact, when ship tier exists
  plugin.source.json   # AssemblyScript source manifest for rebuilding
  src/plugin.ts        # AssemblyScript-subset ship source
  impl.ts              # optional Bun dev-tier fallback/reference
```

The TypeScript/Bun implementation is the dev rung. The shipped rung is a compiled WASM
artifact that the runtime hashes and capability-gates.

## Manifest roles

- Ship-tier active plugins use `plugin.json` as the WASM manifest with `target: "wasm"`,
  `entry.kind: "wasm"`, and `artifact.sha256`.
- Dev-tier-active plugins can keep `plugin.json` on `runtime: "bun-dev"`; if a staged
  `plugin.wasm` is committed, pin it from `plugin.source.json`.
- Every committed `plugin.wasm` must be pinned by either `plugin.json` or
  `plugin.source.json`; prose-only pins do not count.

## Capabilities

Capability names are registry contracts. Use the existing shapes:

- `fs:read:<root>` / `fs:write:<root>` for host-mediated filesystem roots.
- `tmux:read` for tmux inspection.
- `tmux:send` for key injection/nudges.
- `proc:exec:<cmd>` for narrow subprocess grants such as `proc:exec:date`.

Declare only what the plugin needs.

## Rebuild and pin lifecycle

Build from the repo root:

```bash
maw plugin build fleet-plugins/<name>
```

If the AssemblyScript toolchain is missing, run `npm ci` in `packages/wasm-sdk` first.
The build emits `plugin.wasm` and a fresh `artifact.sha256`; keep the active manifest
shape appropriate for the plugin tier, then commit the artifact and updated pin together.

## Tests

Run the normal PR gates plus plugin checks:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo test -p maw-cli --test fleet_plugins_pin_check
```

For squad-like behavior, keep acceptance coverage in `crates/maw-cli/tests/` and let the
pin check prove committed artifact bytes match the manifest hash. The ignored deterministic
rebuild test is available with `cargo test -p maw-cli --test fleet_plugins_pin_check -- --ignored`
when the AS toolchain is installed.
