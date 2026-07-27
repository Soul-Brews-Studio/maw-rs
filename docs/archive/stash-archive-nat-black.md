# Stash archive — nat@black instance (found 2026-07-27)

> `noah` found 21 git stashes on the `nat@black` maw-rs checkout, dated
> **2026-06-18 → 2026-07-24**, never pushed and in no PR. Left alone they vanish
> when the instance is decommissioned. Per m5's rule — **preserve first, decide
> later, delete nothing** — this is the index. It records what each stash is, my
> verdict (only this instance has the context), and where the content lives.
>
> **Nothing was dropped.** All 21 stashes are still in the stash list, plus:
> - permanent gc-safe refs at each commit: `refs/stash-archive/00…20`
> - a lossless bundle + per-stash patches + this manifest, copied off the
>   scratchpad to `~/stash-archive-nat-black/` (survives to decommission).
>
> **The content is secured off-machine — this is resolved, not pending.** The two
> big stashes are a whole-tree delete and a base-mismatch snapshot (6.7M + 3.2M
> patches) and none of it is meant-for-public WIP, so the raw content is **not** in
> this public repo. m5 pulled it from `nat@black` and pushed it to the **private
> `maw-rs-oracle` vault repo → `ψ/archive/nat-black-stashes/` (commit `8659d6e`)** —
> 24 files on origin: the 23 patches, the 8.3MB lossless bundle, this manifest, and
> m5's `00-SCAN.md`. So even after `nat@black` is decommissioned, everything
> restores from the vault with `git bundle unbundle`. Only this index (secret-free)
> lives in the public repo. The local copy at `~/stash-archive-nat-black/` and the
> `refs/stash-archive/*` refs remain as redundancy until the machine is retired.

## Security scan — two independent rounds, same conclusion

Scanned **twice, independently**, each without seeing the other's result — a
stronger guarantee than one pass, because the two rounds keyed on different
evidence and still agreed: **no real secrets.**

- **nat@black (this instance):** scanned added **and** removed lines for private
  keys, real GitHub/Slack/AWS/bearer tokens, and env-value assignments. Matches
  were test fixtures only — `TEST_TOKEN_245` (a literal test placeholder) and
  `maw_auth_private_hmac_for_tests(secret, …)` (a test parameter) — plus source
  files named `token.rs`/`tokens.rs` (code that *handles* tokens, not values).
- **m5 (before committing to the vault):** found `CLAUDE_CODE_OAUTH_TOKEN` 10×, all
  **code** that emits `export`/`pass show` lines (not values), and one
  `federationToken` — a fixture for a fake node named `nova` in a test file that
  the stash was *deleting*, so that literal already lives in public repo history
  and nothing new is exposed. No `.env` / `auth.json` / `.pem` / `id_rsa` in any of
  the 23 diff headers.

Both rounds: safe. Kept off the public repo for size/appropriateness, not secrecy.

## The index (verdict + why)

Legend: **VALUABLE** = may hold unlanded work, review before deleting ·
**UNCERTAIN** = low confidence, worth a glance · **DEAD** = superseded / transient
/ base-noise by its own evidence; safe to delete after a second pair of eyes.

