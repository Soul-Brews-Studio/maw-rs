# Phase 1 Data Model: Lean-Core Workflow Plugins

This feature adds no database. The model defines ownership, artifact, capability, parity, and
cutover records used by manifests, tests, specs, and issue ledgers. Existing team/account files
remain authoritative and are accessed only through approved host contracts.

## VerbOwnership

| Field | Type | Rules |
|---|---|---|
| `canonical` | non-empty string | Unique across native and installed plugin owners. |
| `aliases` | ordered unique strings | Every alias resolves to the same owner and missing hint. |
| `owner_kind` | `native` or `plugin` | Exactly one reachable owner after cutover. |
| `package` | optional package reference | Required when `owner_kind=plugin`. |
| `help_policy` | enum | Declares whether verb owns `--help` and first-argument `-v`. |
| `status` | CutoverState | Tracks reviewed ownership transition. |
| `route_id` | normalized string | Host-owned invoked route supplied to the accepted artifact. |
| `intent_id` | optional normalized string | Host-derived subcommand/action intent; effectful routes receive only its capability/resource subset. |

Validation: canonical and aliases contain no control characters or leading dash; aliases cannot
collide with another runnable plugin canonical/alias. A pre-cutover native collision is represented
as staged/shadowed and cannot receive normal CLI dispatch; native and plugin owners cannot both be
active.

## PluginArtifact

| Field | Type | Rules |
|---|---|---|
| `name` | string | Matches external package/manifest name. |
| `source_repo` | repository | `Soul-Brews-Studio/maw-plugins`. |
| `source_path` | path | Under `packages/<name>/`. |
| `version` | semantic version | Manifest version. |
| `sdk_requirement` | version range | Must satisfy native host floor. |
| `artifact_sha256` | 32-byte digest | Exact committed `plugin.wasm` bytes. |
| `build_provenance` | immutable evidence | Pinned toolchain/environment, exact commands, source SHA, and two clean byte-identical rebuilds equal to `artifact_sha256`. |
| `capabilities` | set of CapabilityGrant | Minimal grants only. |
| `ci_evidence` | immutable run/ref | Must match final artifact bytes. |
| `routes` | ordered nonempty list | Canonical commands/aliases/flag policy; collisions rejected. |
| `intents` | optional ordered list | Required for every new #963 typed/effectful or multi-workflow route; contains non-overlapping argv grammars, effective capability subsets, call budgets, argument roles, and resource plans. |

State transitions: `SourceDraft -> Built -> HashPinned -> CIValidated -> Accepted -> Superseded`.
Only `Accepted` artifacts may be used for downstream native removal.
An immutable pre-#963 legacy `cli.command` artifact may omit intents only while its path/source/hash
matches the recorded legacy eligibility set and it receives no new typed #963 capability. Any changed,
new, `cli.routes`, or typed/effectful scoped manifest must declare intents; legacy eligibility cannot
be copied to another package or hash.

## CapabilityGrant

| Field | Type | Rules |
|---|---|---|
| `domain` | enum | `cli`, `tmux`, `fs`, `consent`, `engine`, or bounded future domain. |
| `action` | string | Typed operation, never wildcard raw authority. |
| `resource` | optional string | Exact CLI verb, named path root, or provider id. |
| `mode` | optional enum | Read/write/execute only where meaningful. |

Forbidden grants: raw process environment, auth token material, arbitrary absolute filesystem path,
unrestricted process execution, unrestricted tmux command, trust-store mutation.

## ParityCase

| Field | Type | Rules |
|---|---|---|
| `id` | stable string | Unique within a command contract. |
| `argv` | ordered string list | Exact CLI input. |
| `environment` | bounded map | Secrets redacted; only contract-relevant variables. |
| `fixture_state` | reference | Deterministic source/team/tmux/provider setup. |
| `expected_exit` | integer | Exact. |
| `expected_stdout` | bytes/text fixture | Exact unless explicitly normalized. |
| `expected_stderr` | bytes/text fixture | Exact unless explicitly normalized. |
| `expected_host_calls` | ordered call list | Includes no-call/no-mutation proofs. |
| `native_red` | evidence | Fails when native owner is absent or expected change unimplemented. |
| `artifact_green` | evidence | Runs committed artifact bytes through real host surface. |

## CutoverState

```text
Inventoried
  -> ContractFrozen
  -> PrerequisitesGreen
  -> ArtifactImplemented
  -> ArtifactAccepted
  -> DownstreamCutoverGreen
  -> NativeRemoved
  -> Converged
```

