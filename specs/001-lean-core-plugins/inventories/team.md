# Frozen inventory: team and companions

Baseline: `maw-rs/origin/alpha` `a17ad351ef9edc28dbd745c84fba584544f40966`.

## Current ownership

Native `team`/`t` enters through `DISPATCH_240` and `team_run`. Seventeen `team_*.rs` files contain
about 6,156 lines. Fourteen `native_team_*.rs` integration suites and 54 golden files freeze behavior.
The existing external `maw-plugins/packages/team` v2.0.1 artifact is shadowed and implements only
default/list/ls; it must be expanded in place, not duplicated.

## Authoritative 43-name matrix

```text
create new list ls status tasks oracle-members members lives history plan preflight check load
spawn spawn-from send msg broadcast inbox invite adopt release up bring apply reassign
liveness live-check down remove delete rm prune gc shutdown resume enter send-enter add task done assign
```

The old external contract omits many of these and advertises stale `oracle-invite`/`oracle-remove`;
current alpha routing above is authoritative. `live-check` and `delete` are mandatory even though
older usage/artifact metadata drifted.

## Behavior clusters and required APIs

| Cluster | Names | Host boundary |
|---|---|---|
| read summary | list/ls/status | named tool-teams/team-vault/team-state roots |
| vault history | lives/history | bounded team-vault list/read + non-authority display path |
| member registry | members/oracle-members | merged primary/legacy team-state reads |
| pane liveness | liveness/live-check | bound charter selector + bounded pane inventory |
| preflight | plan/preflight/check | bound charter, pane/worktree facts, EngineRef/config-command health, typed provider facts |
| state/tasks | create/new/load/task/tasks/add/assign/done | named roots, atomic private writes, invocation clock |
| mailbox delivery | send/msg/broadcast/inbox | named roots + atomic/append state operations |
| pane submit | enter/send-enter | `maw.workflow.pane_submit.v1` |
| ownership/teardown | adopt/release/reassign/member-remove/delete/rm/prune/gc | receipt-bound charter/state, durable archive-before-remove, issued member/workspace/engine refs; only delete/rm are whole-team aliases |
| lifecycle | spawn/spawn-from/up/bring/apply/resume/down/shutdown | issued `LifecycleMemberRef`/`WorkspaceRef`/`EngineRef` plus injected lifecycle primitives |
| invitation | invite | typed consent request; pending remains exit 2 |

No guest receives arbitrary CLI argv, raw tmux, raw environment, generic vault, trust mutation, or
auth token bytes. Delivery work consumes the confirmed/ambiguous semantics from closed #951 and
merged #960 at the planning base so it cannot inherit a misleading
send-key success contract.

## State and context traps

- Native vault precedence is `MAW_STATE_DIR/team-vault`, then the compatibility input
  `MAW_RS_TEAM_PSI`, then repo-local `ψ`; existing plugin `maw.paths.get("vault")` is not parity.
- Tool teams live under `~/.claude/teams`; additional task/oracle state uses current MAW_HOME/XDG
  precedence.
- Writes require native atomic/private semantics and unknown-field preservation.
- Host/root construction is resolve-only; a read or validation failure creates no directory/file.
- Native tmux inventory failure currently becomes empty/missing state and can make up/apply fresh-wake
  apparent missing members. Replacing it with a terminal typed error is a separately frozen,
  human-approved correction; without approval liveness/up/apply artifact work remains blocked.
- Explicit operator and implicit repo charters are native-selected documents with digest receipts;
  approved mutations compare-and-replace only the same document. Symlink refusal is a separately
  approved fail-closed correction, not silent parity.
- One injected `nowMillis` drives all guest-created timestamps/ids; WASI clock remains disabled.
- Engine/cwd/member values never become guest strings with process/path authority. The host issues
  typed refs from exact argument roles, charters, named state, built-ins, providers, or markers.
- Semantic `MAW_TEAM`, consent mode, and session ids may cross only as typed context; no general env
  getter is allowed.

## Native cross-consumers to decouple before deletion

- `session_list_plan.rs` uses team helpers for `maw ls` annotations.
- `oracle_recruit.rs` reuses invite request-id/PIN helpers.
- `wave.rs` directly uses team types/state/lifecycle helpers and remains native until Codex artifact
  acceptance; it must first depend on generic lower-level helpers.
- completions and plugin-list tests currently encode native/shadowed ownership.

## Companion dispositions

| Surface | Native source/caller truth | Frozen proof | Required host action | External owner |
|---|---|---|---|---|
| `gather` | `core_impl/team_gather_scatter.rs` `DISPATCH_329`; no other production caller | in-file gather unit rows; no integration fixture exists yet | current pane + inventory + typed join/layout transaction | `packages/gather` |
| `scatter` | same dispatcher/source; no other production caller | in-file scatter unit rows; no integration fixture exists yet | current pane + inventory + typed break transaction | `packages/scatter` |
| `swarm` | `core_impl/swarm.rs` `DISPATCH_117`; direct split/layout/title/send mechanics and config write | `tests/native_swarm_plugin.rs`; `tests/fixtures/native-swarm/*`; serve-engine advanced-wake row | operator-intent/host-built-in EngineRef-bound batch launch + tool-teams atomic write | `packages/swarm` |
| `artifacts`, alias `artifact` | `core_impl/artifacts.rs` `DISPATCH_71`; both entries call the same handler | `tests/native_artifacts_plugin.rs`; `tests/fixtures/native-artifacts/list.json`; in-file parser/render rows | named artifact-store + legacy read fallback | `packages/artifacts` |
| `wave` | `core_impl/wave.rs` `DISPATCH_326`; direct team/worktree/lifecycle callers | no direct frozen integration fixture exists yet | planned-worktree + issued member/engine refs + durable repo identity/mission-store + pane observe/submit + provider | only `packages/codex`, later cutover |

Swarm's existing custom command/path input is preserved only when the native host proves that the
requested executable token exactly came from the host-owned original CLI position; a guest cannot
invent or extend it. Gather/scatter receive no raw tmux capability. Artifact alias `artifact` must be
present in manifest, known ownership, help/completion, committed-byte, and missing/refusal matrices.

Each companion requires accepted committed bytes, downstream known ownership/help/completion
cutover, native registration removal, missing/refusal parity, wake/attach regression, and full gate.
`assign` and `oracle-recruit` are not silently reclassified as team companions.
