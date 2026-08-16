# Gating — the tiered dev-loop gate

One entry point for every agent and lead: **`scripts/gate.sh`**. It exists
because of a measured problem (m5, 2026-07-16): under 3–5 concurrent
per-worktree gates, each cold full gate took **17–46 min** wall clock and
**12–40 GB** of disk, with cold compile of ~194 unchanged crates as the top
sink. gate.sh attacks that with tiers, a golden warm cache, and merge-train
batching — without moving the merge decision to CI (Nat's policy: **merge
decision = local gate; CI is a 4-hourly safety net only**).

## Tiers

| tier | command | when | what runs |
|---|---|---|---|
| quick | `scripts/gate.sh quick` | while iterating; before opening a PR | `fmt --all --check` + `clippy` (stable, `-D warnings`) + `cargo test -p <crate>` for only the crates your diff vs `merge-base(origin/alpha)` touches. Root `Cargo.toml`/`Cargo.lock` changes escalate to workspace tests; changes outside `crates/` (docs, scripts, CI) skip tests. |
| full | `scripts/gate.sh full` | before merge/promote/release | all 4 CI dimensions (below) |
| batch | `scripts/gate.sh batch <br>...` | lead, landing several PRs | merge-train: throwaway worktree at `origin/alpha`, merge every listed branch, run **one** `full` on the combined tree |

## The 4-dimension CI trap

"Green locally" has burned us when local ran fewer dimensions than CI
(`reference_local-gate-vs-ci-gate`). A change is CI-safe only when ALL of
these pass — `gate.sh full` runs exactly this set:

| # | dimension | command |
|---|---|---|
| 1 | format | `cargo fmt --all -- --check` |
| 2 | workspace tests | `cargo test --workspace --locked --no-fail-fast` |
| 3 | clippy, stable **and** `1.97.0` (CI installs `stable`, which is currently 1.97.0) | `cargo clippy --workspace --all-targets -- -D warnings` (×2 toolchains; a missing 1.97.0 **fails** the full gate unless `GATE_ALLOW_MISSING_197=1` accepts the gap explicitly) |
| 4 | wasm-host subset | `cargo test -p maw-cli -p maw-plugin-manifest --features wasm-host --locked --no-fail-fast` + the matching clippy |

Every test dimension runs with **`--no-fail-fast`** (#796). Without it cargo
halts at the first failing test target, so a red run shows one failure and
hides the rest — the fixer then discovers the next one only on the following
round trip. `--no-fail-fast` changes *what runs*, never the exit code: cargo
still exits non-zero if any test failed, so the gate still fails.

The quick tier deliberately runs **less** — that is fine for iteration, but
the full set must still run somewhere before anything is promoted: either
`gate.sh full` on your branch or a `gate.sh batch` train that includes it.
Never silently drop a dimension.

## Merge-train (the process change)

Per-PR full gates are what melted m5: N agents × ~194 cold crates × shared 18
cores ⇒ contention multiplier ≈ N. Instead:

1. Authors get **quick** green on their PR and hand it to the lead.
2. The lead batches open PRs: `scripts/gate.sh batch agents/foo agents/bar ...`
   — one worktree, one warm-seeded target dir, one full gate amortized over
   the whole train.
3. Train green → merge the batch into `alpha`; train red → bisect by
   re-running with fewer branches (the failing worktree is kept for
   inspection).
4. Promote/release still requires a full gate on the final `alpha` tip.

## Worktree base resolution (`maw worktree`)

`maw worktree` no longer hardcodes `origin/alpha` (#749). Both the default `add
--base` and the `merged?` column that `ls` shows and `clean` acts on come from the
same resolution, always run against the **primary worktree** so the answer does not
change with the caller's cwd:

1. `git rev-parse --abbrev-ref --symbolic-full-name @{upstream}` — the tracking
   upstream of the primary worktree's branch. This is what makes maw-rs resolve
   `origin/alpha` rather than its `main` default branch.
2. `git symbolic-ref --quiet --short refs/remotes/origin/HEAD` — the remote default
   branch, for repos whose primary worktree has no tracking upstream.
3. Neither resolved ⇒ the base is *unresolved*. `add` still tries `origin/alpha` (git
   reports a clear error if it does not exist), while `ls` reports every branch as
   `unmerged` and `clean` removes nothing. An unproven base never deletes a worktree.

Pass `maw worktree add <name> --base <ref>` to bypass resolution entirely; an explicit
base issues no probe at all.

## Golden warm cache

`scripts/gate-cache-refresh.sh` builds workspace test + clippy artifacts,
the wasm-host subset, and (when installed) the 1.97.0 clippy artifacts at the
`origin/alpha` tip into `~/.maw/gate-cache/<sha>/target` — with
`CARGO_INCREMENTAL=0`, since incremental state is per-worktree-path and only
bloats the clone. Routine refreshes clonefile-seed from the newest cached sha,
so they only recompile what the alpha tip changed. `gate.sh` warm-starts by
**APFS clonefile** (`cp -Rc`): copy-on-write, so a clone costs ~0 extra disk
until a gate diverges, and clone time scales with file count, not GB.

Measured on m5 (2026-07-16, alpha @ `48d649e4`, machine otherwise idle):
clonefile of a 14G incremental-laden target dir took **104s**; the first
`cargo build --workspace --tests --locked` on that clone from a *different*
worktree path finished in **6m18s with only 13 crates Compiling** — every
registry/git dep (the ~180-crate bulk, incl. the extism stack) stayed Fresh.
The same build cold under the observed 3–5-way contention: **17–46 min**.
Only the in-repo workspace crates recompile on a cross-worktree clone (their
fingerprints embed the worktree path); dep artifacts are path-independent.

Rules the cache lives by:

- **One-gate-per-target-dir** (`feedback_one-gate-per-target-dir`): concurrent
  test runs on a shared live target dir cross-contaminate at test-execution
  time (phantom FAILEDs from the other tree). gate.sh therefore NEVER shares a
  live target dir: each invocation clones into its own isolated
  `CARGO_TARGET_DIR` (default `<worktree>/target-gate`) and guards it with a
  pid lock — a second gate on the same dir refuses to start.
- The golden dir itself is only ever a **clone source**, never a build dir
  (the refresh script builds into a `.partial` and renames atomically).
- **Disk story**: golden dirs are GB-scale, so they live under one dedicated
  root (`~/.maw/gate-cache/`), the refresh keeps only the **2 newest** shas,
  and retired dirs are `mv`'d to `${TMPDIR:-/tmp}/gate-cache-trash-*` (no
  `rm`; the July disk crisis was untracked target-dir sprawl). Reap your own
  merged worktrees' `target-gate` dirs the same way: `mv` to /tmp.
- Refresh cadence: on demand, or schedule it (no in-repo `.maw/schedule.toml`
  exists yet), e.g.
  `0 * * * * <repo>/scripts/gate-cache-refresh.sh` or
  `maw schedule add gate-cache-refresh --cron "0 * * * *" -- <repo>/scripts/gate-cache-refresh.sh`.

## Don'ts

- Don't run two gates against one target dir "to save disk" — that is the
  exact scar the lock exists for.
- Don't wait/queue machine-wide on other cargo processes (AGENTS.md cargo
  isolation rule) — isolate and run.
- Don't treat a green **quick** as mergeable-to-promote evidence — it is an
  iteration signal.
- Don't gate in CI. CI (`.github/workflows/ci.yml`, 4h schedule + dispatch)
  is the safety net that catches what a local gate missed, never the decision
  maker.
