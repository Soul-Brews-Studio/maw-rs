# Phase 0 Research: Lean-Core Workflow Plugins

All decisions below are based on `origin/alpha` at
`a17ad351ef9edc28dbd745c84fba584544f40966`, Serena symbol/caller audits, existing
fixtures, and the current `Soul-Brews-Studio/maw-plugins/main` tree.
The refreshes from the initial `f1660ab4` audit changed serve websocket code and then landed
#960's confirmed/ambiguous send-key result at `a17ad351`. The latter closes the delivery prerequisite
for Team pane submission; neither refresh changed a scoped owner, caller, fixture, or capability
boundary.

## Decision 1: Use a paired-repository artifact-first train

**Decision**: Guest source and `plugin.wasm` land first through a PR to
`maw-plugins/main`. Only the final merged artifact is accepted. A fresh downstream branch from
then-current `maw-rs/origin/alpha` performs dispatch cutover/removal.

**Rationale**: The committed artifact, not a native implementation or plugin PR head, is what users
execute. Artifact-first order prevents a native verb disappearing before a verifiable replacement
exists.

**Alternatives considered**: One cross-repo branch (no atomic merge); native removal first (unsafe);
moving artifacts back into maw-rs (violates the 2026-07-15 repository split).

## Decision 2: Bring is the MVP and remains plan-only

**Decision**: Port the exact native `bring`/`b` parser/render contract as a capability-free plugin.
Do not make it execute wake or turn the currently inert `wake --split` flag into behavior.

**Rationale**: Native bring only renders a `wake ... --split` plan. It has no effectful caller and is
the smallest complete proof of aliases, help, missing-plugin guidance, artifact parity, and cutover.

**Alternatives considered**: Delegating to `maw.cli.run wake` (behavior expansion and still would not
split); migrating all `maw-bring` policy helpers (unused by native CLI); retaining a native shim
(would leave duplicate ownership).

## Decision 3: Solve split flag ownership before the split artifact

**Decision**: Add manifest-declared ownership for plugin-first `-v` and help behavior, then port split.
Plugins without declarations retain universal `-v`/help behavior.

**Rationale**: The current plugin runtime consumes a first `-v` as plugin version while native split
means vertical. Removing native split without fixing dispatch would silently run the wrong action.

**Alternatives considered**: Breaking `-v` compatibility (rejected); heuristics based on plugin name
(non-extensible); forcing `--vertical` only (observable regression).

## Decision 4: Split uses a typed mutation boundary

**Decision**: Use only a typed `maw.tmux.split.v1` action with validated target, orientation,
percentage, and optional command.

**Rationale**: The current `maw.tmux.command` allowlist cannot express native split safely. Typed
fields make validation and no-mutation failure auditable.

**Alternatives considered**: Widening raw `maw.tmux.command` (rejected); shelling out to tmux (forbidden);
keeping split native (contradicts target ownership).

## Decision 5: Expand the existing team package in place

**Decision**: `maw-plugins/packages/team` is the only `team`/`t` destination. Freeze current alpha as
the behavior authority, reconcile its 43 canonical/alias names against the external 23-name stale
contract, and implement bounded stories while native dispatch still shadows it.

**Rationale**: A second team package would deepen operator confusion recorded in #743. The existing
artifact already proves read-only list/ls and declares the correct canonical verb/alias.

