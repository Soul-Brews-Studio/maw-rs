# maw-rs agent contract

Read this once before taking an issue. Keep changes small, verified, and sourced from repo truth.
For how-to detail, see `docs/agent-guides/adding-a-plugin-artifact.md` and
`docs/agent-guides/release-and-calver.md`.

## Build gate

Every PR must be green on:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Plugin artifact work also needs:

```bash
maw plugin build fleet-plugins/<name>
cargo test -p maw-cli --test fleet_plugins_pin_check
```

If you intentionally run the ignored deterministic rebuild check, install the AssemblyScript
toolchain first with `npm ci` in `packages/wasm-sdk`.

## Cargo isolation rule (replaces the old "cargo queue rule", 2026-07-11)

Do NOT wait for other cargo processes on the machine — the lead runs full-workspace
gates continuously and other coders run in parallel; a machine-wide queue deadlocks
everyone (observed repeatedly on 2026-07-11: coders stalled 20-45 min for nothing).

Instead, isolate your target dir and run immediately:

```bash
CARGO_TARGET_DIR=/tmp/maw-rs-target-<your-worktree-name> cargo test ...
CARGO_TARGET_DIR=/tmp/maw-rs-target-<your-worktree-name> cargo clippy ...
```

The only shared resource is the package cache lock, which cargo resolves itself in
seconds. The 2026-07-06 contention was shared `./target` state — fixed by the
per-worktree CARGO_TARGET_DIR above, not by queueing.

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

- Leaf crates: deterministic, side-effect-free core logic with no internal deps.
- Mid crates: compose leaves, such as peer/tmux/worktree layers.
- Top crate: `maw-cli`, the binary and integration surface.

New logic belongs in the lowest layer that can hold it. Keep I/O out of leaf crates. Use
`cargo tree` as the authoritative dependency graph.

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

## Reporting

Done reports go to the lead window, usually:

```bash
maw hey 33-maw-rs:1 "done #N PR <url> gates green: <exact commands>; root cause: <summary>"
```

Use the current session lead if it differs. Include the PR link, exact gate evidence, and
root cause for bug fixes.
