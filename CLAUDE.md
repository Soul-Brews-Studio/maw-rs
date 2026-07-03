# maw-rs

Rust port of maw-js — distributed terminal multiplexing & fleet management.
A Cargo workspace of small, focused crates. BUSL-1.1 licensed.

## Build Gate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Both must pass before any PR. No source file exceeds 250 lines.

## Branches

- `main` — stable, protected. Never push or merge directly.
- `alpha` — integration branch. All PRs target `alpha`.
- `agents/*` — throwaway worktree branches for agent/coder work.

## Architecture

Layered Cargo workspace:

- **Leaf crates** — self-contained, deterministic, side-effect-free core
  logic (matching, routing, identity, transport, plugin manifest, …) with no
  internal dependencies.
- **Mid crates** — compose the leaf crates (e.g. `maw-peer`, `maw-tmux`,
  `maw-worktree`).
- **Top crate** — `maw-cli`, the binary, depends on the rest of the workspace.

Run `cargo tree` for the current, authoritative dependency graph.

## Conventions

- `forbid(unsafe_code)`, clippy pedantic clean.
- Rust edition 2021.
- Behavior is validated against maw-js JSON test fixtures.
- Core crates stay deterministic and side-effect-free.

## Further Docs

See `docs/` for deeper references — including the parity matrix, wire
protocol, "adding a command" guide, agent/coder team spawn conventions, and
the WASM migration design.