**Alternatives considered**: New `team2` package (duplicate ownership); immediate cutover (breaks most
subcommands); leaving native team indefinitely (contradicts #963).

## Decision 6: Inject generic typed workflow operations; do not expose arbitrary CLI argv

**Decision**: `maw-plugin-manifest` defines DTOs plus an injected `WorkflowHostOps` trait; maw-cli
implements that trait without creating a reverse crate dependency. Team and companion guests use
generic, versioned workflow lifecycle and pane-submit operations, while layout, batch launch,
worktree, provider-health, and maintenance behavior use their separately bounded typed surfaces.
Mailbox send/msg/broadcast/inbox remain named-root state operations, but every instruction-bearing
task/message/prompt/mission/shutdown write consumes an exact ContentRef and is materialized by the
host; generic writes cannot substitute its bytes. Team enter/send-enter and
Wave's state-owned update-prompt Enter/mission pointer are the only pane-submit consumers. Native
code owns wake/done/session/tmux/process composition, validates all targets before
mutation where promised, bounds time/output, rejects recursive plugin dispatch, and returns
confirmed/failed/ambiguous outcomes. No #963 guest receives arbitrary `maw.cli.run` argv.

**Rationale**: Exact command-name capability alone is insufficient because verbs such as `wake` and
`new` accept command-bearing options and broad targets. The injected trait keeps the dependency
graph acyclic while typed operations preserve trusted native mechanisms without giving a guest a
command-construction escape hatch or misclassifying state messages as pane delivery.

**Alternatives considered**: raw `maw.cli.run` argv (rejected); porting wake/done into a guest
(rejected duplicate); raw tmux/process access (rejected).

## Decision 7: Add named, atomic state APIs and typed consent

**Decision**: Add generic named roots for team state and atomic mode-0600 writes/renames. Instruction-
bearing state uses ContentRef-bound semantic writes plus durable content digests. Add a typed
consent-request operation that returns a bounded result but never exposes trust-store paths or
credentials. State APIs preserve unknown JSON bytes when the native contract does.

**Rationale**: Existing guest writes truncate directly, lack rename, cannot reach required state
roots, and intentionally cannot mutate consent/trust. Native team depends on atomic private writes
and human consent.

**Alternatives considered**: `fs:write:vault` (too broad); direct consent-pending file edits (security
regression); lossy typed JSON rewrites (break unknown-field preservation).

## Decision 8: Extract companions before deleting shared team helpers

**Decision**: `gather`, `scatter`, `swarm`, and `artifacts`/`artifact` receive separately addressable external
packages and explicit downstream cutovers. `wave` is Codex/team lifecycle behavior but has exactly
one target owner, `packages/codex`. Shared consumers (`maw ls`, `oracle-recruit`, and native wave
before its later Codex cutover) receive small generic/local helpers before native team deletion.

**Rationale**: Native team functions are called outside `team_*.rs`; deleting by filename would break
unrelated commands. The baseline dispatcher also did not preserve which alias invoked an empty
argument list; the new multi-route identity is used only where one accepted owner is intentional.

**Alternatives considered**: Mechanical delete first (compile/runtime break); silently dropping
companions (scope loss); bundling gather/scatter as indistinguishable aliases (wrong behavior).

## Decision 9: Codex becomes an external provider, not a raw-process guest

**Decision**: Native wake/attach retain a vendor-neutral provider interface and orchestration state
machine. Codex command construction, resume/profile/account policy, `codex accounts`, `more`, and
`wave` move to external packages. The host exposes bounded provider planning and account-occupancy
facts, never raw environments or tokens. The accepted provider descriptor binds an executable
identity and operation-specific argv grammar; a provider cannot select an arbitrary program. Account
occupancy is driven by a manifest-declared, host-validated generic descriptor. Native code resolves
approved account roots and reads process state internally, returning only opaque occupancy facts.

**Rationale**: The user requires all Codex product policy out of core, while wake/attach must remain
working. A provider contract is the only boundary satisfying both without moving host authority.

**Alternatives considered**: Keeping engine-specific branches native (fails lean-core target);
exposing `/proc/*/environ` to a guest (secret leak); moving the entire wake state machine into a
plugin (violates stable kernel).

## Decision 10: Missing/refused aliases are first-class contracts

**Decision**: Extend known-verb metadata to include aliases and ownership. Help, completions,
plugin-list, doctor, and missing/refusal messages read the same ownership source or are guarded by a
convergence test.

**Rationale**: Static known verbs currently omit bring/split aliases, plugin manifests may be absent,
and native-only completion enumerators would lose extracted surfaces.

**Alternatives considered**: Generic unknown-command output (unsafe/misleading); duplicating lists
without a parity guard (drift); keeping native no-op registrations only for help (shadowing).

## Decision 11: Treat external artifact parity as new infrastructure

**Decision**: Restore host-side invocation coverage against committed artifacts and raw manifest
contracts as a foundational task. Do not infer parity from old in-repo fleet-plugin tests or source
tests alone.

**Rationale**: #546 records repo-split test debt, and the external team package itself documents that
native dispatch still owns production. Generated bytes and capability grants must be exercised.

**Alternatives considered**: Trusting source compilation/hash only (does not test host runtime);
testing dev-Bun fallback only (not shipped); testing native output only (does not test artifact).

## Decision 12: Spec Kit drives issue slices, but issues are curated manually

**Decision**: Check in Spec Kit constitution/spec/plan/tasks/analyze artifacts. Create an umbrella and
bounded child issues manually from reviewed tasks rather than running vanilla task-to-issues.

**Rationale**: The stock task-to-issues workflow emits flat per-task issues and does not encode
paired repositories, artifact-first sequencing, parent ledgers, or maw-rs gate rules.

**Alternatives considered**: One epic issue only (not executable); one issue per raw task (too noisy
and loses story boundaries); untracked local planning (violates write-back and auditability).

## Decision 13: Extend result/context/pane contracts without exposing ambient host state

**Decision**: Add an optional validated CLI `exitCode`, typed safe invocation context fields, typed
pane inventory plus manifest-bound boolean observations, injected generic lifecycle/pane/layout/
batch/worktree/repository operations, named-root archive/remove, operator-document/value receipts,
peer trust facts, typed provider health/maintenance, and a host-validated account-occupancy
descriptor. DTOs and host-operation traits live in `maw-plugin-manifest`; maw-cli supplies the
production adapter so dependency direction stays acyclic. Do not add a general environment reader,
arbitrary `maw.cli.run` argv, raw capture text, or raw tmux format command.

**Rationale**: Native team has meaningful exit-2 states, reads a few semantic environment/session
values, and requires pane id/command/path data. The current guest result always maps failure to exit
1, current context omits these fields, and the safe tmux list is too narrow.

**Alternatives considered**: Accepting exit drift (breaks parity); `maw.env.get` over arbitrary names
(ambient secret exposure); widening raw tmux command/format allowlists (unbounded host surface).

## Decision 14: Use Rust WASM for new complex workflow artifacts

**Decision**: Build bring/split and the expanded team/provider artifacts as self-contained Rust
Extism guests. Rewrite the existing team package in place only after the current list/ls artifact
contract is frozen and differentially reproduced; keep package name, canonical verb, alias, and
artifact pin continuity. All Rust guests share `maw-plugins/crates/maw-guest-sdk`, whose DTOs/import
wrappers are tested against the maw-rs host ABI contract. Package-local handwritten host FFI is not
duplicated for the new typed surfaces.

**Rationale**: Full team/provider parity needs structured parsing, versioned DTOs, and many tests.
The external repository already ships Rust guests, while the current compact AssemblyScript team
slice is intentionally read-only and is not a maintainable base for the 6k-line native surface.

**Alternatives considered**: Grow the minified AssemblyScript slice (high drift/weak typing); Bun
dev-tier as production (not the shipped rung); create a second team package (duplicate ownership).

## Decision 15: Multi-route artifacts receive host-owned invoked-command identity

**Decision**: Extend manifests with an optional ordered multi-command route table while preserving
legacy single-command manifests. The dispatcher validates canonical/alias uniqueness and supplies an
unforgeable `invokedCommand` in the guest context. `packages/codex` uses three routes (`codex`, `more`,
`wave`) that share one accepted artifact but remain independently diagnosable.

**Rationale**: Current dispatch strips the matched top-level name, so aliases cannot distinguish
empty/help/overlapping argv. A route identity is required for one Codex-family artifact to own three
top-level commands without heuristics.

**Alternatives considered**: Treating `more`/`wave` as aliases without identity (wrong behavior);
three duplicated packages/artifacts (source/pin drift); keeping native shims (shadow ownership).

## Decision 16: Staged native shadows are accepted, runnable plugin collisions are not

**Decision**: The optional `cli.routes` schema is mutually exclusive with legacy `cli.command` and
contains complete route objects. Duplicate canonical/alias names within a manifest or among runnable
plugins are refused. A name still owned by native dispatch is loaded as an explicit staged shadow,
visible to diagnostics and direct artifact tests, until the atomic cutover removes native dispatch.

**Rationale**: Artifact-first acceptance deliberately requires the new plugin to coexist with the
old native owner. Refusing every native collision would break the already-installed shadowed team
artifact and make a verified cutover impossible.

**Alternatives considered**: Refuse native collisions (breaks staging); allow two runnable plugins
(nondeterministic); native no-op shims after cutover (duplicate ownership).

## Decision 17: Provider calls and workflow host operations have opposite, explicit directions

**Decision**: CLI workflow guests call versioned host operations backed by an injected maw-cli
`WorkflowHostOps` adapter. Engine provider planning is host-to-guest through a dedicated provider
export in a fresh plan-only plugin instance whose CLI-route capabilities are refusal stubs. A typed
surface stack permits `CLI wave -> lifecycle -> provider` but rejects provider-to-host-operation,
provider-to-provider, and same-surface recursion. Path environment values use request-issued opaque
root ids. Custom swarm executables receive EngineRefs only from reviewed original operator argument
roles; defaults use an accepted provider or a frozen native built-in executable id.

**Rationale**: `maw-plugin-manifest` cannot depend on maw-cli, while native wake/done/worktree/tmux
mechanics must remain trusted. Directional APIs preserve the dependency graph and prevent a plugin
from inventing executable authority or recursively driving itself.

**Alternatives considered**: self-spawn through `maw.cli.run` (unbounded/recursive); copying native
orchestration into guests (drift); raw process/tmux capabilities (security regression).

## Decision 18: Preserve operator input through invocation-scoped authority, not ambient access

**Decision**: The host owns one shared per-invocation authority set. It issues pane/cwd ids,
route-and-argument-role document receipts, workspace/planned-workspace refs, engine refs,
lifecycle-member refs, mission-store refs, charter/archived scalar receipts, and provider-root ids;
later operations accept only the expected kind/key from that invocation. Named roots use a primary
plus ordered read-only fallbacks and closed archive/copy/remove. Repository issue context and peer
trust are native bounded facts, not guest process/config access.

**Rationale**: Team charters intentionally contain operator-authored engine commands, reassign reads
GitHub issue text, adopt checks existing trust, and layout mutates live panes. Syntax validation alone
would let a guest invent authority, while removing these behaviors would violate the frozen contract.

**Alternatives considered**: ambient file/git/gh/trust/tmux access (rejected); duplicating all product
policy natively (defeats extraction); dropping charter/reassign/adopt/layout parity (unapproved drift).

## Decision 19: Bootstrap a capability-free ABI before the complete typed ABI

**Decision**: Freeze and vendor a minimal invoke/context/result/route ABI, Rust guest SDK, registry
generator, native-test/wasm CI loop, and committed-artifact harness before any typed workflow host
surface. Bring consumes only this base. Typed surfaces then land independently, followed by one
complete ABI fixture and bounded SDK extensions before Split/Team/Codex consume them.

**Rationale**: Bring is the requested earliest proof and needs no host operation. Making it wait for
every filesystem/tmux/provider security surface would front-load the largest risk and provide no
incremental product evidence. The complete ABI gate still prevents a typed guest racing ahead of its
native implementation.

**Alternatives considered**: one monolithic ABI/SDK freeze before any artifact (unnecessarily blocks
Bring); per-package handwritten FFI (drift); allowing typed guests against provisional imports
(unverifiable).

## Decision 20: Bind execution and repository authority with opaque refs

**Decision**: Lifecycle and worktree operations never accept guest engine, member, pane, repository,
or path strings as authority. Native context/document/named-state/provider resolution issues typed
`EngineRef`, `LifecycleMemberRef`, `WorkspaceRef`, and non-mutating `PlannedWorkspaceRef` values.
More and Wave create deterministic worktrees through plan -> revalidate -> create, then receive an
existing WorkspaceRef. Wave persists a host-issued non-authority repository identity and resolves it
to a fresh mission-store ref on later invocations.

**Rationale**: Native workflows accept operator-authored engine/cwd fields and compute new worktree
destinations, but a compromised guest must not turn those fields into arbitrary process/Git/fs
authority. Invocation-scoped refs preserve valid behavior and make cross-member/repository/stale
misuse testable.

**Alternatives considered**: validated guest strings (forgeable semantic authority); absolute paths
or raw Git/process argv (unbounded); requiring every new worktree to exist before planning
(impossible for More/Wave).

## Decision 21: Archive and display contracts are explicit non-authority outcomes

**Decision**: Host construction/root resolution performs no writes. Reviewed named-root operations
may return bounded display-only canonical paths needed by frozen output, but no request accepts them.
Archive copy must durably complete before source removal starts; copy failure leaves all source bytes
untouched, while post-commit removal may be partial/ambiguous. One invocation clock supplies every
guest timestamp. Ordinary named-root/operator-document/archive-tree symlink hardening, fail-closed
Team inventory errors (rather than missing/fresh-wake behavior), fail-closed More/Wave pane-read
errors, Swarm atomic-private-0600 state, and account-probe `unknown` (rather than `free`) are five
intentional security corrections that each require separate explicit human approval and frozen RED
evidence.

**Rationale**: Native output exposes selected paths and shutdown has archive-before-cleanup semantics.
Making display strings reusable would reopen ambient fs authority, and removal before durable copy
would risk data loss. Security-driven parity changes cannot be smuggled into a mechanical extraction.

**Alternatives considered**: hide all paths (breaks frozen UX); guest timestamps/WASI clock
(nondeterministic ambient host access); best-effort pane/fs errors (unsafe silent success); unreviewed
behavior correction (violates the constitution).

## Decision 22: Source-proven means byte-reproducible for scoped Rust guests

**Decision**: Pin the Rust/wasm toolchain and complete build environment used by every new #963
guest. External CI performs two clean builds, requires their `plugin.wasm` bytes to be identical,
and requires that digest to equal the committed artifact and manifest pin. The toolchain, commands,
source SHA, build-log hashes, and artifact SHA-256 are immutable acceptance evidence.

**Rationale**: A source tree plus an independently pinned opaque binary does not prove that the
binary came from that source. The constitution requires reproducible artifacts, so procedural build
attestation without byte equality is insufficient for these new Rust packages.

**Alternatives considered**: source tests plus pin check only (does not connect source to bytes);
one successful rebuild (does not prove determinism); normalizing or rewriting the artifact after
build (would obscure the executable bytes under review).

## Decision 23: Static grants are narrowed by host-derived invocation intent

**Decision**: Every new #963 typed/effectful, changed, `cli.routes`, or multi-workflow artifact
declares a hash-covered, non-overlapping intent grammar. Native dispatch derives the canonical intent from original argv, intersects the
artifact grant to the intent's exact capabilities, evaluates its target/include/exclude/resource
plan, and gives unmatched or ambiguous input no typed host authority. Each mutating action also has
a mandatory host-enforced call budget; request refs are consumed atomically so confirmed, failed,
or ambiguous work cannot be replayed through another cloned host handle. An immutable pre-#963
`cli.command` artifact retains static legacy grants only while a recorded path/source/hash matches
and it receives no new typed capability; changing/copying it loses eligibility.

**Rationale**: An artifact-wide `terminate` or filesystem grant would otherwise let `team list`,
`more status`, or `wave status` perform a different valid workflow. Target refs alone also do not
prevent the guest from ignoring `--only`/`--keep` or invoking the same split/submit/update twice.

**Alternatives considered**: trust guest subcommand routing (not a security boundary); static exact
action names without intent/resource scope (confused deputy); retryable mutation tokens (duplicate
side effects after ambiguous outcomes).

## Decision 24: Guest-writable state never bootstraps durable authority

**Decision**: Generic state writes preserve a closed authority/content projection. Authority- or
instruction-bearing changes
use typed semantic host mutations that consume issued receipts/outcome refs and commit a
guest-inaccessible durable record with the state. `swarm`, `team`, and `wave-codex` have separate
locked migration epochs sealed immediately before their native owners are removed; mutation imports
remain unavailable before that domain is sealed. Post-seal missing or mismatched records fail closed.
An explicit confirmed/audited native `maw plugin authority reimport` path handles independently
re-provable orphan state.

**Rationale**: A digest proves bytes are stable, not that an engine command, worktree, pane, branch,
or peer decision was host-authorized. Lazy migration after a guest can write state lets an attacker
forge "legacy" bytes and obtain authority on the next invocation. Domain epochs preserve staged
artifact testing and staggered Swarm/Team/Wave cutovers without trusting those bytes.

**Alternatives considered**: digest-only provenance (forgeable); one global epoch (incompatible with
staggered owners); lazy post-cutover migration (indistinguishable from guest-forged legacy state);
raw operator path import (ambient filesystem authority).
