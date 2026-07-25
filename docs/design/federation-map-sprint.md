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

**In progress — #17** (surface decision code): started, NOT yet edited. Plan: add `decision: Option<String>` to `PeerSendWireResponse` (`peer_http_transport_io.rs:42`) + to `PeerSendResponse` (`reqwest_peer_http_client.rs:37`); set `parsed.decision = wire.decision`; in the `status >= 400` branch (`reqwest_peer_http_client.rs:~131`) include decision + a hint. Decision codes: refuse-missing-peer-key / refuse-mismatch / refuse-unsigned / refuse-ambiguous-peer-key / refuse-skew / cache-no-sig.

**Remaining**: Truth #4 (probe resolved_ip/auth_ok/loopback — `peers.rs:234-240`, bigger), Wiring #16 (peer_pubkeys hot-reload — `serve.rs:277/:2765`), #6/#7/#8 (peers_probe_rows extract + unstub `federation_default_state` + prod-mount test), View #9/#10/#11/#15 (/fed page + /fed.json + `maw peers map` + fix door).

**Env surprises on black** (for next session): `rtk` NOT installed here; `rg` output is MANGLED (identifiers→`n`) — use `grep`/Read tool instead. `fd` absent — use `git ls-files | grep`. Cargo at `~/.cargo/bin` (export PATH). black is the ONLY machine with #665 built (`v26.7.23-alpha.1711-4-g3979e884`); m5 + GitHub release still buggy → cut a fresh alpha after fed-map merges (use `maw calver`, NOT skill `/calver`). Binary is named `maw-rs` not `maw` (`cp target/release/maw-rs ~/.local/bin/maw`).
