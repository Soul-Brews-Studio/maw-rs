# Implementation Plan: Lean-Core Workflow Plugins

**Branch**: `agents/spec-963-lean-core-plugins` | **Date**: 2026-08-20 |
**Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-lean-core-plugins/spec.md`

## Summary

Move optional workflow and vendor-specific behavior from `maw-rs` into reproducible,
capability-gated artifacts in `Soul-Brews-Studio/maw-plugins`. Land narrow host/runtime
dispatch/result and the capability-free guest SDK prerequisites first, implement and differentially verify each external artifact while native
dispatch still owns production, then cut over one canonical verb at a time. `wake`, `attach`, `hey`,
`peek`, `serve`, the plugin host, and raw tmux/process/filesystem authority remain native. Bring is the first MVP;
split follows only after flag and typed-mutation prerequisites; team and Codex move in bounded
user-story trains. Scoped guests use injected typed lifecycle, pane inventory/observation, layout,
batch-launch, named-root/input, opaque execution/workspace authority, worktree/repository, provider,
and occupancy APIs rather than
arbitrary CLI argv. `wave` belongs only to
`packages/codex`, whose final combined artifact is accepted before its downstream branch exists.

## Technical Context

**Language/Version**: Rust 2021 on pinned Rust 1.97.1 for `maw-rs`; Rust/Extism guests through the
reviewed external `maw-guest-sdk`; existing AssemblyScript/WASM through `@maw-rs/wasm-sdk` where
retained; TypeScript/Bun only as a development/reference rung.

**Primary Dependencies**: existing `maw-plugin-manifest`/Extism host, `maw-tmux`, external
`maw-plugins`, `maw-crates` parser leaves, serde/JSON contracts, GitHub Spec Kit 0.16.5.

**Storage**: existing XDG/Oracle/team JSON files and plugin manifests/artifact pins; no new
database. Guests see only named roots and typed host results.

**Testing**: Rust unit/integration/fixture tests, external plugin artifact tests and SHA-256 CI,
differential native-vs-plugin fixtures, maw-js JSON fixtures, `scripts/gate.sh quick/full`.

**Target Platform**: Linux and macOS native host; WASM guest artifacts. Platform-specific proof
must fail closed where a typed host operation cannot be proven.

**Project Type**: paired-repository CLI + native host + WASM plugin monorepo.

**Performance Goals**: extracted pure/read-only commands add no more than one plugin startup;
multi-target actions retain bounded current behavior and never trade correctness for retry speed.

**Constraints**: <=250 authored changed lines per PR; no `ψ/`; no raw tmux guest escape hatch; no
raw `/proc` or auth secret exposure; artifact-first paired PRs; wake/attach observable behavior is
frozen; external checkout must be clean/isolated before edits. The initial generated Spec Kit/program-
documentation PR uses one explicit reviewed diff-budget exception with generated/authored counts;
implementation PRs do not inherit it.

**Scale/Scope**: 4 canonical product areas (`bring`, `split`, team companions, Codex provider and
workflows), exactly 43 native team names, companion aliases, two repositories, and a
multi-PR issue train rather than one release-sized change.

## Constitution Check

*GATE: passed before Phase 0 and re-checked after Phase 1.*

| Principle | Plan evidence | Status |
|---|---|---|
| Stable Native Kernel | `wake`, `attach`, plugin host, transport auth, and raw adapters are explicit non-goals; every cutover reruns their regressions. | PASS |
| Plugin Ownership | Target ownership ledger moves optional/vendor workflows into external packages with no reachable native duplicate after cutover. | PASS |
| Behavior-Frozen Extraction | Every story requires native RED/differential fixtures and committed-artifact GREEN before removal. | PASS |
| Fail-Closed Capability Boundary | Contracts are typed, named-root, executable-bound, and secret-safe; missing/refused plugins are tested. | PASS |
| Source-Proven Artifacts | Scoped Rust guests use a pinned build environment; two clean byte-identical rebuilds must equal the committed SHA-pinned artifact before downstream cutover. | PASS |
| Small Verified Slices | Implementation tasks are child-issue/PR sized; the one-time generated Spec Kit/docs excess is explicitly counted and requires review approval. | PASS |

Post-design check: the contracts introduce no unrestricted capability, duplicate owner, or
wake/attach implementation. The one-time generated Spec Kit/program-documentation excess is a
disclosed docs-only exception; implementation remains decomposed under the normal budget. **PASS**.

## Project Structure

### Documentation (this feature)

```text
specs/001-lean-core-plugins/
├── spec.md
├── plan.md
├── research.md
├── data-model.md
├── quickstart.md
├── contracts/
│   ├── cli-ownership.md
│   ├── host-capabilities.md
│   └── provider.md
└── tasks.md
```

### Source Code (paired repositories)

```text
# Soul-Brews-Studio/maw-rs (trusted native host)
crates/maw-cli/src/core_impl/
├── dispatcher.rs
├── plugin_known_verbs.rs
├── session_list_plan.rs       # bring owner removed at bring cutover
├── split.rs                   # deleted at split cutover
├── team_*.rs                  # removed only after full team artifact parity
├── codex_accounts.rs          # removed at Codex workflow cutover
├── more*.rs / wave.rs         # moved together to packages/codex after accepted artifact
├── wake*.rs                   # preserved native
├── attach*.rs                 # preserved native
├── send_federation.rs         # `hey` preserved native
├── tmux_peek.rs               # `peek` preserved native
└── serve_daemon.rs            # `serve` preserved native

