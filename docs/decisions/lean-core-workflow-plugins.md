# ADR: Lean native kernel and external workflow plugins

- **Status:** Proposed for #963; acceptance is the docs/spec PR review
- **Date:** 2026-08-20
- **Source baseline:** `maw-rs/origin/alpha` `a17ad351ef9edc28dbd745c84fba584544f40966`
- **External integration branch:** `Soul-Brews-Studio/maw-plugins/main`

## Context

The native CLI owns optional workflow and vendor policy that changes independently from its trusted
orchestration kernel. `bring` and `split` have no external artifacts; native `team` shadows an
incomplete external artifact; Codex account, workflow, and launch policy is embedded in native code.
This creates duplicate ownership, release coupling, and a temptation to widen raw host authority.

## Decision

### Native trusted kernel

`wake`, `attach`/`a`, their state machines and fast paths, plugin discovery/verification, native
Extism hosting, authentication/consent enforcement, signed transport, and raw tmux/process/filesystem
adapters remain native. The plugin host and `maw-tmux` stay in their current Rust ownership domains
under #911 and #910.

All non-Codex wake/attach behavior is frozen. Provider-present Codex behavior remains at parity.
Explicit Codex selection with a missing/refused accepted provider is the only provider-cutover error
change and fails before mutation; unrelated plugin absence never changes wake/attach. Five separate
fail-closed corrections are proposed, each behind its own recorded human gate: refuse symlinks in
ordinary named-root, operator-document, and archive-tree operations instead of following/skipping
them; return errors for Team pane-inventory failure instead of treating every member as missing and
possibly fresh-waking duplicates; return errors for More/Wave pane reads instead of empty/idle
success; replace Swarm's plain state write with atomic private 0600 state; and report Codex account
probe failure as `unknown` (excluded from `--free`) instead of `free`. Without approval, the affected
slice remains blocked rather than silently changing parity.

### External product owners

| Surface | External owner |
|---|---|
| `bring`, `b` | `maw-plugins/packages/bring` |
| `split` | `maw-plugins/packages/split` |
| `team`, `t` | existing `maw-plugins/packages/team`, expanded in place |
| `gather` | `maw-plugins/packages/gather` |
| `scatter` | `maw-plugins/packages/scatter` |
| `swarm` | `maw-plugins/packages/swarm` |
| `artifacts`, `artifact` | `maw-plugins/packages/artifacts` |
| canonical `codex` (`accounts`), `more`, `wave`, Codex provider policy | `maw-plugins/packages/codex` |

`wave` has one destination only: `packages/codex`. `assign` remains generic Oracle/wake behavior;
`oracle-recruit` remains squad/federation behavior. Provider-name observation used for fail-closed
pane safety may remain native until a later metadata replacement proves equivalent.

### Host boundary

Guests receive versioned typed APIs, not raw tmux/process/environment/filesystem access. The
plugin-manifest crate owns DTOs plus an injected `WorkflowHostOps` trait; maw-cli supplies the only
production adapter for wake/done, pane, worktree, provider, and platform I/O. Team uses generic
lifecycle primitives; Team enter/send-enter and Wave's owned prompt/mission actions use pane
submission. Guests own recipient/order/render policy for state-backed messaging, but task/message/
prompt/mission/shutdown instruction bytes and destinations use exact ContentRef-bound semantic
writes; generic named-root writes cannot alter them.
Split uses `maw.tmux.split.v1`; gather/scatter use typed layout transactions;
swarm uses an operator-intent-bound batch-launch plan; team reads use bounded pane inventory; invite
uses typed consent. More/wave use typed worktree operations and provider maintenance. Provider
planning is executable-id-bound and host-to-guest; account occupancy returns opaque facts. Scoped
guests do not receive arbitrary `maw.cli.run` argv.

Every new #963 typed/effectful, changed, multi-route, or multi-workflow artifact declares a
hash-covered, non-overlapping invocation-intent table. Native dispatch derives the intent and exact
resource plan from original argv, intersects artifact grants to that intent, and enforces finite/
one-shot action budgets; an invalid intent gets no typed host authority. Immutable pre-#963
single-command artifacts retain static semantics only through a path/source/hash-bound legacy row
and only while they gain no new typed capability.

Execution and repository authority is opaque and host-issued: lifecycle/worktree/batch operations
consume engine, member, pane, existing/planned workspace, cwd, and mission-store refs instead of
guest strings. Root/host construction is resolve-only. Display paths are never accepted as
authority, and recursive removal begins only after a durable complete archive copy.

Guest-writable state cannot launder future authority. Authority-bearing or instruction-bearing
changes use closed semantic host mutations and guest-inaccessible durable records. Separate `swarm`, `team`, and `wave-codex`
epochs are locked, migrated, and sealed immediately before their native owners are removed; mutation
imports are refused while a domain is unsealed. Orphan recovery is a confirmed, audited native
`maw plugin authority reimport` operation, never a guest or arbitrary-path capability.

New complex guests use a shared Rust wrapper at `maw-plugins/crates/maw-guest-sdk` with host ABI
conformance tests. Human consent, trust mutation, secret reads, and ambiguous delivery decisions stay
native and fail closed.

The Codex artifact declares three top-level routes (`codex`, `more`, `wave`). A backward-compatible
manifest route table and host-owned `invokedCommand` lets one artifact distinguish them. Duplicate
routes inside/between runnable plugins fail; a native collision is an explicit staged-shadow state so
committed bytes can be accepted before the atomic native cutover. Legacy single-command manifests are
unchanged.

### Artifact-first delivery

For each external surface, source plus committed `plugin.wasm` merges to `maw-plugins/main`, its
manifest pin/CI/direct invocation is accepted, and only then is a fresh `maw-rs/origin/alpha` cutover
branch created. Every scoped Rust guest is built twice in the pinned clean build environment; both
outputs must be byte-identical to each other and to the committed, manifest-pinned artifact. Native
registration is removed atomically with known external ownership. Large dead source is removed
afterward in bounded deletion PRs or a separately disclosed mechanical exception.

## Consequences

- Missing optional workflows become actionable missing/refused-plugin errors instead of native
  fallbacks.
- The final Codex artifact is a prerequisite for any downstream Codex wiring/removal.
- Team cutover waits for all 43 accepted native names, including `live-check` and `delete`.
- Companion artifacts have independent product ownership; the old dispatcher lacked invoked-command
  identity, but the new multi-route contract is reserved for the intentionally combined Codex owner.
- Host-ABI work grows, but each capability is independently reviewable and reusable without moving
  trusted orchestration.

## Superseded and related decisions

This ADR deliberately supersedes native team shadowing documented by #743/#758, native `codex
accounts` placement from #273/#282, and native `more` placement from #174/#214. It coordinates with
#910, #911, and repo-split parity debt #546 without reopening their native host ownership decisions.
