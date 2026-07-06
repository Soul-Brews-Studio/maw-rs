# maw-rs

Rust port of maw-js — distributed terminal multiplexing & fleet management.
A Cargo workspace of small, focused crates. BUSL-1.1 licensed.
For repo-wide agent execution conventions, read `AGENTS.md` first; this file remains the Claude-specific memory and release detail.

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

Version scheme (day-based CalVer, decided 2026-07-05; matches `maw-calver`'s
`compute_version()`):

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
sha256 pin lifecycle) live in `fleet-plugins/` — see its README.
