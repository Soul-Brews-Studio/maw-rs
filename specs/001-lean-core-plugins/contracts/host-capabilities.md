# Contract: Narrow Host Capabilities

These are versioned semantic contracts, not a commitment to exact Rust type names. Each child issue
must freeze JSON/schema and error codes before implementation.

## Injected native operations

Scoped #963 workflow guests do not call arbitrary `maw.cli.run` argv.

`maw-plugin-manifest` defines the serializable DTOs and a narrow `WorkflowHostOps` trait. `MawWasmHost`
stores an injected, thread-safe trait object; its default implementation is unavailable/fail-closed.
maw-cli implements and injects the only production adapter when it constructs the ship-tier runtime.
This keeps the existing dependency direction (`maw-cli -> maw-plugin-manifest`) and lets native
wake/done/tmux/worktree/provider code remain in the top crate. Host tests inject a fake adapter.

`maw.workflow.lifecycle.v1` exposes generic primitives, not product subcommands: ensure a session,
launch/resume one validated member in a validated worktree through native wake, and finish/terminate
one validated member through native done/session mechanics. It also offers a host-owned keep-session
guard when a validated plan would remove every window and a bounded `wait_reinventory` (maximum 30
seconds) that returns freshly issued surviving panes before any optional force terminate. The guest
owns the policy that maps spawn/up/bring/apply/reassign/down/shutdown/more/wave to these primitives
and writes shutdown mailbox requests through named state. Per-action capabilities such as
`workflow:lifecycle:launch` are exact; wildcard resources are rejected.

The same surface exposes a non-mutating `planLaunch` action. It accepts the same issued member,
workspace, engine, command, and optional session-environment refs as `launch`, performs every
host-side validation, and returns a bounded, host-rendered `displayCommand` for print-only parity.
That display string is non-authority data: no host request accepts it as argv, a path, an executable,
or a ref.

Every action consumes an opaque `lifecycleMemberRef`. A `plannedLaunch` ref may be issued from exact
direct team/member argv roles or a bounded validated creation plan and can only ensure/launch. An
`ownedExisting` ref is issued from a route-bound charter, named tool config/archived manifest, or
wave-state receipt and binds team, session, member, pane (when live), workspaceRef, engineRef, and
source digest/generation. Resume/finish/terminate require `ownedExisting`; native code re-inventories
and proves the current pane still matches before mutation. Forged, stale, cross-team/member/pane refs
fail closed. A More create flow receives launch-only scope and cannot terminate existing panes.

The member ref binds a closed workspace selection: either an issued `WorkspaceRef` or
`nativeDefaultResolution`. The latter is issued only from a digest-bound charter
`worktree:false`/equivalent opt-out; an exact direct-spawn intent with no cwd/worktree argument; a
digest-bound spawn-from member with the frozen absent-cwd/worktree fallback; or a
final-record-validated archived resume member. It binds the exact member identity and makes the
native wake adapter omit every repo/workspace override, preserving normal native resolution; it
cannot carry a guest path. For native Team bring's no-charter path, the host may issue an
existing WorkspaceRef only by combining a durable-record-validated registry member identity with the
manifest-fixed relative-member policy under the bound repository, then inspecting/canonicalizing the
result; forged relative segments or cross-repository paths fail. A frozen default EngineRef is issued
separately. The guest cannot synthesize either selection.

`maw.workflow.pane_submit.v1` owns membership proof, all-target preflight, bounded text/Enter
submission, and confirmation for team `enter`/`send-enter` plus Wave's state-owned update-prompt
Enter and mission-pointer submission. Wave calls consume an `ownedExisting lifecycleMemberRef`
issued from digest-bound wave state and an issued current pane; the guest cannot substitute an
inventoried-but-unowned pane. Every call also consumes an opaque `PanePayloadRef`: `operatorArg`
binds Team send-enter text byte-for-byte to its reviewed original argument role, `enterOnly` binds
Team enter or Wave update-prompt confirmation and contains no
guest bytes, and `confirmedMissionPointer` is issued only from the matching committed mission-write
outcome. Cross-role/subcommand substitution, invented text, mission submission before the write,
and replay are refused. State-backed `send`/`msg`/`broadcast`/`inbox` remain guest policy over
named atomic roots and do not use pane submission.

