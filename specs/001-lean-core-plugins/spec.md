# Feature Specification: Lean-Core Workflow Plugins

**Feature Branch**: `agents/spec-963-lean-core-plugins`

**Created**: 2026-08-20

**Status**: Draft

**Input**: User description: "Move maw bring, maw split, every team workflow, and
Codex-specific account/workflow/provider behavior out of native core into plugins while
leaving maw wake and maw attach native, untouched, and working. Use Spec Kit and GitHub
issues to drive the work."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Native Wake and Attach Stay Stable (Priority: P1)

An operator upgrades through the extraction train and continues to launch, resume, pick,
attach to, and switch among Oracle panes with the same `wake`, `attach`, and `a` behavior.
Optional workflow plugins may be absent or broken without changing these native commands.

**Why this priority**: Wake and attach are the trusted recovery path. Breaking them while
moving convenience workflows would strand running and sleeping Oracles.

**Independent Test**: Run the frozen native wake/attach focused suites and original black-box
fixtures against every cutover revision. Run all non-provider workflow plugins both present and
absent and prove byte/exit/effect parity. For Codex, prove parity with the accepted provider installed,
prove explicit missing/refused provider selection fails before mutation with the approved repair
error, and prove every non-Codex engine remains byte-identical without the Codex artifact.

**Acceptance Scenarios**:

1. **Given** an existing local Oracle pane, **When** the operator runs `maw attach` or `maw a`,
   **Then** native attach resolves and switches exactly as it did before extraction.
2. **Given** a sleeping or missing Oracle, **When** the operator runs `maw wake`, **Then** native
   wake launches or resumes it with the frozen argv, output, and failure behavior.
3. **Given** any unrelated extracted workflow plugin is missing, refused, or corrupt, **When** wake
   or attach runs, **Then** that failure cannot intercept or weaken the native command. A missing or
   refused Codex provider affects only explicitly selected Codex launch/resume and fails before mutation.

---

### User Story 2 - Bring Is an Installable Plugin (Priority: P1)

An operator installs the external `bring` artifact and uses `maw bring` or `maw b` to obtain
the same wake/split plan and machine-readable output previously produced by native core.

**Why this priority**: Bring is the smallest complete extraction and proves the paired-repo
artifact, alias, missing-plugin, help, and native-removal choreography before riskier verbs.

**Independent Test**: Invoke the committed artifact for the full frozen parser/render matrix,
then remove native registration and run the same CLI fixtures through installed plugin dispatch.

**Acceptance Scenarios**:

1. **Given** the bring plugin is installed, **When** any currently valid bring or `b` invocation
   runs, **Then** its text/JSON output and exit code match the frozen native behavior.
2. **Given** an invalid or help invocation, **When** bring runs, **Then** help, validation, and
   error-channel behavior match the approved compatibility contract.
3. **Given** the artifact is absent or fails verification, **When** bring runs, **Then** the CLI
   fails loudly with an exact install/repair hint rather than `unknown command`.

---

### User Story 3 - Split Is an Installable Plugin (Priority: P1)

An operator installs the external `split` artifact and splits the current tmux view with the
same target, direction, percentage, optional command, dry-run, validation, and output behavior.

**Why this priority**: Split is small but crosses the privileged tmux boundary and exposes
global plugin flag collisions; solving it establishes the typed mutation pattern safely.

**Independent Test**: Exercise the installed artifact against an injected tmux host for the
entire frozen argv/validation matrix, including `-v`, percentages, command text, and dry-run,
before removing native dispatch.

**Acceptance Scenarios**:

1. **Given** a valid target, **When** the operator uses horizontal or vertical split options,
   **Then** exactly one typed tmux split action occurs with the frozen argv and output.
2. **Given** `-v` is the first split argument, **When** the plugin is invoked, **Then** it means
   vertical split and is not consumed as a universal plugin version flag.
3. **Given** an unsafe target, invalid percentage, unsafe command text, or host refusal, **When**
   split runs, **Then** no tmux mutation occurs and the approved error/exit code is returned.

---

### User Story 4 - Team Workflows Are Fully External (Priority: P1)

An operator installs the existing external `team` artifact and can use every supported native
team and team-only companion workflow without a shadow native implementation. Read-only,
state-writing, lifecycle, messaging, invitation/consent, and teardown behavior remain intact.

**Why this priority**: Team owns the largest optional workflow surface and is already advertised
as an active plugin even though native dispatch currently shadows all but its incomplete source.

