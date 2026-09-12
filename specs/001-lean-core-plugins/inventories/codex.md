# Frozen inventory: Codex product policy and generic provider boundary

Baseline: `maw-rs/origin/alpha` `a17ad351ef9edc28dbd745c84fba584544f40966`.
Target owner for canonical `codex` (subcommand `accounts`), `more`, `wave`, and Codex provider policy:
`maw-plugins/packages/codex`.

The package declares three independent manifest routes. The host supplies canonical
`invokedCommand`, so empty/help/overlapping argv cannot confuse `codex`, `more`, and `wave`.

## Product code that must move

### `codex accounts`

`crates/maw-cli/src/core_impl/codex_accounts.rs` (518 lines, `DISPATCH_273`) owns the only top-level
`codex` grammar: `accounts [--json] [--free] [--slots N]`. It is read-only occupancy reporting, not
profile switching. It reads process/tmux state and platform-specific process environments internally;
raw `/proc`, `ps auxeww`, token, config, and environment bytes must never cross to the guest. Freeze
its table/JSON/free/slots/parser rows before deletion. Preserve the stale-but-observable current help
bytes for extraction parity; any wording correction belongs to a separately approved follow-up.

### `more`

`more.rs`, `more_discover.rs`, `more_spawn.rs`, and `more_status.rs` implement Codex-team
plan/spawn/status/update. They create git worktrees/markers, run provider maintenance, and call native
wake. New destinations use a non-mutating planned-workspace ref followed by native create/marker and
an issued existing-workspace ref; pane cwd strings are display-only and status inspection uses issued
cwd refs. The guest owns policy and formatting but uses typed worktree/lifecycle/provider APIs; it
never copies wake or raw Git/tmux/process code.
Tests in `native_more_codex.rs` and in-file modules are the baseline.

### `wave`

`wave.rs` (246 production lines, `DISPATCH_326`) is Codex/team lifecycle: it creates worktrees/state,
sets engine Codex, calls team up, dispatches missions, heals through wake, reports status, and tears
down. Start persists a host-issued durable repository identity; later dispatch resolves it to an
invocation-local mission-store ref instead of trusting the legacy absolute mission path. Teardown
uses digest-bound wave-state owned-member refs for workspace/branch lifecycle. It currently lacks
direct behavioral tests, so RED fixtures are mandatory before porting. Wave has exactly one external
owner: `packages/codex`.

### Provider-specific team preflight

Codex pool-auth, trust, shared-home/SQLite collision, and CODEX_HOME policy in
`team_preflight_checks.rs` moves behind typed provider health and occupancy facts. Native adapters may
read secrets/process state but return only bounded non-secret status; the guest never receives auth
JSON/config/token bytes.

### Provider command policy

Native `wake_engine_command.rs` keeps generic operator override precedence, safe token parsing, and
command resolution shared by `workon`. Built-in Codex executable selection, Codex/OMX resume
subcommand injection, channel/profile defaults, and related vendor policy move behind an
executable-id-bound provider plan. The final guest cannot choose an arbitrary executable/path,
flags outside the operation schema, or arbitrary environment names/values.

## Native behavior that remains

- wake target/session/window/worktree orchestration and launch confirmation
- attach/a dispatcher and binary fast path
- generic configured command precedence used by wake/workon
- raw process/tmux/filesystem adapters and secret reads
- conservative provider-name/prompt observation needed for fail-closed pane safety until a later
  metadata replacement proves equivalence
- generic Oracle/wake `assign`; squad/federation `oracle-recruit`

Explicit Codex with an absent/refused provider fails before mutation with repair guidance. All
non-Codex and unrelated-plugin wake/attach behavior is byte-identical without the Codex artifact.

Provider planning is host-to-guest through the accepted descriptor's dedicated export in a fresh
instance. The typed stack permits CLI workflow -> native lifecycle -> provider plan, but refuses
provider-to-lifecycle/provider recursion. `more` and `wave` require `maw.worktree.manage.v1`;
provider update uses `maw.engine.maintenance.v1`; launch/submit use injected lifecycle/pane operations.
Every contract must be present in the maw-rs ABI fixture and external Rust SDK before Codex source
work starts.

Current native `more status` turns some tmux list failures into empty success, and Wave turns pane
list/capture failures into empty/idle. The typed contracts deliberately fail closed instead. Those
are observable security corrections, not extraction parity; their child freezes the current RED and
must record explicit human approval before guest implementation. Occupancy retains display-only
`pid`/`displayHome` for frozen JSON, but neither field is accepted as authority.

## Other hardcodes requiring explicit ledger disposition

- `workon.rs` default worktree slug `codex`
- `swarm.rs` multi-provider labels/defaults (owned by the separate swarm package)
- provider labels in `census.rs`
- provider-specific retrospective selection in `worktree_finish.rs`
- conservative agent-pane names in `maw-tmux`
- cross-provider trust-prompt signatures in `wake_pane_launch.rs`

Each row must be classified as external product policy or documented vendor-neutral/safety
compatibility before final source deletion. Do not remove literals by blind search.

## Provider and occupancy contracts

An accepted provider descriptor binds `executable_id`, operation-specific argv schemas, allowlisted
bounded non-secret environment names, and a host-validated occupancy descriptor (approved executable
basenames, non-secret home marker, named root resolver, maximum slots). Native code resolves the
executable/account paths and returns opaque facts. Missing/refused/malformed provider output is
terminal before mutation and never falls back.

## Artifact/cutover gate

Accounts, more, wave, and provider code may land in external slices, but no partial artifact is used
for downstream wiring. After all modules are complete, build/pin/register/invoke/CI-validate and merge
one final combined `packages/codex/plugin.wasm`. Only then create a fresh maw-rs branch for provider
wiring, ownership cutover, and native deletion.
