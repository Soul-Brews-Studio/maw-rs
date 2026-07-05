# maw-rs

Rust port of maw-js — distributed terminal multiplexing & fleet management.
A Cargo workspace of small, focused crates. BUSL-1.1 licensed.

## Build Gate

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Both must pass before any PR.

## Branches

- `main` — stable, protected. Never push or merge directly.
- `alpha` — integration branch. All PRs target `alpha`.
- `agents/*` — throwaway worktree branches for agent/coder work.

## Releases (CalVer)

Version scheme: `v<YY>.<M>.<SEQ>` — two-digit year, unpadded month, release
**sequence within that month**. The last number is a counter, not a day of
the month: `v26.7.3`, `v26.7.4`, and `v26.7.5` all shipped on 2026-07-03.
The exact commit and build time are embedded in the binary (`maw --version`),
not in the version number.

Cut flow: PRs squash-merge into `alpha`; a release promotes `alpha` → `main`
via a **merge-commit** PR, then tags `v<YY>.<M>.<SEQ>` on `main` and publishes
a GitHub release. GitHub auto-closes `Fixes #N` only on default-branch merges,
so close issues by hand when their PR lands on `alpha`.

macOS install note: copying a new binary over an installed one can SIGKILL on
next run (stale code-sign cache on the reused inode) — `rm` first, then `cp`.

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
the WASM migration design. Shipped fleet plugin artifacts (WASM ship tier,
sha256 pin lifecycle) live in `fleet-plugins/` — see its README.