**Independent Test**: Build the external artifact in user-story slices while native dispatch
still owns production. For each slice, run differential fixtures against native output/state and
host call logs. Only after all rows pass, remove native `team`/`t` registration atomically and
rerun the same black-box matrix through plugin dispatch.

**Acceptance Scenarios**:

1. **Given** existing tool-team and vault-team state, **When** list/status/lives/members/history
   runs, **Then** output and liveness classification match the frozen native fixtures except for the
   separately frozen and human-approved inventory-failure fail-closed correction.
2. **Given** valid charter/task/member actions, **When** create/load/add/assign/done/adopt/release
   runs, **Then** only approved named roots or the exact digest-bound operator/implicit charter change
   and unknown fields remain preserved.
3. **Given** a lifecycle request, **When** spawn/up/bring/apply/resume/reassign/down/shutdown runs,
   **Then** the plugin uses bounded typed native lifecycle actions and does not receive arbitrary CLI
   argv or duplicate wake, attach, done, or raw tmux internals. Remove/delete/prune/gc use only the
   closed charter/state archive operations. Team `enter`/`send-enter` and Wave's state-owned update
   prompt/mission-pointer actions alone use pane submission; mailbox messaging does not.
4. **Given** a broadcast or multi-member action partially fails, **When** the command returns,
   **Then** it reports confirmed prior outcomes and the ambiguous/current target without claiming
   full success or silently retrying non-idempotent work.
5. **Given** invitation requires consent, **When** invite runs, **Then** a typed native consent
   request enforces the existing human/auth boundary; the guest cannot edit trust stores directly.
6. **Given** team-only gather/scatter ownership is confirmed, **When** those verbs run after
   cutover, **Then** their separately addressable plugins preserve native layout behavior.
7. **Given** swarm or artifacts/`artifact` is invoked after cutover, **When** default/custom launch
   or artifact read behavior runs, **Then** its separately addressable plugin preserves the frozen
   contract through host-bound executable ids and named artifact roots.

---

### User Story 5 - Codex-Specific Behavior Is a Provider Plugin (Priority: P2)

An operator who uses Codex installs an external provider/workflow artifact and retains account
occupancy reporting, Codex-team workflows, and Codex engine launch/resume/profile behavior. An
operator who does not use Codex carries no Codex product policy in native core.

**Why this priority**: Vendor-specific process, account, and command policy changes faster than
the orchestration kernel and should ship independently, but it requires secret-safe provider APIs.

**Independent Test**: With the provider installed, run the frozen `codex accounts`, `more`, and
Codex wake/attach behavior suites through the generic provider boundary. With it absent, prove
native wake/attach fail explicitly only when Codex was selected and remain functional for other
providers.

**Acceptance Scenarios**:

1. **Given** configured Codex account homes and live panes, **When** `maw codex accounts` runs,
   **Then** table/JSON/free filtering matches frozen behavior without exposing token contents.
2. **Given** a Codex-team `more` workflow, **When** plan/spawn/status/update actions run, **Then**
   their filesystem/process/tmux effects and output match frozen behavior through narrow APIs.
3. **Given** Codex is selected for native wake, **When** launch or resume is planned, **Then**
   generic native orchestration obtains vendor-specific command policy from the installed provider.
4. **Given** the Codex provider is missing or refused, **When** Codex is explicitly selected,
   **Then** wake fails loudly before mutation with repair guidance and never silently falls back.
5. **Given** another engine is selected, **When** wake or attach runs without the Codex plugin,
   **Then** no Codex dependency or error affects that path.

---

### User Story 6 - Operators Can Diagnose Extracted Ownership (Priority: P2)

An operator can inspect help, completion, plugin status, and doctor output to learn which package
owns an extracted verb and how to install or repair it. A maintainer can prove no shadow or stale
native owner remains.

**Why this priority**: Extraction without clear ownership turns a missing plugin into a confusing
regression and can leave dead duplicate implementations that drift.

**Independent Test**: Run help/completion/plugin-list/doctor and source-boundary checks with each
plugin installed, missing, hash-invalid, SDK-incompatible, and under-capable.

**Acceptance Scenarios**:

1. **Given** an installed valid artifact, **When** ownership is inspected, **Then** the command is
   shown as plugin-owned and is not marked shadowed by core.
2. **Given** a missing or refused artifact, **When** its known verb is invoked, **Then** the error
   names the external package, source, refusal reason, and exact repair command.
3. **Given** the final source tree, **When** boundary checks run, **Then** no native dispatcher or
   product-specific implementation owns the extracted surfaces.

### Edge Cases