Both operations reject direct or indirect plugin cycles, cap request/response bytes and execution
time, and return ordered `{requested,confirmed,current,remaining,status,detail}` outcomes. A timeout
after possible mutation is `ambiguous` and is never retried. Lifecycle mutation responses also carry
an opaque `LifecycleOutcomeRef` binding the exact intent/request, ordered confirmed/ambiguous result,
and native member/workspace/engine/pane facts. `maw.state.authority_write.v1` consumes that ref, never
guest-reconstructed outcome JSON or separate pane ids.

## CLI result/status envelope

Guest results retain `{ok,output,error}` and may add an optional bounded `exitCode` (0..=125).
Absent `exitCode` keeps legacy mapping. The dispatcher validates the field and uses it only for CLI
invocations, allowing contracts such as team-consent-pending exit 2 without turning every guest
failure into exit 1. Invalid values fail closed.

## Multi-command invocation identity

Legacy manifests keep this shape unchanged:

```json
{"cli":{"command":"team","aliases":["t"],"help":"...","flags":{}}}
```

A manifest may instead use a mutually exclusive ordered, nonempty route table whose entries have the
same fields:

```json
{"cli":{"routes":[
  {"command":"codex","aliases":[],"help":"...","flags":{}},
  {"command":"more","aliases":[],"help":"...","flags":{}},
  {"command":"wave","aliases":[],"help":"...","flags":{}}
]}}
```

Specifying both `command` and `routes`, an empty route table, an unknown route field, or a duplicate
canonical/alias within one manifest is invalid. Manifest discovery refuses collisions among runnable
plugins. Native route ownership stays in maw-cli: the dispatcher supplies its authoritative route set
to the ownership/list layer, which records `shadowedByNative` without duplicating that set in the leaf
crate. Native dispatch keeps winning while committed plugin bytes are directly verified before
cutover. After a route matches, the host adds the canonical `invokedCommand` to context; aliases never
replace it and the guest cannot override it. Legacy single-command context remains unchanged.

### Invocation intents and effective authority

An effectful route also declares a bounded, hash-covered intent table. Each row has a unique intent
id, a non-overlapping/default-aware argv grammar (subcommand aliases, reviewed flags and positional
roles), an exact subset of the artifact's static capabilities, a mandatory per-action call budget,
and a generic resource-plan schema.
The dispatcher derives `invokedIntent` from the original argv before guest entry. Invalid or
ambiguous argv may still reach the guest for usage rendering, but receives an empty typed-host
capability set. Every host call checks the invocation-effective intersection, not the artifact-wide
grant. Capability-free routes need no intent table. An immutable pre-#963 legacy `cli.command`
manifest may retain its existing static-capability behavior only when package path/source/hash match
a host-recorded eligibility row and it receives no new typed #963 capability; copying or changing
that manifest loses eligibility. Every new #963 typed/effectful, changed, `cli.routes`, or
multi-workflow manifest requires intents and otherwise receives no typed grant.

The resource-plan schema binds target/payload refs to reviewed argv roles plus exact
document/named-state receipts. It may declaratively constrain include/exclude member roles,
eligibility fields, action, cardinality, and ordering; native code evaluates those constraints and
issues refs only for the resulting set. A valid same-team/member/pane ref issued for another intent,
an excluded/kept/adopted/lead member, or a target outside `--only`/`--members` is refused. This keeps
product policy in the accepted manifest/guest contract while preventing `team list` from calling
terminate/write, `more status` from calling maintenance, or `wave status` from teardown/create.
Intent descriptors cannot add a capability absent from the artifact manifest, use wildcards, or
grant a raw process/tmux/fs/environment operation.

Every action budget is host-enforced and shared by all host-function clones for that invocation; a
manifest cannot request an unbounded counter. Security-sensitive reads such as pane inventory/
observation, health, occupancy, and issue context have finite intent/resource-scoped budgets and
cannot broaden or reset them through cloned handles. Mutation authority is one-shot: the host
atomically consumes it before dispatch, and confirmed, failed, timed-out, or ambiguous outcome
exhausts it. Split, pane submit, lifecycle, batch launch, layout, worktree mutation, maintenance,
consent, and semantic state writes all use that rule; replay is terminal.

