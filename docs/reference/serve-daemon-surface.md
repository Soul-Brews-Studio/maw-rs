# serve-* Surface for native serve-daemon design (maw-rs #86-94)
> Gathered from maw-js source (read-only) 2026-06-25 for Bigboy's serve-daemon architecture draft → TK.

## TOP REFRAME
serve-* are **NOT standalone CLI commands**. They are **serve-lifecycle plugins** that all mount onto the *single* `maw serve` Bun HTTP+WS server. No `maw serve-ws` entrypoint. maw-rs #86-94 → 8 native **modules registering into one shared gateway**, not 8 processes.
- One server: `startBunGatewayServer` `src/core/server.ts:271-540` → one `Bun.serve({fetch,websocket})` (`:488`).

## 7 SHARED INFRA (the daemon must provide these)
1. **HTTP route registry** `src/core/serve-route-registry.ts` — `:param` matching (`:49`), dup-register throws (`:94`), plugin-scoped ownership via `forPlugin()` (`:66`). (axum Router IS this.)
2. **WS route registry** `src/core/serve-ws-registry.ts` — keyed by path, `handleUpgrade()` runs BEFORE HTTP.
3. **Request pipeline order** (`server.ts:390-449`): CORS preflight → WS upgrade → engine-plugin proxy → `/api` protected(AUTH gate)→registry → `/api` unprotected→registry → non-api→registry→fallback(serve-views).
4. **CENTRAL auth** `src/lib/elysia-auth.ts` — `isProtected(path,method)` (`:46`) = single source of truth. **Plugins have ZERO auth code.** Protected serve-* paths only: `/triggers/fire`, `/worktrees/cleanup`, `POST /plugins/*`. Everything else PUBLIC. Auth = HMAC-SHA256(method+path+ts, replay window) + ed25519 from-signing/TOFU (#804). Loopback bypass via real `requestIP()` only (header IPs distrusted #191). Fail-closed if peers configured w/o token.
5. **Shared trigger/feed bus** `src/core/runtime/triggers-engine.ts` — serve-triggers reads, serve-triggers-mutate fires.
6. **Shared MawEngine** (`server.ts:279`) — serve-ws uses; tmux/session state.
7. **Lifecycle mount** `src/plugin/lifecycle.ts:260` — sort by weight then name; `profile.apiRouters` whitelist = which modules mount (= the daemon's module-enable list); `views:false` drops views.

## PER-COMMAND TABLE
| # | cmd | route(s) | method | long-lived | auth | state |
|---|-----|----------|--------|-----------|------|-------|
| 86 | serve-agents | /api/agents,/api/agent | GET | one-shot | public | reads tmux+config |
| 87 | serve-debug | /api/plugins,/plugins(html) ; /api/plugins/reload | GET ; POST | one-shot | GET public, POST reload **PROTECTED** | PluginSystem stats |
| 88 | serve-federation | /api/federation/status, /api/peers/discoveries | GET | one-shot | public | transport router/discovery |
| 89 | serve-identity | /api/identity | GET | one-shot | **PUBLIC by design** (pre-auth peer discovery) | exposes node pubkey/TOFU; **embeds own Elysia** |
| 90 | serve-triggers | /api/triggers | GET | one-shot | public | trigger engine (read) |
| 91 | serve-triggers-mutate | /api/triggers/fire | POST | one-shot but **blocks on actions** | **PROTECTED** | fires/mutates trigger bus (side-effects) |
| 92 | serve-views | /topology,/,/* (fallback `*`) | GET | one-shot | public | **Hono+static** UI dist; only `http.fallback` user |
| 94 | serve-ws | /ws,/ws/pty,/ws/tmux | WS | **LONG-LIVED STREAMING** | none at WS layer (pre-gate) | MawEngine+PTY+tmux-stream; heartbeat idle-close |

(serve-worktrees #93: GET /api/worktrees public + POST /api/worktrees/cleanup PROTECTED; scans/removes git worktrees.)

## GATEWAY #22 — already exists in maw-js
- `src/core/gateway.ts` `selectGateway()` (`:50`): bun(default)|rust via `--gateway`>`MAW_GATEWAY`>`config.gateway`.
- **Rust gateway binary** `packages/maw-gateway/` (Cargo crate). `RustGateway.start()` (`gateway.ts:130-213`) starts Bun backend on PORT+1, spawns `maw-gateway serve --port PORT --backend PORT+1`, waits `listening on :PORT`.
- `GATEWAY_CONTRACT.md`: TODAY = **reverse proxy**. Rust owns listener; `GET /api/health` Rust-native; **everything else (all serve-* + /ws/*) PROXIED to Bun backend**; WS full-duplex proxied; narrow env allowlist (no secrets); loopback bind.
- **maw-rs target = CONSOLIDATE**: axum gateway (#22) HOSTS serve-* natively (replace Bun backend), with the 7 shared infra as what each native module registers into.

## HETEROGENEITY WARNING (native rewrite)
Most plugins return raw `Response`; **serve-identity embeds Elysia**; **serve-views embeds Hono+static**; **serve-ws** is WS + bootstrapped in server.ts (needs engine ref). Native rewrite should unify on **axum** handlers/extractors + **tower** middleware for the central auth gate.

## DESIGN IMPLICATIONS (Nova's read, for Bigboy's draft)
- ONE axum `Router`; modules register route subtrees (axum Router = the route registry).
- Central auth = **tower middleware layer** applying `isProtected()` before protected routes — DON'T put auth in modules (mirror maw-js exactly).
- WS via `axum::extract::ws` for /ws/* (long-lived); shared `Arc<Engine>` handle.
- Shared state via axum `State(Arc<...>)`: trigger-bus handle, engine handle, config. (No separate route registry needed — Router is it.)
- module-enable list = `profile.apiRouters` equivalent (config-driven which modules mount).
- serve-identity stays PUBLIC pre-auth (peer discovery) — never gate it.
- Reuse #22 axum gateway as the HOST (not a proxy) = the consolidation.