Transitions are monotonic. A failed hash/capability/parity/gate check returns the item to the latest
valid earlier state; it never silently advances.

## TeamStateRoot

| Name | Native precedence that must be preserved | Access |
|---|---|---|
| `tool-teams` | home-based tool-team root | read/write through named root |
| `team-vault` | `MAW_STATE_DIR/team-vault`, then `MAW_RS_TEAM_PSI`, then repo-local `ψ` | read/write through a new narrow semantic root; never generic vault |
| `team-state` | existing MAW_HOME/XDG state/config precedence | typed read/write/list |
| `team-worktree` | validated repo/worktree paths from native plan | lifecycle delegation only; no arbitrary fs grant |
| `artifact-store` | current maw-cache root plus legacy read-only fallback | merged/deduped read/list; primary-only mutation |
| `mission-store` | host-bound repository `ψ/missions` | bounded read/list and atomic private mission writes only |

Writes requiring atomicity use create-temp, mode 0600, flush, and rename semantics supplied by the
host. Manifest-approved `listRoot` exposes only bounded name/kind rows. Recursive archive-copy,
merge-copy, and archive/remove use separately granted, preflighted source/destination operations. A
durable complete archive commit precedes any removal; copy failure leaves every source byte, while
only post-commit removal may be partial. Unknown JSON members survive mutation when the current
native path preserves them. Each root is modeled as one primary plus ordered read-only fallbacks.
Read/list may return a bounded non-authority `displayPath` only for frozen rendering; no host
operation accepts it.

## TeamActionOutcome

| Field | Type | Rules |
|---|---|---|
| `requested` | ordered member list | Frozen before mutation. |
| `confirmed` | ordered member list | Only actions with confirmed success. |
| `current` | optional member | Failure/ambiguous target. |
| `remaining` | ordered member list | Must not be mutated after stop. |
| `status` | `confirmed`, `failed`, or `ambiguous` | Ambiguous is never rendered as not-delivered or full success. |
| `detail` | bounded string/code | No message bodies/secrets. |

## EngineProviderDescriptor

| Field | Type | Rules |
|---|---|---|
| `id` | normalized string | Exactly `provider.codex` for the first external provider. |
| `artifact` | PluginArtifact reference | Must be accepted. |
| `provider_export` | non-empty export name | Dedicated host-to-guest entry; not the CLI export. |
| `operations` | set | Plan launch/resume/profile/maintenance, health, classification, and occupancy as separately granted. |
| `input_schema_version` | integer | Native host rejects unknown future versions. |
| `output_schema_version` | integer | Guest/native compatibility pinned. |
| `executable_id` | normalized string | Native-bound identity; provider response must echo exactly. |
| `argv_schemas` | operation -> schema | Allowed flags/cardinality/limits; native-enforced. |
| `env_schemas` | ordered name/schema bindings | Scalars are bounded; path values use host-issued provider-root ids only. |
| `occupancy_descriptor` | optional descriptor | Host-validated declarative process/account matcher. |

Provider output references the native-bound executable id and supplies only schema-valid args and
allowlisted non-secret settings. It cannot select an arbitrary executable, directly mutate tmux,
inspect raw process environments, or retrieve secret token content.

## WorkflowHostOperation

| Field | Type | Rules |
|---|---|---|
| `surface` | versioned enum | lifecycle, pane-submit, pane inventory/observe, layout, batch-launch, named state/input, worktree/repo issue, consent/peer trust, provider-maintenance, health, occupancy |
| `action` | closed enum | Generic primitive, never a product subcommand or raw argv. |
| `request` | bounded DTO | Identifiers/options validated before mutation. |
| `capability` | exact grant | No wildcard. |
| `adapter` | injected native trait | Defined by maw-plugin-manifest; implemented by maw-cli; unavailable by default. |
| `outcome` | ordered typed result | Distinguishes confirmed, failed, ambiguous, and unknown. |
| `call_budget` | bounded one-shot counter | Shared across cloned host handles; atomically consumed before mutation and never restored after any outcome. |

The plugin-manifest crate never depends on maw-cli. Provider planning reverses direction: maw-cli
selects an accepted descriptor and invokes its dedicated export in a fresh instance. The typed stack
forbids provider-to-host-operation/provider recursion. Provider mode uses an explicit plan-only
capability set even when the same artifact's CLI routes have broader grants.