### Bound instruction content

No typed workflow accepts guest-authored instruction text as authority. An opaque `ContentRef` is
issued only from an exact reviewed operator argument role; a same-invocation immutable
operator-document receipt scalar; a
host-wrapped `IssueRef` result; or a bounded, hash-covered manifest template evaluated natively from
already issued fields/refs. A later invocation may reissue only previously host-written instruction
content whose exact digest/generation has a final durable authority record; pre-epoch prompt files
must be inventoried into that record set. The ref binds route, intent, semantic role, exact bytes/digest, source
generation, and call budget. Lifecycle prompts and semantic task/message/mission writes consume the
ref and native code materializes those exact bytes. Cross-role/subcommand substitution, guest
literals, altered template output, and replay fail closed. A confirmed mission write may issue the
matching `PanePayloadRef` pointer only after its exact ContentRef bytes are committed.

## Typed invocation context

Guest context may expose only reviewed semantic fields such as host-owned `invokedIntent`, current team identity, consent mode,
current session identity, and one host-generated `nowMillis` sampled once per invocation. The time
value is fakeable in host tests and is reused for every guest timestamp/stamp in that invocation;
WASI and an ambient clock are not enabled. Each field has a manifest capability or is always-safe public context.
There is no general environment lookup. `MAW_RS_TEAM_PSI` remains a native root-selection
compatibility input and never crosses the ABI; other test-only `MAW_RS_TEAM_*` hooks are replaced by
injected host fixtures. Secret/provider auth state is exposed only as typed health/status facts.

## `maw.tmux.split.v1`

This mutation contract is a separate ABI/version/capability slice from pane inventory. Shipping
split does not enable pane inventory or any other tmux operation.

Request:

```json
{
  "planRef": "split-plan-ref-1"
}
```

Response:

```json
{"ok":true}
```

The opaque plan ref is issued only from the reviewed Split target/orientation/percentage/optional
`--cmd` argument roles for this intent and binds their exact parsed values; the guest cannot add or
substitute a pane, command, direction, or size. Validation occurs before issuance and again before
runner mutation: target is nonempty/unpadded/not dash-prefixed/no control; percentage is finite and
1..=99; command is absent or the exact nonempty/control-free operator value. The one-shot host action
performs exactly one typed split. Errors are bounded codes and do not echo secrets or shell-expand
input.

## Named team roots and atomic writes

Semantic roots:

- `tool-teams`
- `team-vault` with native precedence: `MAW_STATE_DIR/team-vault`, then `MAW_RS_TEAM_PSI`, then
  repo-local `ψ`
- `team-state` with current MAW_HOME/XDG precedence
- `artifact-store` with current maw-cache precedence plus the existing legacy read-only fallback
- `mission-store` rooted exactly at the host-bound repository's `ψ/missions`, for wave mission
  documents only

The versioned request is `{root,relative,operation,...}`. Host state stores each root as
`NamedRoot { primary, readFallbacks }`; read/list merge and dedupe primary plus reviewed legacy
fallbacks, while every write/remove targets primary only. An exact-capability `listRoot` enumerates
only bounded name/kind rows for a manifest-approved root; root-self read/write/remove remains denied.
Resolving named roots and constructing a host performs no filesystem writes, even when the artifact
has mutation capabilities. Parent directories are created only inside a fully validated authorized
mutation operation; read-only/parse/preflight/provider failures therefore cannot create roots.
Operations are bounded read/list,
create-if-absent, atomic private replace, and append where the native contract is append-only. The
guest supplies a relative validated path, never an arbitrary absolute path or the root itself. Atomic
private replace uses host-managed temp/0600/flush/rename. Closed `archiveCopy`/`mergeCopy` operations
accept only manifest-bound source/destination named roots, bounded relative subtrees, and a reviewed
named copy/filter/collision policy; they never remove source bytes. `archive_then_remove` uses the
same closed inputs. It preflights both trees, refuses symlinks, and must durably complete/commit the
entire archive copy before the first source removal; any preflight or partial-copy failure leaves all
source bytes untouched. Only post-commit removal may be partial/ambiguous. A copy receipt may
authorize later `removeAfterArchive` of that same source for shutdown only: the host tracks and
permits only its own approved invocation writes between copy and remove, while any untracked
generation/path change is stale. These are the only recursive archive/delete authorities for team
delete/prune/gc/shutdown. Raw paths, symlinks, and traversal fail.
Archive-tree symlink refusal/preflight is an intentional fail-closed correction from native walkers
that may skip or follow in-root entries. It shares the operator-document filesystem-symlink human
gate: freeze both native behaviors and obtain explicit approval before implementing either change.
Any reviewed named-root operation outcome may include canonical source/destination `displayPath`
fields only where frozen output prints them (including team create/resume, lives, artifacts get, wave
mission write/archive). They are bounded and explicitly non-authority: no host request accepts them
as a path, root, pane, process, or provider reference.

