# Contract: CLI Ownership and Cutover

## Ownership Table

| Surface | Current owner | Target owner | Native invariant |
|---|---|---|---|
| `wake` | maw-rs | maw-rs | Registration, callable implementation, fast/HTTP paths, and behavior unchanged. |
| `attach`, `a` | maw-rs | maw-rs | Dispatcher and binary TTY fast path unchanged. |
| `bring`, `b` | maw-rs plan adapter | `maw-plugins/packages/bring` | No native registration after cutover. |
| `split` | maw-rs `split.rs` | `maw-plugins/packages/split` | Shared/native typed tmux host remains. |
| `team`, `t` | maw-rs team router | existing `maw-plugins/packages/team` | No partial/shadow dispatcher after complete parity. |
| `gather` | maw-rs team companion | `maw-plugins/packages/gather` | Injected typed layout adapter remains. |
| `scatter` | maw-rs team companion | `maw-plugins/packages/scatter` | Injected typed layout adapter remains. |
| `swarm` | maw-rs | `maw-plugins/packages/swarm` | Operator-bound batch-launch adapter remains. |
| `artifacts`, `artifact` | maw-rs | `maw-plugins/packages/artifacts` | Named artifact-store host remains. |
| `codex` (`accounts`) | maw-rs | `maw-plugins/packages/codex` | Native host returns bounded occupancy facts only. |
| `more`, `wave` | maw-rs Codex/team workflows | `maw-plugins/packages/codex` | Generic wake/worktree/tmux host stays native. |
| Codex launch/resume/profile policy | maw-rs engine branches | external Codex provider | Native wake/attach state machine stays native. |

## Dispatch Rules

1. A canonical verb has exactly one reachable owner.
2. Native dispatch wins only for surfaces intentionally marked native.
3. Plugin aliases resolve through the same artifact and missing/refusal metadata as canonical names.
4. A backward-compatible manifest uses exactly one of legacy `cli.command` or ordered `cli.routes`.
   Every route contains `command`, optional `aliases`, `help`, and `flags`. The host rejects
   intra-manifest and runnable-plugin collisions and supplies the matched canonical
   `invokedCommand`; a native collision is an explicit staged-shadow diagnostic until cutover, and
   legacy single-command manifests retain their existing context.
5. Manifest metadata declares whether a plugin owns first-argument `-v` and its help contract.
   Undeclared plugins retain universal plugin version/help handling.
6. A missing known plugin returns nonzero and names package, source path, and repair command.
7. A discovered but refused artifact reports the refusal (hash, SDK, capability, manifest, or load)
   rather than degrading to unknown command.
8. Help, completion, plugin-list, and doctor ownership must match the dispatcher.

## Cutover Acceptance

- External merged artifact is SHA-pinned and CI-validated.
- The downstream cutover branch is created fresh only after the exact external artifact is merged,
  hash-pinned, directly invoked, CI-validated, and recorded as accepted.
- Committed-artifact parity suite covers canonical verb and every alias.
- Native owner is removed in the same downstream PR that activates known ownership.
- Search/compiler tests prove no second reachable owner.
- Missing/refused artifact cases are green after native removal.
- Wake/attach focused suites pass on exact downstream bytes.

## Compatibility Rule

Current alpha behavior is authoritative unless a child issue explicitly documents a correction,
its RED proof, and approval. Extraction alone does not normalize help codes, parser quirks, output,
ordering, or legacy formats.
