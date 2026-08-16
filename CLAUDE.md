# maw-rs

Budded from **maw-js** on 2026-05-19

Rust port of maw-js — distributed terminal multiplexing & fleet management.
A Cargo workspace of small, focused crates. BUSL-1.1 licensed.
For repo-wide agent execution conventions, read `AGENTS.md` first; this file remains the Claude-specific memory and release detail.

## Build Gate

```bash
scripts/gate.sh quick   # iterating (fmt + clippy + affected-crate tests)
scripts/gate.sh full    # pre-merge bar — all 4 CI dimensions
```

`full` must pass before any merge/promote (it wraps `cargo fmt --all --check`,
`cargo test --workspace --locked --no-fail-fast`, `cargo clippy --workspace
--all-targets -- -D warnings`, and the wasm-host subset).
Test dimensions run `--no-fail-fast` (#796): one failing target must not mask
the others. See
`docs/guides/gating.md` for tiers, the golden warm cache, and merge-trains.

The toolchain is pinned by `rust-toolchain.toml` — exact channel plus
`targets = ["wasm32-unknown-unknown"]` (#823). CI installs it with a bare
`rustup toolchain install`, and every tier of `gate.sh` preflights that the
active rustc IS the pin and that the wasm32 target is present, refusing up
front instead of failing deep inside a plugin test. Two rustc versions
genuinely disagree about this tree's lints (1.94.0 flagged two
`clippy::redundant_iter_cloned` in `federation_routes.rs` that 1.97.1 does
not), so bumping Rust means editing that one line, never `rustup update`.

## Branches

- `main` — stable, protected. Never push or merge directly.
- `alpha` — integration branch. All PRs target `alpha`.
- `agents/*` — throwaway worktree branches for agent/coder work.

## Releases (CalVer)

Version scheme (day-based CalVer, decided 2026-07-05; matches `maw-calver`'s
`compute_version()`). `maw-calver` lives in the external
[Soul-Brews-Studio/maw-calver](https://github.com/Soul-Brews-Studio/maw-calver)
repo (extracted 2026-07-15, repo split phase 3) and is consumed by `maw-cli`
as a rev-pinned Cargo git dependency:

```
stable:  v<YY>.<M>.<DD>                 one per day
alpha:   v<YY>.<M>.<DD>-alpha.<HMM>     HMM = H×100+M, TZ=Bangkok
beta:    v<YY>.<M>.<DD>-beta.<HMM>      independent channel
```

`HMM` is wall-clock time as a decimal integer with no leading zero (18:30 →
`1830`, 09:05 → `905`). Every minute is a unique slot — no merge-order
collisions. If `HMM` ≤ the highest existing suffix for the same base+channel,
the crate advances to the next calendar day (`next_calendar_base`).

Transition note: before 2026-07-05 the last number was a per-month release
*sequence* (SEQ-era `v26.7.2`–`v26.7.7`). Those tags were retired on
2026-07-05 (notes archived in the vault, commits untouched) and the current
line restarted day-based at `v26.7.5` (= 2026-07-05, same commit as SEQ-era
v26.7.7). The exact commit and build time are embedded in the binary
(`maw --version`) regardless of scheme.

Cut flow: PRs squash-merge into `alpha`; a release promotes `alpha` → `main`
via a **merge-commit** PR, then tags `v<YY>.<M>.<DD>` (stable) or
`v<YY>.<M>.<DD>-alpha.<HMM>` (pre-release) and publishes a GitHub release.
GitHub auto-closes `Fixes #N` only on default-branch merges, so close issues
by hand when their PR lands on `alpha`.

macOS install note: copying a new binary over an installed one can SIGKILL on
next run (stale code-sign cache on the reused inode) — `rm` first, then `cp`.

## Architecture

Layered Cargo workspace:

- **Leaf crates** — self-contained, deterministic, side-effect-free core
  logic (matching, routing, identity, transport, plugin manifest, …) with no
  internal dependencies. Eleven single-consumer leaves (auto-wake, bind,
  bring, feed, fuzzy, hub, identity, plugin-scaffold, policy, routing, split)
  were extracted to the external
  [Soul-Brews-Studio/maw-crates](https://github.com/Soul-Brews-Studio/maw-crates)
  repo (2026-07-16, repo split phase 3 batch) and are consumed by `maw-cli`
  as rev-pinned Cargo git dependencies, like `maw-calver`. `maw-discord`
  (Discord bot connectivity, zero internal `maw-*` deps) was extracted the
  same way to
  [Soul-Brews-Studio/maw-discord](https://github.com/Soul-Brews-Studio/maw-discord)
  (2026-08-13, repo split).
- **Mid crates** — compose the leaf crates (e.g. `maw-peer`, `maw-tmux`,
  `maw-worktree`).
- **Top crate** — `maw-cli`, the binary, depends on the rest of the workspace.

Run `cargo tree` for the current, authoritative dependency graph.

## Conventions

- `forbid(unsafe_code)`, clippy pedantic clean.
- Rust edition 2021.
- Behavior is validated against maw-js JSON test fixtures.
- Core crates stay deterministic and side-effect-free.
- Recursive search in Bash: always `rg` (ripgrep), never bare `grep -rn` —
  it's parallel and skips `.gitignore`/`target/` automatically. Filter with
  `rg -g '*.rs' PATTERN`; add `-u` for gitignored files. Never sweep
  `/opt/Code` with `grep -rn`. (Claude Code's Grep tool already uses ripgrep;
  this rule is for hand-written Bash.)

## Fleet Intelligence Principles

Oracle intelligence = engine × written memory × asking the right peer.

1. **SEARCH-FIRST** — before guessing, search the vault / oracle MCP, or
   `maw hey` the oracle that has actually hit the problem.
2. **WRITE-BACK** — solved something hard? Write the manual/skill immediately.
   Unwritten knowledge dies at compact; your manual is the next oracle's way out.
3. **VERIFY-DONE** — never mark done without running it; dogfood your own tools.
4. **DONE-CRITERIA TEACHING** — dispatch work with explicit gates (tests green,
   files ≤250). Clear criteria teach the receiver to own the loop.
5. **HUMILITY-COMPOUND** — model tiers change monthly; the vault compounds
   forever. The smartest oracle is the one whose peers never relearn a lesson.
6. **TEACH-DONT-EDIT** — when helping another oracle, teach and hand over the
   commands; never edit a peer's repo yourself.

## Further Docs

See `docs/` for deeper references — including the parity matrix, wire
protocol, "adding a command" guide, agent/coder team spawn conventions, and
the WASM migration design. Shipped fleet plugin artifacts (WASM ship tier,
sha256 pin lifecycle) live in the external
[Soul-Brews-Studio/maw-plugins](https://github.com/Soul-Brews-Studio/maw-plugins)
repo under `packages/<name>/` (extracted from this repo's `fleet-plugins/`
on 2026-07-15, repo split phase 1) — see its `docs/fleet-plugins.md`.