A resolve-only `previewDisplayPath` action may join one manifest-intent-bound semantic root with a
host-validated relative template solely for frozen plan/dry-run output, including paths that do not
yet exist. It creates/stats nothing beyond safe parent-boundary validation, returns no receipt/ref,
and no host request accepts the returned string. The intent/resource table fixes the allowed root,
template role, and cardinality; a guest cannot preview an arbitrary root-relative path.

### Cross-invocation authority provenance

Named-root or operator-document bytes are data, never execution/repository/member authority merely
because the guest wrote them and a later invocation can read them. The host defines a closed
authority projection covering member/session/window/pane, cwd/worktree/branch/repository,
engine/full engine command, persisted repo/mission identity, peer/consent decision, and
instruction-bearing prompt/task/message/mission content digests. A generic atomic replace must leave
that projection byte-for-byte unchanged.

Legitimate authority changes use typed semantic state mutations. Requests supply only already issued
document/value/workspace/engine/member/repository receipts plus non-authority fields; native code
materializes the authority-bearing JSON fields. The host records a durable, guest-inaccessible
authority record binding semantic root, relative object/member, source receipt, resulting digest and
generation, and the issued authority projection. State and record updates are preflighted together.
If either commit is partial or ambiguous, no later authority ref may be issued until native
reconciliation succeeds, and the original operation reports the partial outcome truthfully.

The v1 semantic action enum is closed: `createFromReceipts`, `upsertMemberFromRefs`,
`removeBoundMember`, `claimLeadFromCurrentSession`, `writeBoundContent`, `recordLifecycleOutcome`,
`recordBatchOutcome`, `recordPeerDecision`, and `retireObject`. Lead claim binds the invocation's
host-owned current session, team/domain/source generation, and one-shot intent; it cannot accept a
guest session id. Peer and outcome actions consume their opaque refs rather than guest JSON.

Authority epochs are domain-scoped (`swarm`, `team`, and `wave-codex`) because their native owners
cut over at different times. A preparatory release makes every inventoried legacy native writer for
one domain take the same cross-process transition lock and maintain generation/digest state. After
the static cutover, the dispatcher runs a retained native transition hook before the first
write-capable plugin entry on each machine: it proves incompatible legacy writers are quiescent,
takes the lock, snapshots/revalidates the still-native legacy authority projections (including
archived full commands), writes their records, and seals that domain's versioned epoch. Any
concurrent generation change, orphan, or unprovable field refuses the handoff. That domain's mutation
imports remain refused until sealing succeeds; later out-of-band drift also fails closed. Direct artifact compatibility
uses only an injected isolated preseeded/sealed fake domain; it is never a production bypass. After
sealing, a missing/pending/mismatched record never migrates lazily.

An orphan or operator-edited legacy command uses the native `maw plugin authority reimport` recovery
surface, scoped to a closed domain/object/member selector and explicit expected SHA-256. It takes the
domain lock, independently re-proves every live/config/provider field, displays the proposed
projection, requires explicit confirmation, appends a redacted audit, and writes the record. It does
not accept an arbitrary path or guest call. Wrong domain/member/digest, unprovable full command, or
missing confirmation fails without record/state mutation.

Later `EngineRef`, `WorkspaceRef`, `LifecycleMemberRef`, command receipt, repository identity, or
branch-delete authority derived from named state requires an exact current final durable record plus
live revalidation. A guest-crafted or generically rewritten charter, tool config, archived manifest,
or wave state cannot mint a ref. Independently re-provable legacy fields may be normalized only
during the locked pre-epoch scan; containment or a matching content digest alone is insufficient.