crates/maw-plugin-manifest/src/core_impl/
└── wasm_host/                 # narrow typed host/runtime prerequisites only

packages/wasm-sdk/             # typed guest SDK bindings
crates/maw-cli/tests/          # CLI, fixture, missing/refused-plugin, wake/attach regression
crates/maw-cli/src/core_impl/plugin_workflow_host.rs # production injected host-ops adapter

# Soul-Brews-Studio/maw-plugins (guest source and artifacts)
crates/maw-guest-sdk/           # shared Rust typed host ABI wrapper
packages/bring/                # new pure plan artifact
packages/split/                # new typed split artifact
packages/team/                 # existing artifact expanded in place
packages/gather|scatter|swarm|artifacts/ # separate team companion owners
packages/codex/                # codex, more, wave, and provider destination
registry.json
```

**Structure Decision**: Keep trusted orchestration and host adapters in `maw-rs`; keep every guest
source/artifact in `maw-plugins`. Shared generic parsing or policy belongs in the lowest existing
leaf only when both repos genuinely consume it. No third runtime repository is introduced.

## Delivery Phases

### Phase A - Governance and Inventory

1. Land Spec Kit scaffolding and feature artifacts in a docs-only PR for #963 that explicitly
   requests the one-time generated/docs exception and reports exact generated versus authored lines.
2. Freeze exact ownership/caller/fixture matrices for bring, split, team companions, and Codex.
3. Record ADR reversals of #743/#758 (native team), #273/#282 (codex accounts), and #174/#214
   (`more` core placement), including the fixed wave/Codex and companion package assignments.

### Phase B - Base Guest and Foundational Runtime Contracts

1. Make universal plugin flags manifest-declarable so split owns first-argument `-v` and its help
   compatibility without changing other plugins.
2. Add backward-compatible multi-command manifest routing with host-owned `invokedCommand` and
   collision validation.
3. Freeze the existing invoke/context/result route ABI, bootstrap the capability-free Rust guest SDK
   and native/wasm CI, pin the build environment and byte-reproducibility check, make external
   registry generation route-aware, and restore committed-artifact host invocation. This is
   sufficient for Bring and does not imply any typed host function. For every later cross-surface
   ABI or capability evolution, use client-first ordering: ship additive guest/SDK compatibility,
   run it against the current host, then enable the host addition or tightening only after the
   compatible client artifact is accepted and pinned.
4. Add generic injected lifecycle/pane-submit operations: DTO/traits in maw-plugin-manifest and the
   production adapter in maw-cli, never a reverse crate dependency or self-spawn escape hatch.
   Lifecycle consumes host-issued workspace, engine, and member refs rather than guest strings.
5. Add `maw.tmux.split.v1`, issued-pane/cwd-ref inventory, and manifest-bound boolean pane observation as
   independent slices.
6. Add typed layout transactions, EngineRef-bound batch launch, non-mutating planned-workspace then
   create/inspect/marker/branch management, durable repo-identity/mission-store resolution, and
   bounded current-repository issue context.
7. Add primary/fallback named roots, bounded root enumeration/display, atomic private writes,
   durable copy-before-remove, closed explicit/implicit document selectors, document/workspace/
   engine/member receipts, resolve-only host construction, one invocation clock, typed context,
   consent, and read-only peer trust facts.
8. Add plan-only host-to-guest provider planning with opaque issued root candidates,
   maintenance/health, and opaque occupancy before team
   preflight or Codex source depends on them.
9. Only after every typed host surface lands, publish the complete maw-rs ABI fixture and extend the
   already-landed Rust SDK/CI in bounded per-surface slices; first prove each additive binding against
   the preceding host fixture, then enable its host-side requirement. Typed guests cannot consume a
   surface before this conformance row.
10. Retain the forbidden-capability scan and committed-artifact invocation parity coverage that
    replace repo-split test debt (#546).

### Phase C - Bring MVP

1. Implement new external bring artifact from frozen native behavior with no host capability as soon
   as the base ABI/SDK/CI is accepted; it does not wait for unrelated typed host surfaces.
2. Verify committed WASM and aliases directly.
3. Cut over `bring`/`b`, add known-verb alias/install/help/completion behavior, remove only the
   adapter/parser dependency surface, and prove wake/attach unchanged.

### Phase D - Split

1. Implement and verify external split against the typed host/runtime prerequisites.
2. Rebind `bud --split` to the accepted Split owner while preserving its frozen best-effort,
   silently-discarded nested-result behavior; track any propagation change separately.
3. Cut over split and delete only `split.rs`; retain `maw-split` and shared `maw-tmux` actions.

### Phase E - Team, Codex Source, and Companions

1. Expand the existing team artifact read-only matrix.
2. Add state/task/member mutation slices with byte/unknown-field preservation.
3. Add mailbox state, lifecycle, and pane-submit slices through their distinct named-root and typed
   host contracts.
4. Add invitation through typed consent.
5. In parallel with generic Team slices, implement and accept the staged Codex/provider artifact;
   its accepted descriptor is required by Codex-specific Team preflight but its downstream wake and
   command cutover are not.
6. Only after that provider acceptance, complete Team provider preflight and accept separate gather,
   scatter, swarm, and artifacts/`artifact` packages through their typed boundaries with Team.
7. Cut over each companion group downstream with known ownership/help/completions, native deletion,
   missing/refusal parity, wake/attach regression, and full-gate evidence.
8. Keep wave native until the accepted Codex artifact's downstream cutover; move any team helper it needs
   behind a generic lower-level interface before deleting team product helpers.
9. Perform one atomic `team`/`t` dispatch cutover, then delete native files in mechanical slices
   after cross-consumer helpers have moved behind generic lower-level APIs.

### Phase F - Codex Provider and Workflows

1. Inventory every product-specific branch versus necessary generic compatibility detection.
2. Use the already-landed host-to-guest executable-bound provider, health/maintenance, worktree, and
   account-occupancy contracts; do not add a guest-to-host provider planning shortcut.
3. Externalize `codex accounts`, `more`, and `wave` into `packages/codex`; the artifact acceptance
   may occur during Phase E so Team preflight can consume the staged provider descriptor.
4. Move Codex launch/resume/profile policy behind the provider while retaining native wake/attach.
5. Build, pin, invoke, CI-validate, merge, and accept the final combined Codex artifact.
6. Only then create a fresh maw-rs branch, add downstream refusal tests, cut over canonical `codex`,
   `more`, and `wave`, remove native product code in bounded slices, and run full gates.

### Phase G - Convergence

1. Align help, completions, plugin listing, doctor, registry, aliases, and missing/refusal guidance.
2. Prove zero reachable duplicate native owners and zero unadjudicated Codex/team product logic.
3. Run combined exact-tree full gate and external artifact CI/pin checks; install that unchanged
   candidate as a canary and exercise wake/attach/hey/peek/serve (including the God origin) plus
   installed/missing/refused extracted-plugin smokes before cutting the CalVer alpha tag. The God
   canary is browser-equivalent: assert the installed binary commit, negotiated `maw.ws.v1`, and
   `Sec-WebSocket-Accept`, not merely a successful HTTP/WebSocket status line, and record each
   observed value in immutable canary evidence.

## Complexity Tracking

No implementation exception is accepted. The initial Spec Kit generated-scaffold/program-doc PR is
a one-time explicitly reviewed documentation exception and reports generated versus authored lines.
All product/runtime/artifact source tasks are split into <=250 authored-line PRs; generated
`plugin.wasm`, lockfiles, and separately disclosed mechanical deletions follow repository policy.
