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
| 3 | clippy on the **pinned** toolchain | `cargo clippy --workspace --all-targets -- -D warnings` (one run; `rust-toolchain.toml` makes it the same rustc CI runs, and the preflight below proves it) |
| 4 | wasm-host subset | `cargo test -p maw-cli -p maw-plugin-manifest --features wasm-host --locked --no-fail-fast` + the matching clippy |

Every test dimension runs with **`--no-fail-fast`** (#796). Without it cargo
halts at the first failing test target, so a red run shows one failure and
hides the rest — the fixer then discovers the next one only on the following
round trip. `--no-fail-fast` changes *what runs*, never the exit code: cargo
still exits non-zero if any test failed, so the gate still fails.

## The toolchain preflight (#823)

Before #823 the gate's verdict depended on **whose machine ran it**, two ways.
Both were measured on `alpha @ 5a30acab`:

| drift | symptom |
|---|---|
| rustc version | 1.94.0 reported 2 `clippy::redundant_iter_cloned` errors in `crates/maw-cli/src/serve_core/modules/federation_routes.rs` (`:294`, `:407`); 1.97.1 reported 0. Same commit. Both called themselves `stable` — a box that had not run `rustup update` since March was still on 1.94.0, while a fresh `rustup toolchain install stable` (what CI does) got 1.97.1. |
| `wasm32-unknown-unknown` | `maw plugin build` shells out to `cargo build --release --target wasm32-unknown-unknown`, which exits 101 without that rust-std, so `plugin_build_route_a_builds_dist_and_extism_loads_fixture` fails deterministically — far into `cargo test --workspace`, as a subprocess failure that reads like a code bug. `ci.yml` added the target by hand in *both* Rust jobs; nothing told a developer. |

**`rust-toolchain.toml` at the repo root now pins both** — channel, components,
and `targets = ["wasm32-unknown-unknown"]`. `ci.yml` installs it with a bare
`rustup toolchain install` (no arguments = "install what the file says"), so CI
and every worktree judge with the same rustc. Any `cargo`/`rustup` command run
inside the checkout provisions it, including the target.

The channel is an **exact version, not `stable`**, on purpose: rustup installs a
*missing* toolchain named by the file but never *updates* an already-installed
`stable`, so `channel = "stable"` would have left the March box on 1.94.0 and
changed nothing. Bumping Rust is therefore a one-line, reviewable diff that
moves CI and the whole fleet together.

The pin reaches the plugin build too, which is not obvious: that inner `cargo
build` runs in a temp dir far outside the checkout, so a toolchain *file* would
never apply to it. It works because rustup's `cargo` shim exports
`RUSTUP_TOOLCHAIN` into every process cargo spawns, and that variable outranks
directory lookup. Measured: `cargo probe` inside the checkout reports
`RUSTUP_TOOLCHAIN=1.97.1-…` and a child running `rustc --version` from `/tmp`
answers 1.97.1; the same probe outside answers `stable-…` / 1.94.0. So the
target the preflight checks — the pinned toolchain's — is the one the fixture
actually cross-compiles with.

`gate.sh` preflights both facts before running **any** cargo step, in every
tier, and refuses with the fix command rather than failing deep inside a plugin
test. `GATE_ALLOW_TOOLCHAIN_DRIFT=1` skips the preflight and says out loud that
the run no longer reproduces CI. (It replaces `GATE_ALLOW_MISSING_197`, which is
now dead: dimension 3 used to run clippy twice — once on whatever `stable` meant
locally, once on a hardcoded `+1.97.0` guess at CI's stable that was itself
stale — and hard-failed any machine without 1.97.0 installed. One run under the
pin certifies the dimension exactly; only the guess was dropped.)

`scripts/test-gate-preflight.sh` covers this. It is a standalone shell test —
no cargo, no network, seconds to run — in the same style as
`scripts/test-install-resolve.sh`. Run it after touching `gate.sh`'s preflight
or bumping the pin.

## The repository-hygiene preflight (#888)

Before the toolchain check, `quick` and `full` refuse when a tracked file
matches the repository-controlled root `.gitignore`. Ignore rules do not stop
Git tracking a path that a stale branch reintroduces; tests can then append to
runtime state such as `.maw/audit.jsonl` and leave each worktree that runs the
tests dirty. The check fails before cargo and tells the author to untrack the
path or narrow the rule. `batch` gets the same protection from the merged
train's inner `full`.

Only the root `.gitignore` defines this repository invariant. Developer-local
`.git/info/exclude` and global ignore files cannot change the verdict, while
the committed `!**/.maw/teams/` exception keeps team charters trackable. The
toolchain-drift escape hatch does not bypass repository hygiene.

`scripts/test-gate-preflight.sh` pins the quick/full refusal, fail-closed Git
errors, the team-charter exception, and local-exclude non-interference.

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
and the wasm-host subset at the
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
  `CARGO_TARGET_DIR` (default `/mnt/nvme1/cargo/target-gate-<worktree>`) and guards it with a
  pid lock — a second gate on the same dir refuses to start.
- The golden dir itself is only ever a **clone source**, never a build dir
  (the refresh script builds into a `.partial` and renames atomically).
- **Disk story**: golden dirs are GB-scale, so they live under one dedicated
  root (`~/.maw/gate-cache/`), the refresh keeps only the **2 newest** shas,
  and retired cache dirs are `mv`'d to `${TMPDIR:-/tmp}/gate-cache-trash-*` (no
  `rm`; the July disk crisis was untracked target-dir sprawl). Reap your own
  merged worktrees' active gate dirs stay on NVMe and retire under
  `/mnt/nvme1/cargo/retired-gates/` when that volume is available.
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