For frozen local-adopt parity, an explicit `--cwd`/`--worktree` argument may issue a persist-only
`WorkspaceValueRef` containing that exact bounded control-free operator literal without existence or
repository validation. Semantic adopt may store it and mark its durable projection
`operatorLiteralPending`; it cannot be consumed as WorkspaceRef or by lifecycle/Git. A later workflow
may issue a WorkspaceRef only after native canonicalization/boundary validation at that later point.
Pane-derived adopt uses an already validated WorkspaceRef. This preserves current storage behavior
without turning the literal into immediate filesystem authority.

An authorized charter or archived-manifest read may return an invocation-local opaque receipt. A
lifecycle request may reference only `/engines/<same validated engine>` from an operator document or
`/memberEngineCommands/<same validated member>` from a named-root archived manifest. The host
re-reads and digest-checks the file before using the scalar; a stale receipt, different pointer,
literal guest command, or engine/member mismatch fails before launch. This preserves
operator-authored and persisted resume commands without granting a guest arbitrary process
construction.

Lifecycle and worktree marker requests also accept only an opaque `engineRef`, never a guest engine
string. Native code issues it from an approved `-e`/`--engine`/`--pool` argument role, a reviewed
Swarm agent-executable positional role, a digest-bound
charter `/members/<same member>/engine` or the frozen legacy engine fallback
`/members/<same member>/model`, archived `/memberEngines/<same member>`, a frozen native
default/built-in id, an accepted provider descriptor, or a host-written and revalidated `.maw-engine`
marker. Resolution through generic native command configuration happens only after ref validation.
Every engine-influencing field, including legacy `model`/`backendType`, belongs to the durable
authority projection and cannot be changed by a generic state write.
Local adopt may also receive a ref from the selected, intent-bound live pane's host-side conservative
engine classification, but only into an accepted provider or frozen native built-in id. It freezes
the native contains-claude/codex/omx mapping and default-to-Claude fallback for any selected live
non-shell pane. Raw current-command text never becomes authority; stale or cross-pane classification
fails closed.
Forged, stale, cross-member, missing-provider, and unissued refs fail before mutation. A separately
bound engine-command receipt remains required for an operator-authored full launch line.

## `maw.input.document.v1`

The host accepts only closed document selectors:

- `originalArgument` binds a native-issued selector id to a reviewed route, subcommand, and argument
  role (for example `plan.document` or `adopt.charter`) and then binds that exact positional/flag
  value byte-for-byte. The same bytes in a prompt/member/task or another unapproved argv slot grant
  nothing. Absolute paths remain compatible when the operator supplied that exact approved argument;
  the guest cannot invent or substitute one.
- `teamCharter` binds a validated team identity from the current invocation and resolves only the
  native ordered candidates `.maw/teams/<team>.yaml`, `ψ/teams/<team>.yaml`,
  `.maw/teams/<team>.json`, then `ψ/teams/<team>.json` under the host-bound repository.
- `singleTeamCharter` scans only those two charter directories, accepts the native yaml/json
  extensions, and succeeds only when exactly one unique team stem exists.

The host canonicalizes once, requires a bounded regular non-symlink JSON/TOML/YAML document,
enforces a byte cap, and returns bytes, a non-authority display path, and an opaque digest receipt.
There is no ambient path or directory authority. A separately granted `atomicReplace` may write only
the exact receipt-bound charter/document, after re-reading and matching its digest/generation; it
uses host atomic-private-write semantics. A separately granted `archiveCopy` may copy only those
same receipt bytes into a manifest-bound named-root destination. Forged, stale, cross-route, changed
path, wrong selector, or root-self receipts fail before any write. These operations preserve native
adopt/release/remove behavior without giving the guest arbitrary filesystem authority. The same
receipt can back the charter engine-value proof above. Generic replacement cannot change the closed
authority projection; adopt, remove/release, load/spawn, resume, and other authority-bearing changes
use the typed semantic mutation/provenance contract above.

Refusing a symlink supplied in an otherwise approved operator document role is an intentional
fail-closed correction from current native behavior, which follows ordinary filesystem symlinks.
Its child must freeze the current RED, obtain explicit human approval, and record the approved delta
before the host contract is implemented; absent approval, the cutover stays blocked.