- An alias is installed but the canonical verb is not, or vice versa.
- A plugin-global flag (`-v` or `--help`) conflicts with a verb-owned first argument.
- A plugin is present but its artifact hash, SDK floor, capabilities, or source pin is stale.
- Native and plugin dispatch both claim the same verb during a staged rollout.
- Team state contains unknown fields, mixed legacy formats, stale panes, partial member failures,
  or a changed repo/worktree identity.
- A team action prepares multiple members but one later preflight or post-mutation confirmation
  fails.
- Consent/auth endpoints are unreachable, deny the request, or return malformed data.
- A provider is selected explicitly but its plugin is missing; no default-provider fallback is
  permitted after selection.
- A machine has no tmux server, an unreachable server, a stale socket, or a regular file at the
  socket path; extracted commands must preserve existing fail-closed distinctions.
- A plugin host call times out after a non-idempotent request may have been accepted; the guest
  must not retry onto another route and risk double application.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST keep `wake`, `attach`, and `a` registered and implemented natively.
- **FR-002**: Extraction slices MUST preserve the frozen wake/attach observable contract for all
  non-Codex paths and for Codex when the accepted provider is installed; missing/refused Codex
  selection MUST use only the approved pre-mutation repair error and MUST NOT fall back.
- **FR-003**: The system MUST route `bring` and `b` exclusively to one installed external artifact
  after bring cutover.
- **FR-004**: External bring MUST preserve every accepted argument, default, text/JSON output,
  help behavior, and error/exit contract approved by its parity fixture.
- **FR-005**: The system MUST route `split` exclusively to one installed external artifact after
  split cutover.
- **FR-006**: External split MUST preserve target/direction/percentage/command/dry-run behavior and
  reject unsafe input before any tmux mutation.
- **FR-007**: Plugin dispatch MUST allow a verb to own `-v` and help semantics when declared, without
  weakening universal behavior for plugins that do not declare them.
- **FR-008**: The native host MUST expose split only through a narrow typed action or exact safe
  native delegation, never a newly unrestricted raw tmux capability.
- **FR-009**: The existing external `team` package MUST be expanded rather than replaced by a second
  team package.
- **FR-010**: The team artifact MUST implement all canonical native team subcommands and documented
  aliases before native team dispatch is removed.
- **FR-011**: Team read-only operations MUST preserve ordering, liveness, legacy-state, and output
  behavior except for the separately frozen and human-approved inventory-failure fail-closed
  correction.
- **FR-012**: Team state writes MUST be confined to named team/vault/state roots or an exact
  digest-bound charter document, preserve unknown fields where the current contract does, and use a
  closed archive-then-remove operation for recursive teardown. Operator-supplied documents MUST be
  bound to a reviewed route/subcommand/argument role; implicit/single-team charter resolution MUST
  use only the frozen host candidates. Receipt-bound replace/archive-copy MUST reject stale or
  substituted paths before mutation. Root resolution/host construction MUST perform no filesystem
  writes, and a durable complete archive copy MUST precede the first source removal. Ambient
  path/directory access is forbidden. One native-sampled,
  fakeable invocation timestamp MUST drive guest timestamps because WASI/ambient clock is unavailable.
  Guest-writable state MUST NOT become later execution/member/worktree/repository authority: generic
  writes preserve a closed authority plus prompt/task/message/mission-content projection;
  authority- or instruction-changing writes use host-materialized
  issued refs/receipts plus a guest-inaccessible durable provenance record, and partial or stale
  record/state commits fail closed before any later ref is issued.
- **FR-013**: Team lifecycle and pane-submission operations MUST use versioned typed native actions with
  bounded primitive operations, validated team/member/pane/worktree identifiers, scoped payloads,
  recursion denial, and explicit confirmed/failed/ambiguous outcomes. The native implementation MUST
  be injected from maw-cli behind DTO/trait contracts so `maw-plugin-manifest` does not depend back on
  maw-cli; scoped guests MUST NOT receive arbitrary `maw.cli.run` argv or copy wake/attach/done/tmux
  internals. Operator-authored charter engine commands MUST use stale-resistant host-issued receipts
  bound to the same engine/file or archived member rather than literal guest command strings.
  Lifecycle/worktree operations MUST consume host-issued engine, member, pane, and existing/planned
  workspace refs; lifecycle prompts and instruction-bearing task/message/mission/shutdown writes MUST
  consume exact host-issued ContentRefs. Guest strings and display paths never become execution,
  repository, or agent-instruction authority.
- **FR-014**: Team multi-target operations MUST preflight all targets before payload mutation where
  the native contract promises all-or-none preparation, and MUST report partial outcomes truthfully.
