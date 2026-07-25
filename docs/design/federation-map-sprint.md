# Federation map sprint — design + backlog

**Status**: planned on m5 (2026-07-25), **to be executed on `nat@black.local`**
**Branch**: `agents/federation-map` (off `alpha` @ `3979e884`)
**Method**: `/oracle-plan` DNA sprint — 6 independent lenses + 3 adversarial verifiers (9 agents)

---

## Problem

`http://black.local:3456/` shows only *"maw-ui not installed. Run maw ui build or install maw-ui."*
The fleet has no way to see its own federation: which nodes exist, which are reachable,
what identity/version they run, and where handshakes fail.

## Baseline (measured 2026-07-25, before any change)

| what | value |
|---|---|
| `GET /api/federation/status` (m5 **and** black) | `{"local_url":"","peers":[]}` |
| `~/.maw/peers.json` | 9 peers |
| `maw peers probe-all` | **3.85 s**, 3 OK / 9 (white, white-lan, blackmachine) |
| `GET /api/ln/status` | 404 (never ported) |
| `GET /api/health` | `{"ok":true,"port":3456,"server":"local","source":"maw-rs"}` (constant) |
| `GET /api/transport/status` | `{"transports":[{"connected":true,...}]}` (constant `true`) |

## What the research found (all 6 lenses converged)

1. **The endpoint already exists and is a stub.** `serve_core/modules/federation_routes.rs:44`
   mounts `/api/federation/status`, but `federation_mount` → `federation_default_state()`
   (`:80-88`) hardcodes `local_url: String::new(), peers: Vec::new()`.
   Tests pass only because they call `federation_mount_with_state` (`:327`, `:397`) —
   **production mount is bypassed**. Same class as #524 "federation wake is fake".

2. **`maw ui build` is a dead end (2 layers).** `static_views.rs:47`
   `door_html_path: PathBuf::from("core/static/door.html")` is a **relative maw-js leftover**
   that does not exist in this repo → `views_door_response` always falls through to
   `VIEWS_INLINE_DOOR_HTML` (`:18`). Separately, serve reads `ui_dist_dir` = XDG
   `maw/ui/dist` (`:41-44`) while `maw ui` writes `cwd/.maw/ui` (`core_impl/maw_ui.rs:133`),
   and default mode only prints a plan (`maw_ui.rs:153-154`) — it never builds.
   **Installing maw-ui would not change `/`.**

3. **`identity.oracle` is fake fleet-wide.** `"mawjs"` seen on every peer is
   `PEER_DEFAULT_ORACLE` (`maw-peer/.../peer_store_types.rs:1`) / `SERVEIDENTITY_DEFAULT_ORACLE`
   (`serve_identity.rs:8`) — because **`/info` never returns an `oracle` field at all**
   (`serve_core/modules/info_routes.rs:37-56`). White and blackmachine both report `"mawjs"`.
   *Identity-mismatch detection is impossible today.*

4. **`node` is `"local"` everywhere.** Same fallback (`info_routes.rs:37-40`). This breaks
   `matches_local_peer` (`maw-transport/.../pair_health_classification.rs:34-43`) which
   `.find(node == local_node)` — it matches **someone else's row as "us"** → fake Healthy.

5. **`HeyConfig` already carries the real values** — `node` *and* `oracle`
   (`core_impl/send_federation.rs:39-41`, `load_hey_config()` `:1546`). serve already wires
   `agents_node` from it (`core_impl/serve.rs:441`) but **not `oracle`**, and `/info` falls
   back to `"local"` instead of the hostname.

6. **Fleet-in-node needs zero peer-side work.** Proven live: `GET <peer>:3456/api/sessions`
   returns each machine's sessions today — white → 3 (`05-volt`, `38-thongpraditbrewing`,
   `fb-serve`), black → 5 (`33-maw-rs`, `vt-*`), m5 → 20.

## Adversarial verdicts (3 verifiers, default-to-refute)

| claim | verdict | consequence |
|---|---|---|
| "inline page in the binary is the right call (not maw-ui)" | **SURVIVES** | maw-ui's own federation view fetches `/api/config`, which maw-rs does not implement (0 hits), and its type is only `{url,reachable,latency}` — it cannot show ip/oracle/version/auth |
| "serving the map at `/` unauthenticated is safe" | **REFUTED** | see security section below |
| "a map would have caught the 4 bugs" | **REFUTED** | not without new probe fields — see ordering rule |