## InvocationAuthority

One shared per-invocation authority object records host-issued pane ids, pane-inventory-scope refs,
split-plan refs, pane-payload refs, layout-pane-set refs, lifecycle/batch outcome refs,
peer-trust/consent-decision refs, issue refs,
bound content refs,
and cwd refs,
route/argument-role-bound document receipts, scalar charter-command receipts, workspace/value refs,
planned-workspace refs, engine refs, lifecycle-member refs, mission-store refs, original
positional/flag tokens, Team/Swarm session-environment refs, archive-copy receipts, provider-root ids,
and atomic per-action call budgets. Every consumer validates kind,
invocation, digest/generation, and expected semantic key. Host-function clones share this object;
unissued, stale, cross-kind, or cross-invocation references fail closed.

`LifecycleOutcomeRef` and `BatchOutcomeRef` bind the exact invoked intent, request refs, ordered
confirmed/ambiguous outcome, and native-created member/workspace/engine/pane facts. Guests may render
the bounded response, but authority-bearing state writes accept only the opaque outcome ref; they
cannot reserialize, replay, reorder, or substitute another individually valid pane/result.

`InvocationIntent` is derived by the host from canonical route and original argv using the accepted
manifest's non-overlapping grammar. It carries the effective static-capability intersection and a
host-evaluated resource plan over reviewed argv roles plus document/named-state receipts, including
mandatory bounded action/cardinality budgets. Security-sensitive reads have finite budgets;
mutations are one-shot. Invalid or
unmatched argv receives no typed host grants. Refs are intent-bound: same-team valid refs from another
intent or targets excluded by `--only`/`--members`/`--keep`/eligibility predicates are refused. A
mutating request atomically exhausts its one-shot budget before dispatch; confirmed, failed, timeout,
and ambiguous outcomes all make replay through any cloned host handle terminal.

`SplitPlanRef` binds the exact reviewed target, orientation, percentage, and optional command roles
for one split intent. `PanePayloadRef` is one of `operatorArg`, `enterOnly`, or
`confirmedMissionPointer`; it binds exact bytes or a confirmed mission-write outcome, never a guest
literal. Both are one-shot and reject cross-role/intent/replay use.

`PeerTrustRef` binds one trusted named-peer resolution to the reviewed route/intent/peer argument and
trust generation. `ConsentDecisionRef` binds one accepted decision (or exact native consent-disabled
policy) to the same action/peer. Pending, denied, unreachable, missing, unknown, and invalid facts
issue no mutation ref. Remote-adopt/invite semantic writes consume these refs and the host
materializes authoritative peer/request fields; public display values never substitute for them.

`PaneInventoryScopeRef` binds one intent's exact team/session/member, wave-state, layout, or
separately reviewed More all-window read scope. Inventory and observation reveal/issue refs only for
that scope. `IssueRef` binds one positive original-argument issue number to the current issued
repository/workspace and one-shot fetch budget; neither can be reconstructed from display values.

`ContentRef` binds exact instruction bytes to a route/intent/semantic role and one of four reviewed
origins: operator argument, same-invocation immutable operator-document receipt scalar, host-wrapped
IssueRef result, a hash-covered manifest template evaluated natively from issued fields/refs, or
previously host-written instruction content with an exact final durable digest/generation record.
Pre-epoch prompt files are inventoried into that record set. Lifecycle prompts and
semantic task/message/mission writes accept only that ref. A confirmed mission write derives its
pointer PanePayloadRef from the same committed content; guest literals/template substitutions fail.

## PaneObservationDescriptor

An accepted manifest may bind a bounded id to a small set of literal markers. Runtime returns only
matched booleans over a fixed joined capture tail; captured text never crosses the ABI. Discovery
rejects wildcard, control, empty, duplicate, and over-budget descriptors.

## OperatorDocument

A closed selector authorizes either (a) one exact native-reviewed route/subcommand/argument role,
including an operator-supplied absolute path, (b) the native ordered implicit charter candidates for
one invocation-bound team id, or (c) the unique local charter when exactly one candidate exists. The
same bytes in an unapproved argv slot grant nothing. The host returns bounded regular non-symlink
JSON/TOML/YAML bytes, a non-authority display path, and an opaque digest receipt; it grants no ambient
path/directory access. Separately granted operations may digest-check and atomically replace only the
same document or archive-copy its exact bytes to a named root. Lifecycle engine-command references
must resolve `/engines/<same engine>` from that receipt.

