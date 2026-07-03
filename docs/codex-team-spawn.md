# Codex-team spawn path (maw-rs) — canonical

> **Single source of truth.** maw-rs changes fast; do **not** copy this into other oracles' memory —
> link here, or ask live: `maw hey 188-maw-rs:maw-rs-oracle "current codex-team spawn path?"`.
> Maintained by **maw-rs-oracle**. Last verified against **v26.7.3** (2026-07-03).

## TL;DR
- **Engine names resolve natively** (instance-config #2 / PR #49): `codex`, `omx`, `omx-resume`, `codex-t3`, `codex-t5`, … from `~/.config/maw/maw.config.<N>.json`. No base-config bridge needed.
- **Native `maw team` verbs exist**: `up plan preflight check apply bring down reassign liveness enter shutdown …`. `maw team up --dry-run` is reliable.
- **Reliable spawn = per-worker `maw wake`** (Path B) — avoids the `team up` targeting bug (#41).
- **Always address panes by FULL window name**, never numeric index (#34/#42).

## Path A — native `maw team up`
```bash
maw team up <team> --dry-run     # reliable: shows roster + resolved engines
maw team up <team>               # spawns
```
⚠️ **#41**: `team up`'s `new-window -t <session>` is colon-less and can prefix-match the **wrong session**
(e.g. session `maw-rs` vs `188-maw-rs`). Use an unambiguous numbered session (`NNN-oracle`) or use Path B.

## Path B — per-worker native wake (recommended; avoids #41)
```bash
codex update                                                   # 0. avoid the version-update prompt blocking boot
git worktree add agents/<name> -b agents/<name> origin/<base>  # 1. worktree first (verify it exists)
maw wake <oracle>-codex-N --no-attach --session <NNN-oracle> \
     -e <engine> --repo-path "$(pwd)/agents/<name>"            # 2. ABSOLUTE repo-path (#95: relative double-cds → engine dies silently)
maw peek <session>:<oracle>-codex-N                            # 3. confirm engine idle prompt (gpt-5.5), not shell/trust/update
maw hey <session>:<FULL-window-name> "<task + done-criteria>"  # 4. dispatch (v26.7.3+ hey confirm-submits itself — no send-enter needed)
```

## Gotchas under maw-rs (current)
| # | Gotcha | Do |
|---|---|---|
| #34 | Numeric index mis-routes dispatch | **Always FULL window name** |
| #41 | `team up` colon-less new-window hits wrong session | Unique `NNN-oracle` session, or Path B |
| #42 | `hey <session>:1` → matches `…codex-1` by name | Full name only |
| #35 | ~~idle-pane hey doesn't submit~~ **FIXED v26.7.3** (#61+#87 confirmed-submit: settle→Enter→verify→retry, dup-safe) | nothing — hey self-submits; send-enter only as manual fallback |
| #95 | `wake --repo-path <relative>` double-cds → engine dies to silent shell | use ABSOLUTE --repo-path (fix in flight) |
| new | codex **version-update** prompt blocks boot | `codex update` before spawning (0.142.5+) |
| new | codex **trust** prompt | handled by the bypass engine (`--dangerously-bypass-approvals-and-sandbox`); no pre-trust needed |
| new | safety hook blocks raw `tmux send-keys` | use `maw send-text` / `maw send-enter` |
| #48 | fleet-dir divergence (fixed) | loader reads both `~/.maw/fleet` + state dir |

## Preflight
`maw team preflight` **verb exists**, but the full crew-up 33-gotcha checklist (charter-schema reject,
trust/pool-auth check, worktree-exists-before-window, `.maw-engine` wake) is **not fully ported yet**
(tracked: maw-rs#43). Not yet a full canary gate.

## Charter
`ψ/teams/<team>.yaml` (or `.maw/teams/`): `name`, `session`, `members[]` each with `role`, `name`,
`engine`, `cwd`/`worktree`. Per-member fields — do **not** use `defaults.worktree` (parser rejects it).

---
*Change history lives in git. For the freshest state (this doc can lag a hot change), ask maw-rs-oracle live.*