### 🔴 Security finding (pre-existing, NOT introduced by this work)

`serve_api_token_gate` returns early for any path not starting with `/api/`
(`core_impl/serve.rs:504`) — so `/` can **never** be protected by `serve.token`.
Worse, no token is configured today (`serve: null` in both config layers) → `token: None`
→ the gate short-circuits **all** `/api/*` (`serve.rs:2717-2745`), and the default bind is
`0.0.0.0` (`serve.rs:25`).

Verified live from a LAN IP with **zero credentials**: `/api/sessions` → **200, 8488 bytes**
of every pane's cwd/pid/title; `/info`, `/api/message-ledger` → 200. Reachable over LAN,
WireGuard (`10.20.0.18`) and Tailscale. `peers.json` additionally holds `ssh`/`sshUser`
targets and WG addresses — a recon map.

**→ Redaction is mandatory in this work, and the posture decision (fail-closed / bind
127.0.0.1) belongs to Nat.** Filed separately (backlog #12).

### Ordering rule that came out of the verdicts

**Truth → wiring → view.** If the page ships before the probe learns `resolved_ip` and
an auth-path result, the map will show **green while broken**:
- `m5.local` resolves to `127.0.0.1` → the probe hits **our own serve** → 200 OK → green.
- `blackmachine` probes OK but `POST /api/send` → **401 `refuse-unsigned`**
  (`verify_protected_request`, `serve.rs:2493-2504`) — a different layer than `/info`,
  which is unauthenticated (`info_routes.rs:30`) and can never test the signed path.

## Design decisions

- **preact + htm (~6 KB) vendored**, not React UMD (~135 KB) and not a JSX build step.
  Component code is identical across all three, so this is reversible.
- **No CDN, no npm/bun build** — every byte vendored in the binary.
- **`/fed.json` served outside the `/api/` gate** (Mechanic's recommendation), because a
  browser on another machine fetching `/api/*` will 401 once a token exists.
  Consequence: `/fed.json` **must** apply redaction (`ssh`, `sshUser`, url→host) when the
  request is not loopback.
- **Server-side fleet aggregation** — serve collects each peer's `/api/sessions` during its
  probe cycle. Never let the browser fan out to peers (CORS + auth + N connections).
- **Cache-only reads** with `?probe=1` to force a live sweep. `probe-all` is sequential and
  writes `peers.json` through a fixed tmp path (`peers.rs:409`) — probing per page-load
  would race writers.

## Backlog (15 tasks, dependency-ordered)

### Truth — without these the map lies
1. **Baseline** — recorded above.
2. **`node` fallback `"local"` → real hostname** — `serve_identity.rs:8`, `info_routes.rs:37-40`.
   *Verify*: `/info` on m5 vs black returns different, real node names.
3. **`/info` returns real `oracle`** — wire `load_hey_config().oracle` into
   `ServecoreSharedState` (mirror `agents_node`, `serve.rs:441`) and into `info_payload`.
   *Verify*: white ≠ blackmachine (today both `"mawjs"`).
4. **Probe learns `resolved_ip` + `auth_ok` + loopback-self flag** —
   `peers.rs:234-240`. `rg 'resolved_ip|auth_ok|sendable'` = **0 hits** in the workspace today.
   Suggested `auth_ok`: a **read-only** gated GET (e.g. `/api/trust`) and record the status —
   never `POST /api/send`, which would deliver a real message.
   *Verify*: `m5` alias → loopback-self true; `blackmachine` → reachable true, `auth_ok` false.
5. **Fix `stale_age_ms` mixed timestamp formats** —
   `maw-peer/src/core_impl/display_validate_parts/peer_staleness_timestamps.rs:38`
   only parses ISO-8601, but `peers.json` stores **both** ISO (`"2026-06-02T13:54:44.148Z"`)
   and epoch-ms (`"1784953978566"`) → epoch-ms peers currently return `None` = "permanently stale".
   *Verify*: unit test both formats.

### Wiring
6. **Extract `peers_probe_rows() -> Vec<PeerProbeRow>` into `maw-peer`** — deterministic,
   fetcher injected, so CLI and serve share one path. Today probe only builds a `String`
   (`peers.rs:199`) and `--json` is ignored (`peers.rs:131-135`).
7. **Unstub `federation_default_state()`** — read the real peer store; extend
   `FederationStatusPeer` (`federation_routes.rs:110-118`, today
   `{url,node,reachable,latency,agents,clock_warning}`) with `oracle`, `version`,
   `resolved_ip`, `auth_ok`, `node_unique`.
   Prefer reading **per request with a short TTL cache** over baking state at mount time.
8. **Production-mount regression test** — assert through `federation_mount` (not
   `federation_mount_with_state`) that peers are non-empty when the store has entries.
   *Verify the guard*: revert #7 locally → this test must fail.
15. **Aggregate peer fleets into `/fed.json`** — pull each peer's `/api/sessions` during the
   probe cycle; store session name + live/idle/dead only.

### View
9. **`/fed` page + `/fed.json`** — preact+htm inline, one card per node with its fleet inside,
   expandable. Redacted off-loopback. *Verify*: open `http://black.local:3456/fed` from m5;
   then disable outbound network — it must still render (proves no CDN).
10. **`maw peers map` CLI** — same rows, terminal-first. (`maw federation-health` is a
   *formatter*, not a scanner: it takes url/node/reachable/latency from argv —
   `core_impl/federation_identity.rs:3`.)
11. **Fix the door** — point `/` at the map and stop advertising `maw ui build`.

### Filed separately
12. 🔴 **Security issue** — unauthenticated `/api/*` on LAN/WG/Tailscale (evidence above).
13. **Alias shadowing** — a local tmux session (`31-black`, the real `black` oracle) shadows
   the federation peer alias `black` in `maw hey` target resolution. Out of scope for the
   map (verifier confirmed `FederationStatus` has no representation for it) — fix in the
   hey/locate resolver. Same family as #665.
14. **Retro** — scorecard, expected vs actual.

## Mocks (real data, in `docs/design/federation-map-mocks/`)

- `fed-table.html` — dense table view
- `fed-diagram.html` / `fed.svg` — hub-and-spoke topology; the `m5.local → 127.0.0.1` trap
  renders as a **loop back into the centre**, which a table cannot show
- `fed-react.html` — the chosen direction: one card per node with the fleet inside
  (React UMD + htm in the mock; ship as preact + htm)

All three are generated from the live `peers.json` + a real `probe-all` sweep.

---

## Progress log (execution on nat@black.local)

**Done + committed + pushed** (branch `agents/federation-map`):
- ✅ **Truth #2 + #3** (`b71fec70`): `/info` returns real node hostname (not const `"local"`) + `oracle` field. Added `agents_oracle` to `ServecoreSharedState` (mirrors `agents_node`), wired from `load_hey_config().oracle` (`serve.rs:~441`). `info_payload` in `serve_core/modules/info_routes.rs` now takes `(node, oracle)`, falls back to `$HOSTNAME`, emits `oracle` when set. Tests + clippy green.
- ✅ **Truth #5** (`129e4e1c`): `stale_age_ms` parses epoch-ms too (was ISO-only → epoch-ms peers "permanently stale"). New `parse_timestamp_ms` (all-digit→epoch-ms else ISO) in `peer_staleness_timestamps.rs`. Test in `peer_store_mutation_tests.rs`.

- ✅ **#17** (`e50224d0`): surface federation `decision` on `/api/send` reject. Added `decision` to `PeerSendWireResponse` + `PeerSendResponse`; on `>=400`, `peer_send_error_message` appends `[decision=<code>]` + a `decision_hint` mapping each of the 6 refusal codes to its real cause (missing-peer-key → *restart after add* — folds the #16 finding into the hint). Server already emits it (`serve.rs:2515/2541`). 3 tests. Twin verified 49 tests green on m5.
- ✅ **Truth #4** (`b2c88fb2`): probe carries `resolved_ip` + `loopback_self` + `auth_ok` slot. `ProbePeerResult`/`ProbePeerPlan` gain the fields; `probe_peer_from_plan` overlays them on EVERY path (incl. failure) via a new `probe_peer_outcome` inner. `is_loopback_ip` inlined in maw-peer (no cross-crate dep). `peers_probe_peer` resolves the URL host to an IP (std DNS) → catches the `m5.local → 127.0.0.1` trap live. 4 tests. **auth_ok deferred to #7** (a read-only signed GET `/api/trust` — in the protected allowlist per `request_verify.rs:536-543`; deferred so a blind-signing bug can't paint the whole map falsely red, and #7 already extends `FederationStatusPeer.auth_ok`).
- ✅ **#16** (`74ee7fcf`): `peer_pubkeys` hot-reload. New reusable `HotReload<T>` (TTL 3s + `hot` flag + `frozen` for tests) in `serve.rs`; `resolve_request_cached_pubkey` reads `state.peer_pubkeys.get(now_secs)` instead of a Vec baked at boot → an added peer is trusted within 3s, no restart. 2 tests. **#7 reuses `HotReload` for the federation snapshot** (same freeze pattern).

**Checkpoint gate note**: a full-workspace `cargo test` run was contaminated because it compiled *while* serve.rs was mid-edit (lesson = memory `one-gate-per-target-dir`). Re-verified clean: 2 tests fail on this machine but are **pre-existing, non-hermetic** — proven by stashing my edits and running on the branch baseline (both still fail): `send_acl::sink_registry_preserves_audit_and_maw_log_bytes` reads the real machine oracle (`maw-rs`) for `from` instead of the fixture (`atlas`) — an env-leak; `serve_core::servecore_ws_pty_attaches...` is parallel-flaky (passes single-threaded). Neither touches the files this sprint changed. Candidate follow-up: make both hermetic.

**Messaging gap found**: `maw hey m5:33-maw-rs` / `maw hey m5` from black → `node 'm5' not in namedPeers or peers` even though `maw peers list` shows alias `m5` (node m5, 192.168.1.118). Cross-node hey resolves node from a `namedPeers` config that peers.json doesn't populate — a real federation-addressing gap, same family as #13 alias-shadowing. black→m5 pings currently fail (m5→black works); relaying via commit messages instead. Candidate follow-up.

**Remaining**: #6/#7/#8 (peers_probe_rows extract + unstub `federation_default_state` reusing `HotReload` + prod-mount guard test), View #9/#10/#11/#15 (/fed page + /fed.json + `maw peers map` + fix door).

**Env surprises on black** (for next session): `rtk` NOT installed here; `rg` output is MANGLED (identifiers→`n`) — use `grep`/Read tool instead. `fd` absent — use `git ls-files | grep`. Cargo at `~/.cargo/bin` (export PATH). black is the ONLY machine with #665 built (`v26.7.23-alpha.1711-4-g3979e884`); m5 + GitHub release still buggy → cut a fresh alpha after fed-map merges (use `maw calver`, NOT skill `/calver`). Binary is named `maw-rs` not `maw` (`cp target/release/maw-rs ~/.local/bin/maw`).

## Follow-up finding (#665 sibling — registry resolver) — 2026-07-25

After #665 shipped (v26.7.25-alpha.1308), `maw wake maw-rs` on a node with
MANY registry entries fails with a DIFFERENT error from a DIFFERENT resolver:

- repo resolver `wake_resolve_repo_target` (`wake.rs:811-814`) → `ambiguous fuzzy repo` — **fixed by #665** ✅
- registry resolver `wake_resolve_registry_target` (`wake.rs:787-790`) → `ambiguous registry target for maw-rs: 33-maw-rs:maw-rs, 33-maw-rs:maw-rs-oracle, 33-maw-rs:mawrs-codex-cli, …, inverted-pendulum-oracle:maw-rs` — **still broken**

Why `literal_name_tiebreak` (#665) doesn't help: registry `candidate.name` is the
full `session:window` (e.g. `33-maw-rs:maw-rs`), never bare `maw-rs`, so
`name.to_lowercase() == raw` matches none → still `Ambiguous`.

**Proposed fix (same crate as #665, maw-matcher)**: extend the tiebreak so the
**window part** (strip `session:` before comparing) is what's matched literally,
or make an exact window-name beat prefix/fuzzy. m5 repro'd it with 7 entries
(incl. `mawrs-codex-*` zombie-team leftovers). Workaround: `maw wake maw-rs --repo <org/repo>`.
→ NEW TASK #18, sibling of #665/#13.