## InvocationClock

The native host samples one `nowMillis` for the invocation and exposes it as safe typed context.
Guests reuse it for every current-command timestamp/archive/message id. Tests inject it; WASI and an
ambient clock remain disabled.

## DurableAuthorityRecord

A native-only record binds one semantic root/object/member, its source document/value receipts, the
resulting file digest/generation, and a closed authority projection: team/member/session/window/pane,
workspace/repository/branch, engine/full command, durable repo/mission identity, peer/consent
decision, and prompt/task/message/mission content digests. Guests cannot
read, write, copy, or invent these records. General state writes must preserve the projection;
authority-changing writes are typed semantic mutations whose host adapter materializes values only
from already issued refs/receipts and commits the record with the state. A partial state/record commit
is ambiguous and blocks later ref issuance until native reconciliation. Subsequent EngineRef,
WorkspaceRef, LifecycleMemberRef, command, mission, or delete-branch issuance requires an exact
record/digest match plus live native revalidation. Legacy state receives a record only after every
field is independently re-proved; content digest or repository containment alone never confers
authority.

The closed semantic action set is `createFromReceipts`, `upsertMemberFromRefs`,
`removeBoundMember`, `claimLeadFromCurrentSession`, `writeBoundContent`, `recordLifecycleOutcome`,
`recordBatchOutcome`, `recordPeerDecision`, and `retireObject`. Lead claim binds the host current
session/team/source generation; peer/outcome actions consume opaque refs and cannot accept
guest-reconstructed authority fields.

`AuthorityEpoch` is a versioned native-only seal scoped separately to `swarm`, `team`, or
`wave-codex`. A preparatory release puts every inventoried legacy writer under the same cross-process
domain lock/generation protocol. After static cutover, a retained dispatcher hook runs before first
write-capable plugin entry on each machine, proves incompatible writers quiescent, snapshots and
revalidates exact legacy projections (including archived full commands), seals, and only then enables
that domain's mutation imports. Concurrent or later untracked drift fails closed. Direct artifact
tests use injected isolated seeded domains, never the production registry. After sealing,
missing/pending/mismatched records never migrate lazily. The native `maw plugin authority reimport` recovery accepts only a closed
domain/object/member selector plus expected digest, independently re-proves fields, requires explicit
confirmation, and audits the record; it accepts no arbitrary path or guest request.

## WorkspaceRef

An opaque same-invocation reference is issued only from a pane cwd, an approved `--cwd`/`--worktree`
argument role, or a digest-bound charter member cwd/worktree field. Native code canonicalizes and
validates the repository/worktree boundary; worktree inspection/lifecycle accepts the reference, not
the displayed or guest-supplied path. Cross-pane/member/invocation and stale refs fail closed.

`WorkspaceValueRef` is a separate persist-only local-adopt value issued byte-for-byte from an exact
`--cwd`/`--worktree` operator role. It may be stored as `operatorLiteralPending` without existence or
repository validation but is never accepted by lifecycle/worktree/Git. A later invocation issues a
WorkspaceRef only after native canonicalization/boundary validation; pane-derived adopt stores a
validated WorkspaceRef instead.

## PlannedWorkspaceRef

A non-mutating worktree plan binds one host-resolved repository, a validated relative
`agents/<slug>` destination, host-issued `GitRefRef` base/branch authority, and an EngineRef. A
`GitRefRef` may originate only from a reviewed operator argument, an inspected WorkspaceRef/branch
fact, or a manifest-hash-covered deterministic template evaluated against the native repository
snapshot; a guest string can never be upgraded into one. Only `worktree.create` consumes the plan;
the host revalidates generation/collision state and returns a normal WorkspaceRef after confirmed
creation. Absolute/traversal/cross-repository inputs, stale plans, and guest-reconstructed values fail
before mutation. More and Wave use plan -> create -> lifecycle rather than inventing a destination
WorkspaceRef.

## EngineRef

An opaque same-invocation launch-policy reference is issued only from an approved
`-e`/`--engine`/`--pool` argument role, a reviewed Swarm agent-executable positional role, a
digest-bound charter member engine or frozen legacy `model` engine fallback, an archived manifest's
`memberEngines` entry, a host-owned frozen built-in/default id, an accepted provider descriptor, or a
host-written and revalidated `.maw-engine` marker. Local adopt may receive an EngineRef only from the
intent-bound selected pane's host-side conservative engine classification into an accepted provider
or frozen built-in id. Lifecycle and worktree-marker operations accept
the ref, never a guest engine string. The host binds it to the exact executable/config policy and
rejects forged, cross-member, stale, missing-provider, or unissued refs before mutation.