| # | date | on / message | size | SHA | verdict — why |
|---|---|---|---|---|---|
| 0 | 07-24 | alpha · WIP (matcher #665) | 938f −173039 | `bd56af7f8545` | **DEAD** — deletes the *entire tree* (ci.yml, .gitignore, AGENTS.md, everything, 0 insertions). A transient "rm everything" working state; the inverse of the repo, no content value. |
| 1 | 07-23 | (no branch) · help about-consent verbs | 3f +93 | `0d1bb4063e95` | **DEAD (likely)** — about-consent help verbs landed via **#663** ("describe all verbs"). Superseded WIP. |
| 2 | 07-23 | (no branch) · help about-consent verbs | 2f +92 | `1d22a56de495` | **DEAD (likely)** — near-duplicate of #1, minutes apart; same #663 supersession. |
| 3 | 07-18 | worktree-wf · Merge #585 | **1f wake.rs +632/−38** | `6482f566c293` | **VALUABLE** — the one real-content stash: 632 new lines of `wake.rs` on a workflow worktree that never became a PR. **Diff against current `wake.rs` before deciding** — this is the only one likely to contain work that never landed. |
| 4 | 07-15 | worktree-wf · **stale-base-edits** | 1f +15/−8 | `d5a73c143df4` | **DEAD** — labelled stale-base by its author. |
| 5 | 07-12 | alpha · **superseded by #428** | 1f +11/−5 | `86f81794aa62` | **DEAD** — label says superseded by #428. |
| 6 | 07-11 | bud-session-prefix · wip-386-pr2 | 2f +95/−7 | `a8aa9758699e` | **DEAD (likely)** — #386 (bud session prefix) landed; PR-2 WIP superseded. |
| 7 | 07-11 | feat-372b-token-verbs · preexisting-dirty-before-386 | 5f +251/−56 | `eb1a82ee76a8` | **UNCERTAIN** — "preexisting dirty" captured before #386 on the token-verbs branch. If token-verb work is fully in `main`, dead; glance before deleting. |
| 8 | 07-09 | feat-318d-picker · wip-318d-picker | 4f +188/−59 | `158c6b4edd9d` | **DEAD (likely)** — #318 picker series landed. |
| 9 | 07-09 | fix-299-upsert-guard · before-305-scan-reality | 28f +112/−905 | `1aba7d839adf` | **DEAD (likely)** — WIP preserved before #305 superseded #299. |
| 10 | 07-09 | feat-318a-resolver-leaf · before-303-repair | 6f +304/−0 | `dc63af5266fc` | **UNCERTAIN** — all-insertion (+304) resolver-leaf WIP before #303. #318a landed, but all-new content is worth a glance for stray unlanded bits. |
| 11 | 07-08 | feat-318a-resolver-leaf · temp-318a-work | 21f +423/−70 | `5baf11d0f36d` | **DEAD (likely)** — "temp" work on #318a, superseded by the landed extraction. |
| 12 | 07-05 | main · claude-md-calver-fix | 566f +52158/−26516 | `537c07924384` | **DEAD** — whole-tree base-mismatch snapshot. Intent was a small CLAUDE.md CalVer edit; the stash captured a giant divergence. CalVer is already in `main`'s CLAUDE.md; the diff is base noise. |
| 13 | 07-05 | main · partial cherry-pick routing+tmux | 22f +18/−8817 | `383499bf2ad8` | **DEAD** — abandoned partial cherry-pick (mostly deletions). |
| 14 | 07-05 | main · routing cherry-pick test | 6f +2/−1562 | `0bf80975cf64` | **DEAD** — cherry-pick experiment artifact. |
| 15 | 06-18 | agents/1-codex-4 · native-whoami-before-commit | 7f +330/−27 | `9e0af1963d94` | **DEAD (likely)** — codex-team-era safety stash (throwaway `agents/*` branch); work landed via codex PRs or abandoned 6 weeks ago. |
| 16 | 06-18 | agents/1-codex-3 · native kill before alpha refresh | 6f +93/−9 | `f04e43d7b488` | **DEAD (likely)** — codex-era transient safety capture. |
| 17 | 06-18 | alpha · main-cleanup-before-codex5-merge | 15f +368/−49 | `7f6c6a2675de` | **DEAD (likely)** — cleanup captured before a codex5 merge; the merge is long done. |
| 18 | 06-18 | agents/1-codex-3 · native kill before alpha reset | 9f +97/−9 | `4840610ccebe` | **DEAD (likely)** — codex-era transient safety capture before an alpha reset. |
| 19 | 06-18 | agents/1-codex-4 · pre-alpha-reset | 1f +1/−1 | `0de70bead3d2` | **DEAD** — 1-line marker before an alpha reset. |
| 20 | 06-18 | agents/1-codex-5 · runtime-marker | 1f +1/−1 | `5a82f4bfc565` | **DEAD** — 1-line runtime marker. |

## Recommendation

**One stash is worth a human's time: #3 (`wake.rs +632`, `6482f566c293`).** It's
the only real-content stash on a workflow worktree that never became a PR — diff it
against current `wake.rs` to see if any of those 632 lines are still missing.
**#7 and #10** deserve a 30-second glance. Everything else is dead by its own label
(superseded / stale-base / temp / pre-reset) or is a whole-tree / delete-everything
artifact with no content value.

But per the rule: **I did not delete any of them.** They're recorded here with the
reason each is dead, and left in place for a second decision-maker to action. To
inspect any, in order of durability:

1. **From the vault (survives `nat@black`):** the private `maw-rs-oracle` repo,
   `ψ/archive/nat-black-stashes/` — `git bundle unbundle stashes-nat-black.bundle`,
   or read the per-stash `.patch` directly.
2. **On `nat@black` while it exists:** `git show <SHA>`, or the `refs/stash-archive/*`
   refs, or `~/stash-archive-nat-black/`.

Every stash is addressable by the SHA in the table above, in either location.