## `maw.tmux.pane_inventory.v1`

This read-only contract is a separate ABI/version/capability slice and cannot invoke split or any
other mutation.

Returns bounded typed pane records needed by read-only team/liveness/more operations: pane id,
session, window name/index, pane title, active-window/title facts, current command, current path, and
an opaque same-invocation `cwdRef` issued by native code.
The request consumes an opaque `PaneInventoryScopeRef` derived from the invoked intent/resource plan:
Team/gather/scatter are limited to the bound charter/session/member set, Wave to digest-bound wave
state, and More only to its separately frozen all-window read scope. The guest cannot broaden filters,
choose another team/session, or supply raw tmux format strings or commands. Returned records and
cwdRefs are intersected with that scope; metadata for unrelated panes never crosses the ABI.
Host errors remain errors rather than an empty inventory. Every returned pane id and current-pane id
is issued into one shared per-invocation authority set; later mutation surfaces require an issued id.
Syntactically valid but unissued ids fail before mutation. Host-function clones share that authority.
`cwdRef` may be consumed only by worktree/repository inspection; the displayed cwd string itself is
never accepted as authority.

## `maw.tmux.pane_observe.v1`

An accepted manifest may declare a small hash-covered table of observation ids and bounded literal
marker alternatives. Invocation requests only declared ids for one pane inside the same issued
`PaneInventoryScopeRef`; a pane merely inventoried under another intent/scope is refused. The
host captures a fixed bounded joined tail and returns `{id,matched}` booleans; it never returns pane
text. Discovery rejects duplicate ids, controls, empty markers, excessive marker bytes/counts, and
wildcards. Capture failure is an error, never `matched:false`. This gives wave update-prompt and
working/idle policy to the external artifact without exposing arbitrary capture or a repeated
substring oracle.

## `maw.tmux.layout.v1`

Accepts a bounded ordered transaction containing only validated `join`, `break`, and allowlisted
`select-layout` primitives through an opaque `layoutPaneSetRef`. Native code issues that ref from the
current pane plus the exact charter/member/resource plan for this invoked intent; broad inventory
membership alone is insufficient. The guest cannot add another valid inventoried pane. The native
adapter validates each bound action immediately before that action, stops on the first post-mutation
error, and returns prior confirmed/current/remaining outcomes. This deliberately preserves the
frozen ordered behavior: a later invalid target can follow earlier confirmed layout mutations, but
the guest cannot substitute an action or target. This is the only gather/scatter layout authority;
no raw tmux argv or format string crosses the ABI.

## `maw.workflow.batch_launch.v1`

Accepts at most ten pane-launch entries, an allowlisted layout, bounded titles, and reviewed session
ids. It returns created pane ids, truthful partial outcomes, and an opaque `BatchOutcomeRef` binding
the exact intent/request/ordered result. Semantic state writes consume only that ref and native code
materializes created member/pane/engine facts; a guest cannot substitute another valid pane/result.
A custom executable is accepted only
through an `engineRef` issued from that exact reviewed original Swarm positional; defaults must
resolve through an accepted provider descriptor or a native host-owned exact built-in executable-id
allowlist frozen by the child contract (needed for legacy default Claude swarm). The guest cannot add
shell fragments, arguments, or environment outside the typed schema. Optional parent/session values
are opaque refs issued only from reviewed Swarm `--parent`/`--parent-session-id` and `--session-id`
argument roles. Native code alone sets exact `MAW_PARENT_SESSION_ID` for every launch and
`MAW_SESSION_ID` only when the validated batch has one member; arbitrary env names, controls,
guest literals, or the single-session value on multi-member batches are refused. Native code owns
split/current-window/layout/title/launch mechanics. This preserves swarm's operator-selected/default
command behavior without granting raw process or tmux access.

The same opaque session-ref type may be issued from reviewed Team-spawn parent/session argument
roles, but is intent/kind-bound: Team lifecycle may forward only those bound native wake fields and
cannot use a Swarm ref; Swarm cannot use a Team ref. The one-member environment rule applies only to
Swarm batch launch.

## `maw.worktree.manage.v1`