## LifecycleMemberRef

An opaque member authority has one of two scopes. `plannedLaunch` is issued from exact direct
team/member arguments or a bounded validated creation plan and permits only session ensure/member
launch. `ownedExisting` is issued from a route-bound charter, named tool configuration, archived
manifest, or wave-state receipt; it binds team, session, member, pane when live, WorkspaceRef,
EngineRef, and source digest/generation. Resume, finish, and terminate require `ownedExisting`, and
the native adapter re-inventories the pane before mutation. Cross-team/member/pane and stale refs fail
closed. A More workflow receives launch-only authority and cannot terminate an existing member.
Workspace selection is a closed `boundWorkspaceRef | nativeDefaultResolution` enum; the second variant is
issued only from a digest-bound charter worktree opt-out, exact direct-spawn intent with no cwd/
worktree argument, digest-bound spawn-from member fallback, or final-record archived resume member;
it binds the member identity, carries no path, and makes the native adapter omit repo/workspace
overrides so wake performs its normal resolution. Team bring may receive
a bound WorkspaceRef only from a durable registry-member identity evaluated by the native fixed
relative-member policy under the bound repository and then inspected; guest relative strings never
qualify.
For Wave teardown, worktree archive/remove/delete-branch may consume the owned ref and derive its
workspace/branch internally after re-reading the bound wave-state generation; the guest never
receives a free branch/path authority.

## RepoIdentity and MissionStoreRef

Wave state may persist a host-issued durable `RepoIdentity` plus a legacy display-only mission path.
The identity is not a filesystem authority. On a later invocation native code resolves and validates
it against the current/registered repository boundary, then issues an invocation-local
`MissionStoreRef` for that repository's `ψ/missions`. Mission writes accept only this ref and a
validated team/member-relative path. Legacy absolute state is migrated only after the same native
boundary proof; forged, stale, or cross-repository identities fail closed.

## SessionEnvironmentRef

An opaque ref is issued only from reviewed Team-spawn or Swarm `--parent`, `--parent-session-id`, or
`--session-id` argument roles and binds route, intent, kind, and exact control-free value. Team
lifecycle may forward only the bound native wake fields. Swarm batch launch may materialize only
`MAW_PARENT_SESSION_ID` for every member and `MAW_SESSION_ID` for a one-member batch. Guests cannot
choose environment names, submit a literal, cross Team/Swarm intents, or apply the Swarm
single-session ref to a multi-member batch.

## AccountOccupancyDescriptor

| Field | Type | Rules |
|---|---|---|
| `provider_id` | normalized string | Must match accepted provider. |
| `executable_basenames` | bounded nonempty set | Tokens only; no path or wildcard. |
| `home_marker` | reviewed identifier | Non-secret marker approved at discovery. |
| `named_root_resolver` | enum | Native resolver; guest never supplies absolute paths. |
| `max_slots` | bounded integer | Native-enforced upper bound. |

The descriptor is declarative external policy validated by the native host. Runtime results contain
only opaque account ids and occupancy facts.

## AccountOccupancy

| Field | Type | Rules |
|---|---|---|
| `slot` | positive integer | Within requested bounded slot range. |
| `state` | `free`, `busy`, or `unknown` | Probe failures are `unknown`, never `free`. |
| `owner` | optional redacted identity | Pane/session/user label only. |
| `provider_home_id` | optional opaque id | Never raw secret/config content. |
| `pid` | optional positive integer | Display-only frozen JSON field; never a host process handle. |
| `display_home` | optional canonical path string | Display-only frozen JSON field; never accepted by a host fs/provider operation. |

The native host computes bounded occupancy facts. The guest formats provider-specific UX and never
receives unrelated environment variables. Display-only pid/home fields preserve existing public CLI
output but are deliberately absent from every authority-bearing request type.

## OwnershipLedgerEntry

Maps one story/verb to `VerbOwnership`, source symbols, fixtures, prerequisite child issues,
external PR/artifact evidence, downstream PR/gates, wake/attach regression evidence, and current
`CutoverState`. Issue #963 is the human-readable index; the Spec Kit tasks are the execution graph.
