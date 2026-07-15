# Adding a plugin artifact

This is the dev-Bun → ship-WASM ladder used by the fleet plugins, including the squad
path through #145, #149, and #235.

> **Repo split phase 1 (2026-07-15):** the fleet plugin packages moved out of this
> repo's `fleet-plugins/` into
> [Soul-Brews-Studio/maw-plugins](https://github.com/Soul-Brews-Studio/maw-plugins)
> under `packages/<name>/`. The full reference is that repo's
> `docs/fleet-plugins.md`. Paths below are relative to a maw-plugins checkout.

## Source layout

A fleet plugin directory normally contains:

```text
packages/<name>/
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
- Dev-tier TypeScript entries are run directly with `bun <entry> ...args`, so Bun will
  only evaluate the module. Add an `import.meta.main` self-invoke block to call the
  exported handler when the file is executed:

  ```ts
  if (import.meta.main) {
    const result = await handler({ source: "cli", args: process.argv.slice(2) });
    if (result.output) console.log(result.output);
    if (result.error) console.error(result.error);
    process.exit(result.ok ? 0 : 1);
  }
  ```

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

Build against a maw-plugins checkout:

```bash
maw plugin build <maw-plugins-checkout>/packages/<name>
```

If the AssemblyScript toolchain is missing, run `npm ci` in **maw-rs's**
`packages/wasm-sdk` first.
The build emits `plugin.wasm` and a fresh `artifact.sha256`; keep the active manifest
shape appropriate for the plugin tier, then commit the artifact and updated pin together.

## Tests

Run the normal maw-rs PR gates:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

The sha256 pin-hash gate (formerly maw-rs's
`crates/maw-cli/tests/fleet_plugins_pin_check.rs`, deleted in the repo split)
now runs in maw-plugins CI: it proves every committed `plugin.wasm` matches its
manifest pin (`plugin.json`, falling back to `plugin.source.json`) and that the
maw-menubar universal helper matches its `bundledArtifacts` pin. Host-side
invoke coverage against the committed artifacts is pending the repo-split test
rework.