Closed actions are inspect, create, prune, archive/remove, and delete-branch. The host resolves the
repository root and validates branch/worktree/ref tokens before Git or filesystem mutation. Create
accepts only an `engineRef` plus an opaque `plannedWorkspaceRef`. A non-mutating `planCreate`
operation binds the current host repository, a validated relative `agents/<slug>` destination,
host-issued `GitRefRef` base/branch authority, and engineRef, then issues that ref. A `GitRefRef` is
issued only from a reviewed operator argument role, an inspected WorkspaceRef/branch fact, or a
manifest-hash-covered deterministic branch template evaluated against the native repository
snapshot; a guest-supplied syntactically valid ref is never upgraded to authority. Create revalidates the plan/generation,
refuses absolute/traversal/cross-repository substitutions or collisions, writes the validated public
engine id to `.maw-engine` natively, and returns an existing `workspaceRef` in its truthful outcome.
Inspect returns canonical path/id, branch, and a re-issued engineRef/marker
fact; it never returns
arbitrary file bytes. Inspecting a pane-reported cwd accepts only an opaque `cwdRef` issued by pane
inventory and returns `notRepo` rather than executing against a guest string. Lifecycle/create accepts
only an existing `workspaceRef` issued from a successful create/inspect, an approved
`--cwd`/`--worktree` argument role, or a digest-bound charter member `/cwd`/`worktree` value,
canonicalized and constrained exactly as the native repo boundary. Forged, stale,
cross-pane/member/invocation refs fail. The guest cannot provide an arbitrary executable, absolute
repository root, or raw Git argv. Failure after possible mutation is explicit and never blindly
retried.

Wave teardown may pass an `ownedExisting lifecycleMemberRef` issued from a digest-bound named
wave-state member. Worktree archive/remove/delete-branch derives the bound workspace, branch, and
engine internally from that ref; the guest cannot override any of them. Native code re-reads the
wave state and repository generation, so forged, stale, cross-team/member/branch refs fail before
Git or filesystem mutation.

Wave start persists a host-issued durable `repoIdentity` beside its legacy non-authority mission
display path. A later wave-state read gives that identity back only to native code, which
re-resolves/revalidates the repository and issues an invocation-local `missionStoreRef`; mission
writes accept only that ref plus a validated team/member-relative path. Legacy absolute
`missionDir` values are never guest authority: native code issues a ref only after proving the path
belongs to the current/registered repository boundary. Forged, stale, or cross-repository identities
fail before write.

## `maw.repo.issue_context.v1`

The request contains only an opaque one-shot `IssueRef` issued from the reviewed positive
issue-number argument role and bound repository/workspace intent. Native code derives and validates the current
GitHub repository from the invocation's bound repo/worktree context, fetches only title/body/labels
through an injected client, enforces byte/count limits, and returns content already wrapped and
marked as untrusted external task material. A substituted/replayed issue number or cross-repository
ref is refused. The guest cannot choose a repository, execute `git` or `gh`, receive credentials, or
reinterpret fetch/parse failure as an empty issue.

## Provider planning, maintenance, and health

Provider planning is host-to-guest, not a guest host-function call. An accepted manifest descriptor
names a dedicated provider export and executable id. Native code invokes that export in a fresh
instance with an `InvokeSource::Provider` context, validates the operation-specific plan, and applies
it only to the descriptor-bound executable. The typed call stack permits CLI workflow -> native
lifecycle -> provider plan, but rejects provider -> lifecycle, provider -> provider, and same-surface
recursion. The provider-mode instance registers ABI-compatible refusal stubs but grants only the
plan-context capability set: every filesystem, pane, lifecycle, layout, batch, worktree, maintenance,
occupancy, consent, and other mutation/read import is unavailable even when the same artifact's CLI
routes hold it. The shared typed call stack propagates into the fresh instance.

`maw.engine.maintenance.v1` executes only an accepted provider's allowlisted maintenance operation
(for Codex, `update`) after validating the provider plan; the guest cannot select the program.
`maw.engine.health.v1` accepts issued engine/workspace refs and returns bounded non-secret
executable/config-command/auth/trust/isolation facts used by team preflight, including
`commandStatus` and only reviewed non-authority display paths needed by frozen output. Raw tokens,
config bytes, paths containing secrets, and ambient environment do not cross the boundary. Provider plan environment values are schema-bound scalars or opaque native
provider-root ids; a provider never supplies an absolute/path-like value such as raw `CODEX_HOME`.

