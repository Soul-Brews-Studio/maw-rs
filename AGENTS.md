# maw-rs agent contract

Read this once before taking an issue. Keep changes small, verified, and sourced from repo truth.
For how-to detail, see `docs/guides/adding-a-plugin-artifact.md` and
`docs/guides/release-and-calver.md`.

## Build gate

Use the tiered gate runner (`docs/guides/gating.md` is the process doc):

```bash
scripts/gate.sh quick   # iterating / before opening a PR: fmt + clippy(stable) + affected-crate tests
scripts/gate.sh full    # before merge/promote: all 4 CI dimensions
```

`gate.sh full` runs exactly: `cargo fmt --all -- --check`,
`cargo test --workspace --locked --no-fail-fast`,
`cargo clippy --workspace --all-targets -- -D warnings`, and the wasm-host
subset (`cargo test -p maw-cli -p maw-plugin-manifest --features wasm-host
--locked --no-fail-fast` plus its clippy).

All of it runs on the toolchain `rust-toolchain.toml` pins — exact channel plus
`targets = ["wasm32-unknown-unknown"]` — which every tier preflights before
touching cargo, so a stale rustc or a missing wasm32 target is named up front
instead of surfacing as a mystery lint or a failing plugin test (#823). Do not
`rustup update` to fix a gate: edit the pin (one line, one reviewable diff, and
CI follows), or set `GATE_ALLOW_TOOLCHAIN_DRIFT=1` to state the gap out loud.

Every test dimension carries
`--no-fail-fast` (#796) so one red target cannot hide the rest — it changes
what runs, never the exit code, so a failure still fails the gate. It warm-seeds an isolated `CARGO_TARGET_DIR` from the
golden cache (`scripts/gate-cache-refresh.sh`) and locks the target dir so
two gates can never share one. Leads amortize full gates over several PRs
with `scripts/gate.sh batch <branch>...` (merge-train).

Fleet plugin artifacts live in the external
[Soul-Brews-Studio/maw-plugins](https://github.com/Soul-Brews-Studio/maw-plugins)
repo under `packages/<name>/` (extracted from this repo's `fleet-plugins/` on
2026-07-15, repo split phase 1). Plugin artifact work happens there:

```bash
maw plugin build <maw-plugins-checkout>/packages/<name>
```

The sha256 pin-hash gate (formerly `cargo test -p maw-cli --test
fleet_plugins_pin_check`) now runs in maw-plugins CI. If you rebuild an
AssemblyScript artifact, install the toolchain first with `npm ci` in this
repo's `packages/wasm-sdk`.

## Cargo isolation rule (replaces the old "cargo queue rule", 2026-07-11)

Do NOT wait for other cargo processes on the machine — the lead runs full-workspace
gates continuously and other coders run in parallel; a machine-wide queue deadlocks
everyone (observed repeatedly on 2026-07-11: coders stalled 20-45 min for nothing).

Instead, isolate your target dir on the 4 TB NVMe and run immediately:

```bash
CARGO_TARGET_DIR=/mnt/nvme1/cargo/target-omx-<your-worktree-name> cargo test ...
CARGO_TARGET_DIR=/mnt/nvme1/cargo/target-omx-<your-worktree-name> cargo clippy ...
GATE_TARGET_DIR=/mnt/nvme1/cargo/target-gate-omx-<your-worktree-name> scripts/gate.sh quick
GATE_TARGET_DIR=/mnt/nvme1/cargo/target-gate-omx-<your-worktree-name> scripts/gate.sh full
```

The only shared resource is the package cache lock, which cargo resolves itself in
seconds. The 2026-07-06 contention was shared `./target` state — fixed by the
per-worktree CARGO_TARGET_DIR above, not by queueing.

Never use `/tmp` for an active `CARGO_TARGET_DIR` or `GATE_TARGET_DIR` on this
machine: `/tmp` is on the constrained root filesystem, while `/mnt/nvme1` is
the 4 TB build volume. Keep both paths slug-specific so parallel agents cannot
cross-contaminate artifacts. Gate-owned fixture and retired-cache handling still
follows `TMPDIR` as documented in `docs/guides/gating.md`; do not move or delete
those shared paths by hand.

## Branch and PR rules

- `main` is stable/protected. Never push or merge directly.
- `alpha` is the integration branch. Open all PRs against `alpha`; squash-merge there.
- Create work branches from `origin/alpha` as `agents/<type>-<issue>-<slug>`.
- Put `Fixes #N` in the PR body.
- GitHub auto-closes issues only on default-branch merges; close issues by hand after the
  PR lands on `alpha`.

## Diff budget

Keep each PR at or below 250 changed lines, excluding lockfiles and generated
`plugin.wasm`. If the real fix must exceed that budget, say so explicitly in the PR body.

## Never touch `ψ/`

`ψ/` is the PSI vault and must not be committed. `.gitignore` must keep covering it; verify
before pushing:

```bash
grep -n '^ψ/\|^ψ/\*' .gitignore
git diff --name-only | grep '^ψ/' || true
```

## Workspace map

- Leaf crates: self-contained logic with no internal deps. Most are deterministic and
  side-effect-free; filesystem-facing leaves contain I/O within documented boundaries.
- Mid crates: compose leaves, such as the peer/tmux layers. `maw-worktree` and
  the schedule launchd/runner adapters are external rev-pinned mid crates.
- Top crate: `maw-cli`, the binary and integration surface.

New logic belongs in the lowest layer that can hold it. Keep I/O out of pure core leaves;
explicit filesystem-facing leaf adapters must contain and test their I/O. Use `cargo tree`
as the authoritative dependency graph.

## No raw tmux

Never use raw `tmux` commands (`send-keys`, `split-window`, `select-pane`, `join-pane`,
`break-pane`, `select-layout`, `rename-window`, `kill-window`, etc.) when a `maw` verb
exists. Use the maw verb instead:

| instead of raw tmux | use maw verb |
|---------------------|-------------|
| `tmux send-keys` | `maw run` / `maw hey` / `maw send-text` / `maw send-enter` |
| `tmux split-window` | `maw split` / `maw tile` / `maw new --split` |
| `tmux kill-window` | `maw kill` / `maw done` |
| `tmux new-window` | `maw new --window` |
| `tmux select-layout` | `maw layout` (#264) |
| `tmux join-pane` | `maw join` (#264) |
| `tmux break-pane` | `maw break` (#264) |
| `tmux swap-pane` | `maw swap` (#266) |
| `tmux resize-pane` | `maw resize` (#267) |
| `tmux select-pane` | `maw focus` (#267) |
| `tmux select-pane -T` | `maw rename-pane` (#267) |

If the maw verb doesn't exist yet (marked with issue #), file the gap — don't fall back
to raw tmux. The safety hook blocks `tmux send-keys` for this reason.

## Style

- Workspace Rust edition is 2021.
- `unsafe_code` is forbidden by workspace lint.
- Clippy pedantic warnings are enabled; the PR gate treats warnings as errors.
- New `crates/maw-cli/src/core_impl/*.rs` dispatcher files use per-file `DISPATCH_NN`
  consts. `build.rs` panics on duplicate dispatcher numbers, so renumber when parallel
  PRs collide.
- For hand-written shell search, use `rg`, not recursive `grep -rn`. **Never sweep the
  filesystem or ghq root** (no `grep -r`/`find`/`bfs` from `/`, `~`, or the code root
  wholesale — 3 machine-freezing incidents, 2026-07-09). Find a repo:
  `ghq list | rg <name>` or `ls -d "$(ghq root)"/github.com/*/<name>*` (ghq root varies
  per machine — m5=/opt/Code, MBA=~/Code — always resolve via `$(ghq root)`). Find a
  file: `git -C <repo> ls-files | rg <name>`. Content: `rg` in the narrowest dir.

## Fixtures

Observable behavior is validated against maw-js JSON fixtures. When behavior changes,
update or add fixtures; never delete fixtures just to make tests pass.

## Fleet intelligence principles

Oracle intelligence = engine × written memory × asking the right peer.

1. **SEARCH-FIRST** — before guessing, search the vault / Oracle MCP, or ask the
   peer that has actually hit the problem.
2. **WRITE-BACK** — solved something hard? Write the manual or skill immediately.
   Unwritten knowledge dies at compact; the manual is the next Oracle's way out.
3. **VERIFY-DONE** — never mark done without running it; dogfood your own tools.
4. **DONE-CRITERIA TEACHING** — dispatch work with explicit gates and limits.
   Clear criteria teach the receiver to own the loop.
5. **HUMILITY-COMPOUND** — model tiers change monthly; written knowledge
   compounds. The smartest Oracle is the one whose peers never relearn a lesson.
6. **TEACH-DONT-EDIT** — when helping another Oracle, teach and hand over the
   commands; never edit a peer's repository yourself.

## Reporting

Done reports go to the lead window, usually:

```bash
maw hey 33-maw-rs:1 "done #N PR <url> gates green: <exact commands>; root cause: <summary>"
```

Use the current session lead if it differs. Include the PR link, exact gate evidence, and
root cause for bug fixes.
