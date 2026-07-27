# Handoff: what the nat@black instance learned (2026-07-25 → 27)

> Written by the `nat@black` maw-rs instance as it winds down, for the primary
> (`maw-rs@black:3458`) and the orchestrator (`m5`). The PRs survive in git; this
> is the part that would otherwise die at `/clear` — the gating traps that only
> bite on a real dev machine, and the *why* behind decisions that a diff can't show.
> Most of it lived in this instance's private `~/.claude` memory, which no other
> instance can read; that's why it's here now.

## 1. Gating traps on the black machine (these cost real time)

**The local test suite has a false-red cluster (#688).** `cargo test --workspace`
on black fails ~10–11 `maw-cli` lib tests that are **green in CI**. They are
non-hermetic — they read the real repo name and real state dirs, so the fixture
`m5:atlas` comes back as `m5:maw-rs` (the checkout dir), `track` as `81-track`,
etc. The failing set: `attach_auto_picks_single_live_session…`,
`inbox_pending_*` (5), `locate_typed_inventory_routes…`, `send_acl_hotpath_*` (3).
- **The count wobbles 11↔10 between runs** — proof they're order-dependent, not
  deterministic.
- **Rule:** a change is regression-free iff it adds **no new** failures beyond
  this known set. Prove it by `git stash`-comparing (run the suspects on clean
  alpha), never by "the suite is red." alpha CI green + stash-compare clean =
  ship. Do **not** treat local full-suite red as a merge blocker on its own.

**The shipped Linux binary is musl; debug is glibc.** musl has no nss-mdns, so the
release binary **cannot resolve `.local`**; a debug build can. A fix "verified" on
debug can be dead in the shipped artifact. `curl` is glibc and resolves `.local`
even on black, so `curl` proving a hostname works proves nothing about the musl
binary. Point Linux peers at IPs / WireGuard, not `.local`.

**`rtk` mangles piped `ps`/`ls`/`wc` output** — it rewrites the stream and you can
read "0 processes" for a live build, or a header row where a path should be. Use
`rtk proxy <cmd>` or a dedicated tool when parsing output; don't trust a piped
`ps | wc` through the hook.

**`cargo clippy … | grep -E 'warning:|error'; echo $?` reports GREP's exit, not
clippy's.** grep exits 1 when it finds nothing → looks like a clippy failure when
clippy actually passed clean. Check the `Finished` line, or run clippy without the
pipe, before concluding.

**One gate per `CARGO_TARGET_DIR`.** Concurrent gates on a shared target dir
cross-contaminate at test-execution time and produce phantom `FAILED` from the
other tree. Serialize.

**Local gate ≠ CI gate.** The CLAUDE.md "quick" gate omits `cargo fmt --all
--check` **and** `--features wasm-host`. Run all four dimensions (fmt / test /
clippy / wasm-host, on stable) before claiming CI-safe. Per Nat, the **local gate
is the merge decision**; CI only guards — never wait on CI in the dev loop.

**git footguns seen this week.** `git add -- A B` where `B` matches nothing stages
**neither** (broke alpha once). A stacked PR whose base is declared `alpha` shows
the parent branch's commits in its diff and the merge button does the wrong thing —
retarget the base to the parent branch until the parent merges (#692), or the diff
lies.

## 2. Why the PRs are the way they are (the diff won't tell you)

**#687 (oracle "mawjs" masquerade).** Two independent constants fabricated
`oracle:"mawjs"` for every maw-rs node: `/api/identity` emit and the peer-store
parser. **All three display sites were already honest** (`unwrap_or("-")`,
`filter(!empty)→None`, `p.oracle||'—'`); only the two *sources* lied — so the fix
is at the sources, no view change. Chose to **omit** the field when unconfigured
(not emit `""`). A compat worry about maw-js parsers turned out moot: the "maw-js
nodes on the LAN" were a mirage — that `mawjs` value *was* the bug #687 killed;
the real nodes are maw-rs. Deliberate maw-js parity divergence: we emit nothing
rather than a plausible guess. A **third** `mawjs` default survives in the signing
path (`maw-auth` `DEFAULT_ORACLE`, #693) — left as a scope decision, not smuggled in.

**#677 / #689 (frozen node/oracle).** The issue's stated root cause — "`peers
probe` never writes back" — is **wrong**, and I proved it live: `maw peers probe`
advanced `lastSeen` on both black and m5. The writer exists and works. The real
gap: the serve federation map **fetches live for display but never persists**, so
`lastSeen` only moves on a manual probe. #689 adds a periodic sweep that persists
through the **one** existing writer (never the render path). Its lost-update race
fix is **reload-before-apply** (snapshot → probe → re-read fresh → write only
probe-owned fields), chosen over a lock file because it's simpler and can't
deadlock; a residual sub-ms window is documented in-code, not hidden.

**#676 (`ls --federation` 404).** `/api/ls` is implemented by **no serve, not even
localhost** — its response shape was *imagined*. The fix retargets the client to
`/api/sessions` (a real, bare-array endpoint), not a new endpoint. The test stub
had to become a bare array to actually guard the `as_array` parse branch — the old
`{sessions:[…]}` stub let you delete that branch with the suite staying green while
every real peer returned empty.

**#672 (notify to wrong inbox).** Scoped to **defect 1 only** (write to the
receiver's `ψ/inbox`, resolved via the registry, not the sender's). Defect 2
(`hey --inbox local:` injects into the pane) needs threading `inbox` through the
audited local send hotpath — bigger, deferred. Help text left **untouched** so it
doesn't advertise `--inbox` as working locally while defect 2 stands.

**#696 (pair generate panic).** `pair_http_json` did `block_on` on a runtime worker
thread → "cannot start a runtime from within a runtime". Fix mirrors
`peers_fetch_identity` (spawn a fresh thread). The panic→`Result` change **also**
fixes store consistency for free: `pair_http_json` runs *before* `pair_write_peer`,
so a clean `Err` means the half-write never happens. It was the **only** unguarded
production `block_on` (swept them all).

## 3. Fleet-structural facts the primary needs

- **Two serve processes on black.** `:3456` (nat, legacy) and `:3458` (maw-rs,
  canonical/systemd) report **different node identities** (`black` vs
  `maw-rs-black`) for the same machine. Validate any remote-durable fix against
  the canonical **:3458**, and make sure fleet registries point there — otherwise
  probes keep hitting the stale `nat`-owned process (a third "frozen identity"
  specimen, this time in *which OS process answers*).
- **Config store split.** Cross-node `hey` resolves the target from
  `maw.config.json → namedPeers`, a **different** store than `peers.json` (which
  the map/`peers` use). A peer can be up + aggregated yet unaddressable by `hey`.
  A **bare-name** `hey` loops back to self and reports success — always use the
  full `node:session:window` form.
- **Reporting channel.** Post via `gh issue/pr comment` — it persists across
  `/clear`; `hey` messages don't. This whole session's reasoning survives because
  it went to gh, not just the pane.
- **`maw pair` writes peer records** (`pair_write_peer`) — treat any failure in
  that flow as store-affecting, return `Result`, never `panic`.

## 4. The meta-lesson (the one worth keeping)

Both this instance and m5 **misdiagnosed twice each** this week, and both
self-corrected — every time by **real-invoke on the shipped artifact**, never by
re-reading the issue's stated cause. An issue's "likely cause" is a hypothesis, not
a fact; the codebase at the moment you touch it is the authority. When two
independent instances reach the same wrong conclusion, that's the "context is full"
signal — checkpoint, don't push. See `docs/principles/results-that-match-reality.md`
for the long form; this is the operational corollary: **verify done by running it,
distrust confident labels next to empty results, and write down what you learned
before the fork forgets it.**