## Rust guest ABI wrapper

New Rust guests use `maw-plugins/crates/maw-guest-sdk`. The crate builds as `cdylib` and `rlib`, owns
reviewed Extism imports, request/response DTOs, size bounds, error decoding, and ABI-version assertions.
WASM externs are target-gated behind an injected guest `Host` trait so native pure tests use a fake.
The exact maw-rs ABI fixture is vendored with its source commit and digest; external CI runs SDK/native
pure tests plus every WASM build. The fixture and conformance result must agree before acceptance.

## `maw.consent.request.v1`

Request carries bounded public intent (peer identity, requested action, and a display-summary
`ContentRef` bound to reviewed fields/template) but no guest summary bytes, credentials, or
trust-store path. Native auth/consent code validates, records, prompts/routes, and
returns `{status,requestId?,oneTimePairCode?,expiresAt?,peer?}` where status is accepted, pending,
denied, unreachable, or invalid. Every optional display field is generated/validated by the host,
bounded, and invocation-bound. A successful pending result includes request id, one-time pair code,
expiry, and peer and maps to CLI exit 2. If the host has durably persisted the local pending request
but peer POST fails, `unreachable` includes only that local request id so native recovery/rendering
remains truthful; invalid/denied or pre-persist failure includes none. The
guest cannot choose/persist a code as authority, write trust or consent files directly, or
reinterpret a denial as success.

The same bounded adapter exposes a read-only `maw.peer.trust_status.v1`: given one validated peer
identity from team adopt/invite context, native code resolves named-peer metadata and returns only
`{peerId,node?,displayUrl?,status}` where status is trusted/untrusted/missing/unknown. Public node/URL
fields are display/state values and cannot be reused as a network or filesystem authority. It never
returns trust-store paths, keys, tokens, or mutable records; lookup failure is `unknown`, not trusted.

A trusted result also carries an opaque `PeerTrustRef`; an accepted result (or an exact host-owned
consent-disabled policy decision) carries an opaque `ConsentDecisionRef`. Each is bound to the
canonical route, invoked intent, reviewed peer-argument role, resolved peer id, current trust/consent
generation, and one-shot action budget. Pending, denied, unreachable, missing, unknown, and invalid
results confer no mutation authority. Remote-adopt semantic state mutation consumes the matching
trust ref; invite state mutation consumes the matching public-peer descriptor and decision ref, then
native code materializes all peer/request authority fields after revalidation. Display JSON, node,
URL, request id, pair code, and expiry cannot substitute for either ref, and generic state replace
cannot change those authorization fields.

## Account occupancy descriptor

The accepted provider manifest declares a bounded descriptor containing provider id, approved
executable basenames, one reviewed non-secret home-marker name, maximum slot count, and a native
named-root resolver. The host validates the descriptor at discovery; arbitrary environment names,
absolute paths, wildcard executables, shell fragments, and secret-marker names are rejected.

At invocation the guest may request only slot numbers from that accepted descriptor. Native code
resolves account roots to opaque ids, reads process environment internally, compares only the
approved marker, and returns occupancy facts. No raw environment name/value or resolved account path
is returned to the guest.

## `maw.account.occupancy.v1`

Request: provider id, bounded slot range, and optional known provider-root ids. Native code inspects
platform process/tmux state and returns only
`{slot,state,owner?,session?,pane?,pid?,displayHome?}`. `pid` and canonical `displayHome` are
non-authority display fields required solely for frozen `codex accounts --json` parity; no host
function accepts them as a path/process reference. Probe/read/parse failure returns `unknown`, not
`free`. After the separately frozen human gate, the guest renders that literal state in text/JSON and
excludes the row from `--free`; it never converts uncertainty into availability. No raw environment,
argv containing secrets, token, or config file bytes cross the boundary.

## Error and retry rules

- Invalid input, denied capability, host unavailable, ambiguous mutation, and schema mismatch are
  terminal.
- A guest never retries a non-idempotent action after timeout or unknown outcome.
- Multi-target guests stop at the first ambiguous/post-mutation failure and report confirmed prior
  targets separately.
- Host error messages are bounded and redact request payloads where they may contain user text.
