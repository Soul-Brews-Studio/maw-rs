# Closed but NOT fixed — 23 live defects

Closed 2026-08-17 as an explicit queue clear-out at the repo owner's instruction.
Every one below still reproduces on `alpha`. Each GitHub issue carries a close
comment restating its mechanism and repro, so reopening is cheap: `gh issue reopen <N>`.

## Security (5)

| # | defect |
|---|--------|
| 828 | `/ws` accepts unauthenticated upgrades and reaches `tmux send-keys` — no config mitigation |
| 709 | `maw hey` executes its text as shell commands on a non-agent pane (reproduced; branch `fix/709-send-text-guard` exists at a1408968, 129 commits stale, needs reconstruction) |
| 819 | `maw peers add` stores an identity that can never match inbound; the drafted fix could fabricate `<old-oracle>:<new-node>` and silently break auth |
| 817 | `/api/pair/generate` publishes `node:'local'` and port 3456 regardless of config |
| 203 | fork re-sync coordination (@tonkmac) |

## Correctness (10)

818, 821, 826, 829, 810, 814, 733, 683, 623, 763

## Test / infra (8)

851 (415 dirs leak per run; guard landed in #853, migration not done), 824, 809, 808, 815, 841, 842, 546

---

Fixed and verified today: 757, 838, 823, 840, 822, 787, 732, 839, 742.
