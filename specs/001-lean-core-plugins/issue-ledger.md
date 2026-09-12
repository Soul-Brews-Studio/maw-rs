# Issue ledger: lean-core workflow plugins

Umbrella: [maw-rs#963](https://github.com/Soul-Brews-Studio/maw-rs/issues/963)

Planning base: `a17ad351ef9edc28dbd745c84fba584544f40966`.
This ledger is updated only with immutable merged SHAs, artifact SHA-256 values, exact gate evidence,
human-gate decisions, and manual issue closure state. `pending` is not evidence. Task ownership below
is exhaustive and unique; child issues may contain several predeclared PR slices.

## Canonical child ledger

| Order | Child | Task range | Direct dependencies | External artifact gate | State |
|---:|---|---|---|---|---|
| 1 | pending: Spec Kit/ADR/inventories/ledger | T001-T006 | #963 | n/a | pending |
| 2 | pending: native wake/attach/hey/peek/serve regression boundary | T073-T076 | 1 | n/a | pending |
| 3 | pending: route/flag/multi-command/result contracts | T007-T017 | 1 | n/a | pending |
| 4 | pending: base guest ABI/SDK/registry/policy/artifact harness | T018-T024 | 3 | base SDK accepted | pending |
| 5 | pending: injected lifecycle and pane-submit host | T025-T032 | 3 | n/a | pending |
| 6 | pending: typed split host | T033-T034 | 3 | n/a | pending |
| 7 | pending: pane inventory and no-text observation host | T035-T036 | 5 | n/a | pending |
| 8 | pending: named roots/input/context/consent/trust host | T037-T045 | 5 | human gate: filesystem symlink hardening | pending |
| 9 | pending: layout/batch/worktree/repo host | T046-T054 | 7,8 | n/a | pending |
| 10 | pending: provider/health/maintenance/occupancy host | T055-T069 | 8,9 | human gate: occupancy probe `free` -> `unknown` | pending |
| 11 | pending: complete typed ABI/SDK/CI | T070-T072 | 4-10 | typed SDK accepted | pending |
| 12 | pending: bring/b artifact and downstream cutover | T077-T083 | 2,3,4 | accepted first; no typed-host dependency | pending |
| 13 | pending: split/bud artifact and downstream cutover | T084-T090 | 2,3,4,6,11 | accepted first | pending |
| 14 | pending: team contract/read surfaces | T091-T097 | 2,4,7,8,11 | human gate: inventory failure | pending |
| 15 | pending: team plan/preflight/state/tasks | T098-T104 | 8-11,14,22 | partial source only; T100 after accepted provider | pending |
| 16 | pending: team messaging/ownership/teardown | T105-T112 | 5,8-11,14,15 | partial source only | pending |
| 17 | pending: team lifecycle/invite | T113-T119 | 5,8-11,14-16 | partial source only | pending |
| 18 | pending: team companions and complete artifact acceptance | T120-T126 | 6-11,14-17,22 | human gate: Swarm atomic-private state; accepted first | pending |
| 19 | pending: generic downstream Team/wave decoupling | T127-T129 | 5,7-11 | n/a | pending |
| 20 | pending: companion downstream tests/cutovers/deletions | T130-T137 | 2,18,19 | accepted first | pending |
| 21 | pending: atomic team/t cutover and grouped deletion | T138-T139 | 2,14-20 | accepted first | pending |
| 22 | pending: Codex accounts/more/wave/provider artifact | T140-T153 | 2,4,5,7-11 | human gate: pane-read failure; accepted first | pending |
| 23 | pending: Codex provider wiring/cutover/deletions | T154-T161 | 2,3,10,11,19,22 | accepted first; no Team-cutover dependency | pending |
| 24 | pending: ownership/security/docs convergence | T162-T166 | 12,13,20,21,23 | combined | pending |
| 25 | pending: exact-tree verification/review/release | T167-T171 | 24 | combined | pending |

### Dependency invariants

- Child 12 is the earliest product MVP. Its artifact needs only the base route/ABI chain; its
  downstream cutover also consumes the native wake/attach/hey/peek/serve baseline. It never waits for typed host
  children 5-11.
- Every artifact is merged, hash-pinned, directly invoked, CI-validated, and recorded accepted before
  its fresh downstream branch exists.
- Codex source/artifact work may proceed in parallel with Team. Child 23 waits only for the generic
  wave decoupling in child 19, not Team artifact/cutover/deletion children 18/20/21.
- T138 is the concrete atomic team/t ownership switch. T139 is a distinct deletion-only coordination
  checkpoint for one compilable grouped flat-include deletion after every external consumer is gone.
- Five behavior/security corrections are separate human gates: ordinary named-root/operator-document/
  archive-tree symlink hardening (child 8); occupancy probe failure `free` -> `unknown` (child 10);
  Team inventory failure becoming terminal (child 14); Swarm plain write -> atomic private 0600 state
  (child 18); and More/Wave pane-read failure becoming terminal (child 22). No child treats proposed
  approval as evidence; each records the exact approval/comment URL before implementing its correction.

## Predeclared PR-slice manifest

Every named slice is a separate <=250-authored-line PR unless explicitly marked as the one-time
T001 documentation/generated exception or a mechanical deletion exception. RED tests are captured
before implementation but become green in the same mergeable slice. If frozen-base measurement shows
a row cannot fit, the child issue must add smaller named slices before code begins.

| Child | Ordered bounded slices |
|---:|---|
| 1 | `GOV-docs`: T001-T006 docs/generated scaffold, ADR, inventories, task graph, ledger; one reviewed generated/program-doc exception with generated/copied/authored counts and no product code. |
| 2 | `NATIVE-wake`; `NATIVE-attach`; `NATIVE-boundary`; `NATIVE-evidence` (T073, T074, T075, T076). |
| 3 | `ROUTE-flags`; `ROUTE-schema`; `ROUTE-collisions-shadow`; `ROUTE-match-context`; `ROUTE-consumers-help-list`; `ROUTE-consumers-bind-json`; `ROUTE-exit-envelope` (T007-T017). |
| 4 | `BASE-fixture`; `BASE-sdk-core`; `BASE-sdk-invoke`; `BASE-ci-native`; `BASE-ci-wasm`; `BASE-registry`; `BASE-registry-tests`; `BASE-policy`; `BASE-policy-ci`; `BASE-host-artifact`; `BASE-known-ownership`; `BASE-help-diagnostics` (T018-T024). |
| 5 | `OPS-trait-default`; `OPS-runtime-injection`; `LIFE-dto-authority`; `LIFE-host-actions`; `LIFE-native-launch`; `LIFE-native-finish-wait`; `SUBMIT-dto-preflight`; `SUBMIT-host-outcomes`; `SUBMIT-native-adapter` (T025-T032). |
| 6 | `SPLIT-host-red-dto`; `SPLIT-host-register-sdk` (T033-T034). |
| 7 | `PANE-inventory-red`; `PANE-inventory-host`; `PANE-observe-red`; `PANE-observe-host` (T035-T036). |
| 8 | `ROOT-model-list`; `ROOT-resolution-display`; `INPUT-selectors-approval`; `INPUT-host`; `FS-atomic`; `FS-document-receipts`; `FS-archive-copy`; `FS-copy-remove`; `CTX-semantic-clock`; `CTX-authority-refs`; `AUTH-red-projection`; `AUTH-dto-register`; `AUTH-native-record-store`; `AUTH-state-record-transaction`; `AUTH-reconcile`; `AUTH-domain-lock-hook`; `AUTH-epoch-import`; `AUTH-reimport-cli`; `AUTH-content-peer`; `CONSENT-red-host`; `TRUST-red-host`; `CONSENT-native`; `TRUST-native` (T037-T045). |
| 9 | `LAYOUT-red-authority`; `LAYOUT-host`; `LAYOUT-native`; `BATCH-red-authority`; `BATCH-host`; `BATCH-native`; `WORKTREE-plan-create`; `WORKTREE-inspect-marker`; `WORKTREE-teardown`; `WORKTREE-native`; `REPO-issue-host`; `REPO-issue-native`; `MISSION-repo-identity` (T046-T054). |
| 10 | `PROVIDER-descriptor`; `PROVIDER-dto-candidates`; `PROVIDER-plan-only-denials`; `PROVIDER-runtime-stack`; `PROVIDER-selection-red`; `PROVIDER-validation`; `HEALTH-red-host`; `HEALTH-native`; `MAINT-red-host`; `MAINT-native`; `OCCUPANCY-red-host`; `OCCUPANCY-linux`; `OCCUPANCY-macos` (T055-T069). |
| 11 | `ABI-complete-fixture`; `ABI-registration`; then external `SDK-lifecycle`, `SDK-pane`, `SDK-tmux`, `SDK-roots-input`, `SDK-authority-content`, `SDK-layout-batch`, `SDK-worktree-repo`, `SDK-provider-health`, `SDK-maint-occupancy`, `SDK-ci-native`, `SDK-ci-wasm-registry` (T070-T072). |
| 12 | external `BRING-parser-render`, `BRING-artifact-accept`; downstream `BRING-tests-core`, `BRING-tests-edges`, `BRING-cutover`, `BRING-cleanup`, `BRING-evidence` (T077-T083). |
| 13 | external `SPLIT-parser-security`, `SPLIT-artifact-accept`; downstream `SPLIT-tests`, `SPLIT-bud-propagation`, `SPLIT-cutover`, `SPLIT-evidence` (T084-T090). |
| 14 | `TEAM-contract-gate`; `TEAM-rust-router`; `TEAM-list`; `TEAM-status`; `TEAM-history`; `TEAM-members`; `TEAM-liveness` (T091-T097). Every guest module is wired through production `src/lib.rs` in its own slice. |
| 15 | `TEAM-plan`; `TEAM-preflight-generic`; `TEAM-preflight-provider`; `TEAM-create`; `TEAM-load`; `TEAM-tasks`; `TEAM-assign` (T098-T104). |
| 16 | `TEAM-send`; `TEAM-inbox`; `TEAM-enter`; `TEAM-send-enter`; `TEAM-adopt-release`; `TEAM-reassign`; `TEAM-member-remove`; `TEAM-delete-rm`; `TEAM-prune-gc` (T105-T112). |
| 17 | `TEAM-spawn-plan`; `TEAM-spawn-state-life`; `TEAM-up-apply`; `TEAM-bring`; `TEAM-resume-plan-state`; `TEAM-resume-life`; `TEAM-down`; `TEAM-shutdown-archive`; `TEAM-shutdown-wait-clean`; `TEAM-invite`; `TEAM-invite-consent` (T113-T119). |
| 18 | `GATHER-source-artifact`; `SCATTER-source-artifact`; `SWARM-scaffold-parse`; `SWARM-plan`; `ARTIFACTS-scaffold-parse`; `ARTIFACTS-render`; `TEAMSET-router-policy`; `TEAMSET-artifact-accept` (T120-T126). |
| 19 | `DECOUPLE-ls-reader`; `DECOUPLE-invite-ids`; `DECOUPLE-wave-state`; `DECOUPLE-wave-lifecycle`; `DECOUPLE-wave-pane-worktree` (T127-T129). |
| 20 | `TEAMTEST-read-state`; `TEAMTEST-messaging-submit`; `TEAMTEST-lifecycle`; `TEAMTEST-companions`; `CUT-gather`; `CUT-scatter`; `DELETE-gather-scatter`; `EPOCH-swarm-writer-lock`; `EPOCH-swarm-hook-seal`; `CUT-swarm`; `CUT-artifacts`; `DELETE-swarm`; `DELETE-artifacts`; `COMPANION-evidence` (T130-T137). The two deletion PRs may use only their disclosed mechanical exception. |
| 21 | `EPOCH-team-writer-lock-a`; `EPOCH-team-writer-lock-b`; `EPOCH-team-hook-seal`; `CUT-team-t` (T138); `DELETE-team-component` (T139, one grouped deletion-only PR for the exact flat component with disclosed mechanical >250 exception); `TEAM-closeout` ledger evidence in T139. |
| 22 | `CODEX-inventory-gate`; `CODEX-scaffold-accounts`; `CODEX-accounts-table`; `CODEX-accounts-json`; `MORE-discover`; `MORE-worktree`; `MORE-status`; `MORE-update-life`; `WAVE-start`; `WAVE-heal`; `WAVE-status`; `WAVE-dispatch`; `WAVE-teardown`; `CODEX-provider-tests`; `CODEX-provider-export`; `CODEX-artifact-accept` (T140-T153). |
| 23 | `CODEX-downstream-tests`; `CODEX-more-test-convert`; `WAKE-provider-pure-plan`; `WAKE-provider-switch`; `EPOCH-wave-writer-lock`; `EPOCH-wave-hook-seal`; `CUT-codex-routes`; `DELETE-codex-accounts`; `DELETE-more-component` (grouped mechanical exception); `DELETE-wave`; one bounded residual-policy slice for each exact T160 file; `CODEX-evidence` (T154-T161). |
| 24 | `CONVERGE-ownership`; `CONVERGE-guides`; `CONVERGE-boundary`; `CONVERGE-analysis`; `CONVERGE-ledger` (T162-T166). |
| 25 | external `FINAL-plugin-ci`; native `FINAL-full-gate`; `FINAL-smoke`; `FINAL-review`; `FINAL-release` (T167-T171). |

## Evidence schema

For each child and each named PR slice record:

- frozen repository/base SHA and branch creation time;
- authored/generated/mechanical line counts and any approved exception;
- RED command/output/log hash and exact final focused commands (twice where required);
- isolated NVMe quick gate and unpiped/non-PTY full gate for every maw-rs PR slice, including
  governance, ledger, documentation, convergence, and deletion slices;
- full external CI, committed artifact SHA-256, manifest/registry pin, and direct invocation for every
  maw-plugins acceptance;
- frozen diff SHA-256 and independent reviewer identity/verdict;
- missing/refusal, aliases, output/exit, no-mutation, and wake/attach evidence;
- human gate issue/comment link where a behavior correction applies;
- downstream merge SHA and manual issue closure state.

## Child-25 immutable exact-tree evidence record

T166 creates the final in-tree ledger snapshot. T167-T171 run only against its unchanged candidate
trees and append evidence to issue #963 (or an immutable externally-addressable attestation linked
there), never back into the reviewed/released tree. The record must identify: maw-rs alpha SHA and
tree hash; maw-plugins main SHA; registry SHA and every scoped package/artifact SHA-256; T167
toolchain/build/CI/direct-invocation log hashes; T168 full-gate log hash; T169 smoke result hashes;
T170 reviewer identity/verdict and frozen diff hashes; and T171 canary candidate/version/pin/smoke
evidence (including browser-equivalent God WebSocket `maw.ws.v1` and `Sec-WebSocket-Accept`
assertions with observed version/echo/accept values), release/tag/closure comment URLs.
Any later ledger update is a separate docs-only audit PR and cannot claim to prove that candidate.