- **FR-015**: Team adopt/invitation MUST use typed native peer-resolution/trust and consent requests,
  preserve host-owned pending request-id/pair-code/expiry display behavior, and MUST NOT grant the
  guest direct trust-store, peer-config, credential, or network authority.
- **FR-016**: `gather`, `scatter`, `swarm`, and `artifacts`/`artifact` MUST each receive explicit external
  package ownership, accepted-artifact evidence, downstream known-verb/help/completion cutover,
  native registration removal, missing/refusal parity, and wake/attach/full-gate evidence. Their
  required issued-pane layout, operator/built-in-bound batch launch, artifact-store, pane observation,
  and worktree effects MUST use typed native operations rather than raw tmux/process/CLI authority.
  Captured pane text MUST NOT cross to a guest. `wave` belongs only to the external Codex package.
- **FR-017**: The native core MUST expose a vendor-neutral engine-provider contract sufficient for
  native wake/attach orchestration; the provider may select only the executable identity already
  bound by the accepted descriptor and may return only operation-schema-valid argv and allowlisted,
  bounded, non-secret environment settings. Provider planning is a host-to-guest call to a dedicated
  export/fresh plan-only instance; path-valued environment settings MUST use host-issued root ids,
  maintenance is a typed native action, and provider code cannot call any CLI-route host operation or
  re-enter provider planning recursively. Native wake MUST snapshot the prospective target
  branch/commit directory config, invoke the provider exactly once before wake phase/state,
  worktree, or tmux mutation, revalidate the snapshot, and refuse a race instead of replanning after
  mutation. The existing single append-only CLI request/failure audit remains permitted and exact.
- **FR-018**: Codex-specific command construction, resume/profile/account policy, and workflow UX
  MUST move behind the external provider/workflow boundary before the epic closes.
- **FR-019**: The external Codex artifact MUST preserve `codex accounts`, `more`, and `wave` behavior
  except for separately frozen and human-approved More/Wave pane-read and account-occupancy
  probe-failure corrections. Approved occupancy failure renders `unknown`, is excluded from `--free`,
  and is never reported as `free`.
  Account occupancy MUST use a manifest-declared, host-validated generic descriptor and opaque native-
  resolved account identifiers; raw process environment values, tokens, and arbitrary paths MUST NOT
  cross the boundary. Git/worktree creation, status, prune/archive/removal, and provider update MUST
  use typed native operations rather than raw process execution; worktree create/inspect MUST own the
  engine marker and branch facts required by more. New More/Wave worktrees MUST use a non-mutating
  planned-workspace then revalidated create flow. Wave mission writes MUST resolve a persisted
  host-issued repository identity to an invocation-local mission-store ref rather than trust stored
  absolute paths. Frozen accounts JSON may receive display-only pid and canonical home fields only
  when no host operation accepts them as authority.
- **FR-020**: Explicit Codex selection with a missing/refused provider MUST fail before mutation and
  MUST NOT silently choose another engine.
- **FR-021**: Non-Codex wake/attach paths MUST remain usable without the Codex artifact installed.
- **FR-022**: Every extracted verb MUST be listed as a known external verb with a canonical package,
  source path, alias policy, route identity, and actionable install hint. One backward-compatible
  artifact MAY own multiple declared top-level routes only when the host supplies an unforgeable
  `invokedCommand` and validates every canonical/alias collision. External registry generation MUST
  emit every route deterministically to the same immutable package/pin rather than skip route-only
  manifests.
- **FR-023**: Missing, stale, hash-invalid, SDK-incompatible, and under-capable artifacts MUST fail
  closed and identify the reason without leaking secrets. Every new #963 typed/effectful, changed,
  `cli.routes`, or multi-workflow artifact MUST declare a hash-covered non-overlapping
  intent/resource/action-budget table. Native dispatch MUST derive the
  invoked intent from original argv, intersect static capabilities to that intent, issue only its
  operator-selected/document-bound targets, and refuse cross-intent or excluded same-team resources.
  Unmatched/ambiguous intent receives zero typed host grants; artifact-wide grants alone never
  authorize an operation. An immutable pre-#963 `cli.command` artifact MAY retain its static legacy
  semantics only while a host-recorded package path/source/hash row matches and it receives no new
  typed capability; eligibility cannot be copied or drifted.
- **FR-024**: Help, completions, plugin listing, and doctor output MUST agree on native versus plugin
  ownership.
- **FR-025**: A native dispatcher and an external plugin MUST NOT both be reachable owners of the
  same canonical verb after cutover.
