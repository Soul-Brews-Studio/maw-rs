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

## Cargo queue rule

This repo's worktrees share cargo caches/target state. Before running test or clippy gates:

```bash
ps aux | grep '[b]in/cargo'
```

If another worktree's cargo is live, wait. This avoids shared-cache contention observed on
2026-07-06.

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

## Style

- Workspace Rust edition is 2021.
- `unsafe_code` is forbidden by workspace lint.
- Clippy pedantic warnings are enabled; the PR gate treats warnings as errors.
- New `crates/maw-cli/src/core_impl/*.rs` dispatcher files use per-file `DISPATCH_NN`
  consts. `build.rs` panics on duplicate dispatcher numbers, so renumber when parallel
  PRs collide.
- For hand-written shell search, use `rg`, not recursive `grep -rn`.

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
