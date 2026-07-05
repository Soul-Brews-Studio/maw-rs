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
maw work <repo|url|.> --wt <name> --fresh -e <engine>           # 1. creates agents/<N>-<name>, branch agents/<N>-<name>, and boots the window
maw peek <session>:<FULL-window-name>                           # 2. confirm engine idle prompt (gpt-5.5), not shell/trust/update
maw hey <session>:<FULL-window-name> "<task + done-criteria>"   # 3. dispatch (v26.7.3+ hey confirm-submits itself — no send-enter needed)
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
| new | **`Error: turn/start failed in TUI`** — codex TUI dies to shell (seen after a hey lands on an errored session); the branch work survives, but the new mission is LOST | `maw peek` first; recover via **Respawn** below. Any hey sent into the dead TUI must be re-sent after respawn |
| new | worker relaunched with **bare `codex`** (no bypass flags) → sandbox pins writable-root to spawn cwd → every cross-worktree op = `[ ! ] Action Required` approval hell ("not YOLO", Nat 2026-07-03) | NEVER relaunch bare. Always the full engine string (below) or `maw wake -e codex`. Verify: footer must show `danger…` |

## Respawn a dead/non-YOLO worker (window already exists, pane at shell)
```bash
maw run <session>:<FULL-window-name> \
  'cd <ABSOLUTE-worktree> && OMX_AUTO_UPDATE=0 codex --search --dangerously-bypass-approvals-and-sandbox \
   "Read MISSION.md in this directory and execute the mission."'
maw peek <session>:<FULL-window-name>   # footer: engine idle + `danger…` = YOLO active
```
Same engine string as `commands.codex` in `~/.config/maw/maw.config.50.json` — keep them in sync.
Spawn IN the mission worktree (`cd` first); kickoff prompt as the positional arg.

## Preflight
`maw team preflight <team.yaml|team.json>` now runs the issue #43 crew-up gate offline: charter schema,
session/worktree ordering, `.maw-engine` command resolution, pool `access_token` expiry, actual
`CODEX_HOME` trust, CODEX_HOME isolation, and nested worktree collision checks.

The post-spawn boot verification remains a manual helper in the preflight output: run
`maw peek <session>:<window>` for each member and confirm the pane shows the engine idle prompt,
not a shell, trust prompt, or update prompt.

## Charter
`ψ/teams/<team>.yaml` (or `.maw/teams/`): `name`, `session`, `members[]` each with `role`, `name`,
`engine`, `cwd`/`worktree`. Per-member fields — do **not** use `defaults.worktree` (parser rejects it).

---
*Change history lives in git. For the freshest state (this doc can lag a hot change), ask maw-rs-oracle live.*