- **FR-026**: Every external artifact MUST be committed with a matching SHA-256 manifest pin and
  pass external repository CI before downstream native removal. Every scoped Rust guest MUST build
  twice from clean outputs in the pinned environment; both outputs MUST be byte-identical to each
  other and to the committed, manifest-pinned artifact.
- **FR-027**: Every extraction MUST retain or add parity fixtures covering stdout, stderr, exit code,
  JSON, host-call ordering, and no-mutation failure paths.
- **FR-028**: Every downstream ownership change MUST rerun focused wake/attach regressions and the
  repository gate required by the constitution.
- **FR-029**: The final native source MUST contain only generic provider/host contracts for the scoped
  products, with no reachable Codex/team/bring/split product implementation.
- **FR-030**: Issue #963 MUST maintain a reviewed child ledger mapping each requirement to bounded,
  ordered, independently verifiable work.

### Key Entities

- **Native Kernel**: Trusted orchestration and host adapters that remain in the binary.
- **Workflow Plugin**: An installable, capability-declared external artifact that owns one or more
  explicitly declared canonical CLI routes and their aliases; each invocation carries host-owned
  route identity.
- **Engine Provider**: Vendor-specific policy used by generic native wake/attach orchestration.
- **Capability Grant**: A narrow permission binding one host action or named data root to a plugin.
- **Parity Contract**: Frozen observable behavior and side-effect ordering shared by native RED and
  external GREEN tests.
- **Artifact Pin**: The exact SHA-256 binding the manifest to committed `plugin.wasm` bytes.
- **Ownership Ledger**: The issue/spec record mapping verbs, aliases, source owners, target packages,
  prerequisites, fixtures, and cutover status.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: 100% of frozen non-Codex wake/attach tests and fixtures pass on every cutover tree;
  provider-present Codex rows retain parity; explicit missing/refused Codex rows fail before mutation
  with the one approved actionable error; unrelated optional-plugin absence causes zero change.
- **SC-002**: 100% of frozen bring and split parity rows pass through committed external artifacts
  before their native registrations are removed.
- **SC-003**: All 43 frozen non-help native team canonical/alias names, including `live-check` and
  `delete`, plus every companion canonical/alias including `artifact`, have an independently
  executable external acceptance row before their cutovers.
- **SC-004**: 100% of Codex-specific source/caller inventory rows are either moved to the external
  provider/workflow artifact or explicitly converted to vendor-neutral native interfaces.
- **SC-005**: Source-boundary checks report zero reachable duplicate native implementations for all
  extracted verbs after convergence.
- **SC-006**: Missing/refused-plugin tests cover every extracted canonical verb and alias and always
  return actionable nonzero errors rather than bare unknown-command output.
- **SC-007**: An automated policy test over every scoped external manifest rejects raw process-
  environment access, arbitrary `maw.cli.run` argv, unrestricted filesystem/process/tmux authority,
  wildcard workflow/pane actions, trust-store mutation, and authentication-secret access.
- **SC-008**: Every committed guest artifact matches its manifest SHA-256 and passes external CI;
  every downstream ownership PR passes the exact repository gates in the constitution.
- **SC-009**: Help, completion, plugin-list, doctor, and issue-ledger ownership agree for every scoped
  verb in an automated convergence matrix.
- **SC-010**: The full program closes with all Spec Kit functional requirements mapped to completed
  tasks and no CRITICAL/HIGH inconsistency left in the final analysis.

## Assumptions

- `Soul-Brews-Studio/maw-plugins` remains the canonical source for guest artifacts.
- Extracted workflows are installable optional packages rather than embedded release binaries;
  absence is handled by actionable known-verb guidance.
- The current CLI behavior at each child issue's frozen alpha SHA is the compatibility baseline,
  unless that child explicitly approves a behavior correction.
- The existing `team` package is the destination for `team`/`t`; `gather`, `scatter`, `swarm`, and
  `artifacts`/`artifact` are separately addressable external packages; `wave` is owned only by
  `packages/codex`.
- The native plugin loader/host and `maw-tmux` remain trusted Rust infrastructure under #911/#910.
- Provider-specific launch policy can be obtained through a narrow generic interface without moving
  the native wake/attach state machine.
- Security-sensitive host prerequisites land separately before any guest uses them.
- Mechanical artifact generation or native deletion may exceed the ordinary line budget only when
  the PR body explicitly isolates authored logic from generated/mechanical bytes.
- The one-time Spec Kit scaffold/program-documentation PR uses an explicitly reviewed diff-budget
  exception; all implementation slices retain the ordinary <=250 authored-line limit.
