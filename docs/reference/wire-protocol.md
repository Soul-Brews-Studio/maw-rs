# maw-js Wire Protocol (E1)

Pinned version: maw-js v26.6.13.

Reverse-engineered for Issue #7. This is a docs-only target for the Rust port; no production Rust behavior is changed here.

## Evidence and capture boundary

Ground truth is maw-js v26.6.13 source under `/home/agent/github.com/Soul-Brews-Studio/maw-js/src`. Safe capture was limited to localhost/temp state so no real fleet peer received writes.

Captured evidence:

- `maw serve` temp instance with `MAW_HOME=/tmp/maw-wire-e1.gcFYSk`, `bind=127.0.0.1`, `discovery.transport=off`, no peers, port `49661`. `GET /api/sessions?local=true` returned HTTP 200 JSON and serve logged `GET /api/sessions?local=true -> 200`. The process exited before additional loopback calls, so route response shapes below are source-cited unless specifically noted.
- A one-shot local recorder exercised maw-js `curlFetch()` for an outbound peer send. It captured actual `POST /api/send` headers and body: `content-type: application/json`, `x-maw-timestamp`, `x-maw-signature`, `x-maw-auth-version: v3`, `x-maw-from`, `x-maw-signature-v3`, and body `{"target":"remote-oracle","text":"E1 signed capture","inbox":true}`. The recorder returned `{ok:true,target:"remote-oracle",state:"queued",receipt:[...]}` and `curlFetch()` parsed that JSON.

Unsafe/not captured:

- Real cross-node `maw hey` and pairing writes were not exercised against live peers because `/api/send`, `/api/wake`, `/api/pair/:code`, and `/api/pair/auto` can write tmux panes, wake agents, or persist peer state. Those paths are documented from source only.
- Zenoh was not captured because it requires a configured `zenoh.locator` and zenohd remote-api; source confirms it is opt-in.
- MQTT was not captured because maw-js has only feed publish hooks, not `hey` transport delivery; source confirms the configured topic shape.

## Transport inventory: what maw-js actually uses

| Transport | Actual use | Evidence |
|---|---:|---|
| local tmux | yes, default local fast path for `hey`; no network envelope | `TmuxTransport` wraps `sendKeys`, connects by setting `_connected=true`, rejects non-local host, resolves target then `sendToTmux(tmuxTarget,message)` (`src/transports/tmux.ts:1-5`, `src/transports/tmux.ts:14-32`, `src/transports/tmux.ts:38-57`, `src/transports/tmux.ts:78-81`). The CLI `cmdSend` local branch resolves a pane, persists receiver inbox first, then calls `sendKeys(target,outboundMessage)` (`src/commands/shared/comm-send.ts:857-886`). |
| HTTP federation | yes, primary cross-node `hey`/peer fallback | CLI remote branch posts JSON to `${peerUrl}/api/send` with `from: senderIdentity.wireFrom` signing (`src/commands/shared/comm-send.ts:947-950`). Discovery fallback does the same (`src/commands/shared/comm-send.ts:1018-1021`). Server remote branch still forwards `/api/send` with `from:"auto"` (`src/api/sessions.ts:531-537`). `HttpTransport` is registered when `config.peers.length > 0` (`src/transports/index.ts:101-108`). |
| Scout UDP discovery + HTTP auto-pair | yes by default unless disabled | Router registers `ScoutTransport` when discovery resolves to `scout` or `both` (`src/transports/index.ts:67-77`). Protocol is JSON over UDP multicast `224.0.0.224:31746` with Scout/Hello/Announce messages (`src/transports/scout-protocol.ts:1-8`, `src/transports/scout-protocol.ts:12-47`, `src/transports/scout-protocol.ts:57-88`). Auto-pair then POSTs HTTP `/api/pair/auto` (`src/transports/scout-pair.ts:1-5`, `src/transports/scout-pair.ts:42-83`). |
| Zenoh full transport | implemented, opt-in only | Registered only when transport `zenoh` is enabled and `config.zenoh?.locator` is set (`src/transports/index.ts:90-97`). It opens zenoh-ts over a locator such as `ws://host:10000` (`src/transports/zenoh.ts:1-10`, `src/transports/zenoh.ts:57-63`). Topics are `maw/<node>/hey/<oracle>`, `maw/<node>/presence`, and `maw/<node>/feed` (`src/transports/zenoh.ts:149-180`). |
| Zenoh scout | implemented, opt-in discovery/presence provider | Discovery resolver can choose `zenoh`; router registers plugin `zenoh-scout` only when discovery is `zenoh` or `both` (`src/transports/index.ts:23-38`, `src/transports/index.ts:83-86`). v26.6.13 wires it through `PluginTransportAdapter` and plugin symbol `createZenohScoutTransport` instead of importing the shim directly (`src/transports/index.ts:20-21`, `src/transports/index.ts:80-86`, `src/transports/index.ts:134-181`). |
| MQTT | **not used for `hey` delivery**; feed broadcast only when configured | The interface comment still mentions MQTT for remote targets (`src/core/transport/transport.ts:4-10`), but the concrete router registers tmux, scout, zenoh-scout, zenoh, http, nanoclaw; it does not register an MQTT transport (`src/transports/index.ts:57-112`). MQTT code publishes feed events only to `maw/v1/oracle/<oracle>/feed` and `maw/v1/node/<node>/feed` (`src/plugins/builtin/mqtt-publish.ts:2-3`, `src/plugins/builtin/mqtt-publish.ts:22-23`) through a broker at `config.mqttPublish.broker` (`src/core/transport/mqtt-publish.ts:1-8`, `src/core/transport/mqtt-publish.ts:14-31`). |
| NanoClaw | external channels, not maw wire prerequisite | Router registers `NanoclawTransport` as optional transport (`src/transports/index.ts:111-112`). |

Conclusion for E6: do **not** implement MQTT as a `hey` transport target unless a new maw-js behavior appears; current maw-js uses MQTT only as optional feed publication. Conclusion for Zenoh: keep it as real but opt-in; do not cut it without an explicit port-vs-cut decision.

### Version delta 26.5.21 → 26.6.13

v26.5.21 `transports/index.ts` imported and registered `HubTransport` (workspace WebSocket, when workspace config exists), imported and always registered `LoRaTransport` (stub), and imported/exported `MdnsTransport` without registering it. v26.6.13 removed all three from `transports/index.ts` and moved zenoh-scout to a `PluginTransportAdapter` pattern (`src/transports/index.ts:5-14`, `src/transports/index.ts:57-112`, `src/transports/index.ts:134-181`). The carried-transport union still lists `"hub"` as vestigial (`src/core/transport/transport.ts:53-60`). maw-rs should target the v26.6.13 registered set; Hub-as-transport may return via plugin-adapter, so flag it for E5/E7.

## Serve/gateway wire

`maw serve` starts a Bun HTTP+WS server. It computes `HTTP_URL=http://localhost:<port>` and `WS_URL=ws://localhost:<port>/ws` (`src/core/server.ts:119-123`), routes WebSocket upgrades before HTTP API routing, routes `/api/*` through engine plugin proxy and the Elysia API (`src/core/server.ts:226-247`), then binds using `config.bind` or the bind-host heuristic (`src/core/server.ts:262-296`). The heuristic returns `127.0.0.1` by default and `0.0.0.0` when peers/namedPeers, `MAW_HOST=0.0.0.0`, or peers store exists (`src/core/bind-host.ts:1-16`, `src/core/bind-host.ts:34-52`).

### HTTP routes relevant to the wire

`api` is mounted at prefix `/api`, applies CORS, `federationAuth`, and `fromSigningAuth`, then registers route modules (`src/api/index.ts:35-78`). Key routes:

| Path | Method | Request | Response | Auth |
|---|---|---|---|---|
| `/api/sessions` | GET | optional query `local=true` | array of sessions; local rows are `{name, windows, source:"local"}`; aggregate mode includes peer sessions | public/read. Source: `src/api/sessions.ts:261-278`. Captured `GET /api/sessions?local=true` returned HTTP 200 JSON. |
| `/api/capture` | GET | query `target` | `{content}` or `{content:"", error, target?, validWindows?, hint?}` | public/read. Source: `src/api/sessions.ts:299-339`. |
| `/api/feed` | GET | `limit` query | `{events,total,active_oracles}` | public/read. Source: `src/api/feed.ts:43-56`. |
| `/api/feed` | POST | feed event body | `{ok:true}` | protected for POST. Source: `src/api/feed.ts:59-78`, `src/lib/elysia-auth.ts:37-40`, `src/lib/elysia-auth.ts:46-56`. |
| `/api/send` | POST | `{target,text,attachments?,inbox?}` | success `{ok:true,target,text,source,lastLine?,state,receipt?,inbox?,warning?,reason?,wokeFor?}`; errors 404/500/502 with `{error,...}` | protected write. Source: `src/api/sessions.ts:356-359`, `src/api/sessions.ts:470-528`, `src/api/sessions.ts:531-577`, `src/api/sessions.ts:580-609`, `src/lib/elysia-auth.ts:22-35`. |
| `/api/probe` | POST | optional `{target}` | no target: `{ok:true,transport:"local",source,sessions}`; local target: `{ok:true,target,transport:"local",source}`; peer target: `{ok:true,target,transport:"ssh",source,node}` | protected write because it walks send path. Source: `src/api/sessions.ts:736-805`. |
| `/api/wake` | POST | `{target}` or `{oracle,task?}` | `{ok:true,target}` or error | protected write. Source: `src/api/sessions.ts:816-836`, `src/lib/elysia-auth.ts:22-35`. |
| `/api/pane-keys` | POST | `{target,text,enter?}` | `{ok:true,target,enter}` | protected write. Source: `src/api/sessions.ts:709-725`, `src/lib/elysia-auth.ts:22-35`. |
| `/api/transport/status` | GET | none | `{transports: [{name, connected}]}` | public/read. Source: `src/api/transport.ts:23-31`. |
| `/api/transport/send` | POST | `{oracle,host?,message,from}` | `{ok,via,reason?,retryable}` | protected write. Source: `src/api/transport.ts:32-49`, `src/core/transport/transport.ts:154-174`. |
| `/api/federation/status` | GET | none | `{localUrl,localReachable,localLatency?,peers,totalPeers,reachablePeers,clockHealth}` | public/read. Source: route `src/api/federation.ts:93-96`, shape builder `src/core/transport/peers.ts:190-241`. |
| `/api/identity` | GET | none | identity route; used by peer probes | public/read; v3-signing may be sent on outbound probes. Source: route `src/api/federation.ts:118-144`; peer fetch `src/core/transport/peers.ts:76-105`. |
| `/api/peers/discoveries`, `/api/peers/discovered` | GET | `all`, `limit` | `{ok,total,shown,filtered,peers:[...]}` | public/read. Source: `src/api/peers-discoveries.ts:59-80`. |

### WebSocket routes

Serve exposes two direct WS paths in v26.6.13: `/ws/pty` and `/ws`. The server upgrade path runs before HTTP routing (`src/core/server.ts:226-247`). The default `/ws` dispatches to `MawEngine.handleOpen/handleMessage/handleClose`; `/ws/pty` dispatches PTY messages and close handling (`src/core/server.ts:201-214`, `src/core/server.ts:234-240`). No separate `/ws/tmux` route is wired in this pinned source.

## `hey` deliver path

### CLI to local tmux

1. Sender identity resolves from `--from`, `MAW_SENDER`, or local config/tmux fallback; human display form is `<node>:<oracle>`, and explicit wire-from form is `<oracle>:<node>` (`src/commands/shared/comm-send.ts:80-178`).
2. `cmdSend` resolves the target via local sessions and `resolveTarget` (`src/commands/shared/comm-send.ts:521-1021`). `resolveTarget` checks exact tmux address, fleet/session aliases, local findWindow, explicit `node:agent`, manifest, agents map, and peer alias in that order (`src/core/routing.ts:62-200`).
3. Local/self-node branch formats a body prefix `[node:oracle]` unless command-like or already signed (`src/commands/shared/comm-send.ts:193-218`), resolves a specific pane, writes receiver inbox, calls `sendKeys`, verifies submit, logs feed, and reports `delivered` (`src/commands/shared/comm-send.ts:857-942`).

Wire: no HTTP/MQTT/Zenoh frame; this is local `tmux send-keys`.

### CLI to configured peer over HTTP

Endpoint: `POST <peerUrl>/api/send`.

Request body schema:

```json
{
  "target": "<remote target/oracle>",
  "text": "<message, possibly [node:oracle]-prefixed>",
  "inbox": true
}
```

`inbox` is present only when requested. `cmdSend` constructs exactly this body and passes `from: senderIdentity.wireFrom` to `curlFetch()` for the configured-peer branch (`src/commands/shared/comm-send.ts:947-950`). Discovery fallback uses the same endpoint/body with the original query and also passes `from: senderIdentity.wireFrom` (`src/commands/shared/comm-send.ts:1018-1021`). Explicit or `MAW_SENDER` identities parse human `<node>:<oracle>` into wire `<oracle>:<node>`; automatic local identity still uses `"auto"` internally and is resolved by `curlFetch()` (`src/commands/shared/comm-send.ts:118-178`, `src/core/transport/curl-fetch.ts:87-90`). Server-side forwarding from `/api/send` posts `{target,text,inbox?}` to another peer and uses `from:"auto"` (`src/api/sessions.ts:531-537`).

Version delta: v26.5.21 used `from:"auto"`; v26.6.13 changed to `senderIdentity.wireFrom` — wire `X-Maw-From` value is the `oracle:node` either way; the difference is where it is resolved.

Captured outbound peer send (local recorder): method `POST`, path `/api/send`, body `{"target":"remote-oracle","text":"E1 signed capture","inbox":true}`, headers included `content-type: application/json`, `x-maw-timestamp`, `x-maw-signature`, `x-maw-auth-version: v3`, `x-maw-from: sender-oracle:sender-node`, and `x-maw-signature-v3`.

Response schema consumed by CLI: if `res.ok && res.data?.ok`, state is `delivered` only when `res.data.state === "delivered"`; otherwise it is treated as `queued`, with `target` and `lastLine` surfaced (`src/commands/shared/comm-send.ts:952-991`). Failure reports `Remote fetch failed` and exits (`src/commands/shared/comm-send.ts:994-1010`). `/api/send` receiver returns delivered/queued local results or 502 for peer forwarding failure (`src/api/sessions.ts:470-528`, `src/api/sessions.ts:564-577`).

Retry/error semantics:

- `curlFetch()` defaults timeout to 10000 ms and caps response bodies to 10 MiB (`src/core/transport/curl-fetch.ts:35-50`, `src/core/transport/curl-fetch.ts:125-180`).
- Native fetch returns `{ok:false,status:0,data:null}` on network/parse/abort errors and logs a warning (`src/core/transport/curl-fetch.ts:170-180`).
- `sendKeysToPeerDetailed()` maps non-ok peer response to `{ok:false,state:"failed",status,error}` and logs status/body snippet (`src/core/transport/peers.ts:416-452`).
- Transport router failover classifies timeout/unreachable/rate-limit as retryable, auth/rejected/parse as non-retryable (`src/core/transport/transport.ts:33-43`, `src/core/transport/transport.ts:154-174`).

## HMAC and auth-over-wire

### Fleet HMAC (v1/v2)

Config key source: `config.federationToken`, minimum length enforced by validation (`src/config/validate-ext.ts:28-35`). `curlFetch()` loads config and signs if token exists (`src/core/transport/curl-fetch.ts:67-81`).

Headers:

- `X-Maw-Timestamp: <unix seconds>`
- `X-Maw-Signature: HMAC-SHA256(federationToken, payload)`
- `X-Maw-Auth-Version: v2` only when body hash is included by `signHeaders()` (`src/lib/federation-auth.ts:115-137`)

Payloads:

- v1 legacy: `METHOD:PATH:TIMESTAMP`
- v2 body-bound: `METHOD:PATH:TIMESTAMP:BODY_SHA256`

Source: design and code at `src/lib/federation-auth.ts:1-18`, `src/lib/federation-auth.ts:70-80`, `src/lib/federation-auth.ts:124-137`. Incoming verification uses ±300 seconds clock window and constant-time comparison (`src/lib/federation-auth.ts:39-46`, `src/lib/federation-auth.ts:89-101`).

Important compatibility detail: current `curlFetch()` intentionally calls `signHeaders(token, method, path)` **without body**, so fleet HMAC is v1/body-unsigned even for JSON POST; the v3 layer binds the body when `from` is provided (`src/core/transport/curl-fetch.ts:74-81`). Elysia HMAC verifier still expects the full `/api/...` path (`src/lib/elysia-auth.ts:312-319`).

Protected paths: `/send`, `/pane-keys`, `/probe`, `/wake`, `/sleep`, `/talk`, `/transport/send`, `/triggers/fire`, `/worktrees/cleanup`, engine register/unregister, plus POST `/feed`, POST `/plugins/*`, and GET `/plugin/download/*` (`src/lib/elysia-auth.ts:22-56`). Loopback bypass is allowed based on Bun `requestIP()` only, not spoofable headers (`src/lib/elysia-auth.ts:257-272`). If peers are configured but token is absent, protected non-loopback writes fail closed unless `allowPeersWithoutToken` is true (`src/lib/elysia-auth.ts:274-288`).

### Per-peer from-signing (v3)

Key source: local peer key from state path `peer-key`, generated mode 0600 on first read or overridden by `MAW_PEER_KEY` (`src/lib/peer-key.ts:2-12`, `src/lib/peer-key.ts:24-80`). It is published as `pubkey` in identity/pair flows and pinned TOFU in peers store (`src/lib/peers/store.ts:59-86`).

Headers:

- `X-Maw-From: <oracle>:<node>`
- `X-Maw-Signature-V3: HMAC-SHA256(peerKey, METHOD:PATH:TIMESTAMP:BODY_SHA256:FROM)`
- `X-Maw-Timestamp: <unix seconds>`
- `X-Maw-Auth-Version: v3`

Source: `signRequestV3()` and `signHeadersV3()` (`src/lib/federation-auth.ts:140-209`). `resolveFromAddress()` derives `<config.oracle ?? "mawjs">:<config.node>` and skips auto v3 if node is unset (`src/lib/federation-auth.ts:211-223`). `curlFetch()` adds v3 when `opts.from` is explicit or `"auto"` and a from-address is available (`src/core/transport/curl-fetch.ts:47-65`, `src/core/transport/curl-fetch.ts:82-105`).

Incoming v3 verification prefers `x-maw-signature-v3`, reads `x-maw-from` and `x-maw-timestamp`, checks cached peer key, rejects skew over 300 seconds, and verifies payload `METHOD:PATH:TIMESTAMP:BODY_SHA256:FROM` (`src/lib/federation-auth.ts:338-417`, `src/lib/federation-auth.ts:481-543`). The Elysia `fromSigningAuth` runs after fleet HMAC and refuses protected non-loopback writes on `refuse-*` decisions (`src/api/index.ts:35-40`, `src/lib/elysia-auth.ts:129-137`, `src/lib/elysia-auth.ts:167-230`).

### Pairing/auth-over-wire

Manual pair endpoints:

- `POST /api/pair/generate` body `{ttlMs?}` or `{expires?}` -> HTTP 201 `{ok:true,code,expiresAt,ttlMs,node,port}` (`src/api/pair.ts:79-87`).
- `GET /api/pair/:code/probe` -> `{ok:true,node}` or 400/404/410 (`src/api/pair.ts:89-94`).
- `POST /api/pair/:code` body `{node,url}` -> writes peer via `cmdAdd`, stores consumption result, returns `{ok:true,node,url:"http://localhost:<port>",federationToken:<random hex>}` (`src/api/pair.ts:96-108`).
- `GET /api/pair/:code/status` -> consumed or pending/expired/not_found (`src/api/pair.ts:110-118`).

Scout auto-pair:

- UDP discovery must have seen a recent Hello zid. `/api/pair/auto` body `{node,oracle?,url,zid,pubkey?,capabilities?}`; missing fields -> 400, no recent hello -> 403, pubkey mismatch -> 409 (`src/api/pair.ts:120-154`).
- Success persists peer, deletes zid, returns `{ok:true,node,oracle,url,pubkey,proof?,oneWay?}`; `proof` is HMAC over canonical identity when `federationToken` exists (`src/api/pair.ts:156-178`, `src/transports/scout-pair-proof.ts:10-35`).
- Initiator POSTs `/api/pair/auto` with `Content-Type: application/json`, retries after `[0,200,800]` ms with a 2000 ms timeout, verifies `proof`, then persists peer (`src/transports/scout-pair.ts:42-83`, `src/transports/scout-pair.ts:92-121`).

No nonce field exists in these auth schemes; replay defense is timestamp window plus body hash/v3 TOFU pin, and Scout auto-pair has a recent-Hello zid window (`src/api/pair.ts:44-55`, `src/api/pair.ts:128-133`).

## Federation-sync wire

`maw federation sync` is an active CLI helper, not a persistent protocol. It fetches every `namedPeer` identity with `GET <peer.url>/api/identity`, using `curlFetch(..., {from:"auto"})` for v3 signing (`src/commands/shared/federation-fetch.ts:1-23`). It computes a diff and applies it locally (`src/commands/shared/federation-sync.ts:2-30`, `src/commands/shared/federation-sync-cli.ts:72-159`). No remote mutation occurs in sync fetch; local apply may update config depending on flags.

Federation health/status probes:

- Reachability probes `GET <url>/api/sessions` with retries controlled by `peerProbeRetries` and `peerRetryBackoff` (`src/core/transport/peers.ts:76-103`).
- Identity fetch `GET <url>/api/identity` is best-effort and signed with `from:"auto"` (`src/core/transport/peers.ts:76-105`).
- Session aggregation fetches `GET <url>/api/sessions?local=true` and validates session shape (`src/core/transport/peers.ts:125-165`).
- Symmetric status asks `GET <peer.url>/api/federation/status` and checks whether local node appears in peer view (`src/core/transport/peers.ts:284-365`).

## Workspace-hub wire

Workspace API is mounted under `/api/workspace` (`src/api/index.ts:18`, `src/api/index.ts:65`). State persists under `mawDataPath("workspaces")` (`src/api/workspace-storage.ts:8-12`, `src/api/workspace-storage.ts:49-52`). Workspace token is a random 32-byte hex string (`src/api/workspace-helpers.ts:12-18`).

Auth for all workspace `/:id/*` routes uses headers `x-maw-signature` and `x-maw-timestamp`, signing `METHOD:PATH:TIMESTAMP` with the workspace token; verification uses ±300 seconds (`src/api/workspace-auth.ts:9-21`, `src/api/workspace-auth.ts:25-38`).

| Path | Method | Request | Response | Source |
|---|---|---|---|---|
| `/api/workspace/create` | POST | `{name,nodeId}` | `{id,token,joinCode,joinCodeExpiresAt}` | `src/api/workspace-routes.ts:29-58` |
| `/api/workspace/join` | POST | `{code,nodeId}` | `{workspaceId,token,name}` | `src/api/workspace-routes.ts:62-85` |
| `/api/workspace/:id/agents` | POST | signed; `{name,nodeId,status?,capabilities?}` | `{ok:true,agents:<count>}` | `src/api/workspace-routes.ts:89-127` |
| `/api/workspace/:id/agents` | GET | signed | `{agents,total}` | `src/api/workspace-routes.ts:130-138` |
| `/api/workspace/:id/status` | GET | signed | `{id,name,createdAt,nodes,nodeCount,healthyNodes,agents,agentCount,feedCount}` | `src/api/workspace-routes.ts:142-159` |
| `/api/workspace/:id/feed` | GET | signed; `limit?` | `{events,total}` | `src/api/workspace-routes.ts:163-174` |
| `/api/workspace/:id/message` | POST | signed; `{from,text,to?}` | `{ok:true}` | `src/api/workspace-routes.ts:179-199` |

## Zenoh wire

When enabled, `ZenohTransport` opens `@eclipse-zenoh/zenoh-ts` with `new Config(config.zenoh.locator)` and sets `connected=true` on session open (`src/transports/zenoh.ts:57-63`). It declares liveliness `maw/<node>/alive` (`src/transports/zenoh.ts:64-68`) and subscribes to:

- `maw/*/hey/<thisNode>`: payload JSON decoded to `TransportMessage`, then `transport="zenoh"` (`src/transports/zenoh.ts:70-85`).
- `maw/*/presence`: payload JSON `TransportPresence` (`src/transports/zenoh.ts:87-101`).
- `maw/*/feed`: payload JSON `FeedEvent` (`src/transports/zenoh.ts:103-114`).

Publish shapes:

- hey topic `maw/<thisNode>/hey/<target.oracle>` body `{from:<node>,to:<oracle>,body:<message>,timestamp:<epoch_ms>,transport:"zenoh"}` (`src/transports/zenoh.ts:149-160`).
- presence topic `maw/<thisNode>/presence` body is `TransportPresence` (`src/transports/zenoh.ts:167-172`).
- feed topic `maw/<thisNode>/feed` body is `FeedEvent` (`src/transports/zenoh.ts:175-180`).

## Scout discovery wire

Transport: UDP JSON multicast/unicast. Constants: multicast address `224.0.0.224`, port `31746`, version `1` (`src/transports/scout-protocol.ts:12-15`). Message shapes:

```ts
Scout  = { type:"maw-scout", version, zid, whatAmI, ts }
Hello  = { type:"maw-hello", version, zid, whatAmI, node, oracle, locators, capabilities, oracles, ts }
Announce = { type:"maw-announce", node, port, oracles, ts }
```

Source: `src/transports/scout-protocol.ts:18-47`, constructors at `src/transports/scout-protocol.ts:57-81`. Default Hello capabilities are `['pair','feed','send']` (`src/transports/scout-protocol.ts:61-81`). Pairing moves to HTTP `/api/pair/auto` as described above.

## MQTT feed wire

If `config.mqttPublish.broker` exists, maw-js creates an MQTT client and publishes JSON with QoS 0 (`src/core/transport/mqtt-publish.ts:14-31`). The built-in feed plugin publishes each feed event to:

- `maw/v1/oracle/<event.oracle>/feed`
- `maw/v1/node/<config.node ?? "unknown">/feed`

Source: `src/plugins/builtin/mqtt-publish.ts:2-23`. There is no `maw hey` MQTT topic in registered transport code.

## Omissions / not-yet-mapped

- HubTransport, LoRaTransport, and MdnsTransport are not registered transports in v26.6.13; see the version-delta note above. The `"hub"` transport discriminant remains in the carried-message union as vestigial (`src/core/transport/transport.ts:53-60`).
- v3 verification also accepts a legacy newline-payload fallback using header `x-maw-signed-at`: `FROM\nSIGNED_AT\nMETHOD\nPATH\nBODY_HASH`. A Rust verifier that implements only the colon-delimited v3 payload will reject older alpha peers; E8 must implement this fallback (`src/lib/federation-auth.ts:419-427`, `src/lib/federation-auth.ts:535-537`).

## Cross-version compatibility notes

HMAC v1/v2/v3 (`src/lib/federation-auth.ts`) and pairing (`src/api/pair.ts`) are byte-identical across v26.5.21 and v26.6.13 (diff = 0 lines), so auth/pairing compatibility spans 26.5.21–26.6.13. Scout constants and message types remain the same across both versions: multicast `224.0.0.224:31746`, version `1`, and Scout/Hello/Announce shapes (`src/transports/scout-protocol.ts:12-47`). Keep the v3 legacy newline-payload fallback (`x-maw-signed-at`) because it is the backward-compat path that lets maw-rs interoperate with both fleet versions, including Mac 26.5.21 and VPS 26.6.13 (`src/lib/federation-auth.ts:419-427`, `src/lib/federation-auth.ts:488-537`).

## Mapping to maw-rs seams

| maw-js construct | Wire responsibility | Rust seam |
|---|---|---|
| `Transport` interface `name/connect/disconnect/send/publishPresence/publishFeed/on*/canReach/connected` (`src/core/transport/transport.ts:70-103`) | Common transport abstraction | `maw-transport::Transport` trait: `name`, `connected`, `can_reach`, `send` (`crates/maw-transport/src/core_impl/part01.rs:322-335`). |
| `TransportRouter.send()` first connected reachable transport, retryable classification (`src/core/transport/transport.ts:154-174`) | Ordered failover semantics | `TransportRouter<T>::send` loops connected/can_reach transports and classifies errors (`crates/maw-transport/src/core_impl/part01.rs:337-380`). |
| `TmuxTransport` local fast path and `_connected` lifecycle (`src/transports/tmux.ts:14-32`, `src/transports/tmux.ts:38-57`) | Local no-network delivery | Rust `TmuxLocalTransport` with `connected: bool`, handlers, `connect`, `disconnect` (`crates/maw-transport/src/core_impl/part01.rs:407-439`). |
| `HttpTransport` peers fallback, `publishFeed` to `/api/feed`, `send()` via `sendKeysToPeer` (`src/transports/http.ts:32-51`, `src/transports/http.ts:57-76`, `src/transports/http.ts:82-103`) | HTTP federation side effects | `HttpTransportIo` seam: list local sessions, get all sessions, resolve target window, send peer keys, post peer feed, timeout (`crates/maw-transport/src/core_impl/part01.rs:257-302`). |
| `curlFetch()` signing and HTTP execution (`src/core/transport/curl-fetch.ts:47-123`) | Outbound signed HTTP | Implement behind Rust HTTP transport/auth boundary; preserve header names and v1/v3 compatibility. |
| `federationAuth` + `fromSigningAuth` (`src/lib/elysia-auth.ts:236-330`, `src/lib/elysia-auth.ts:167-230`) | Inbound protected write auth | Rust server/auth crate should verify same HMAC payloads before dispatching protected routes. |
| `ScoutTransport`/pair APIs (`src/transports/scout-protocol.ts:12-47`, `src/api/pair.ts:120-178`) | Discovery and pairing | Candidate separate Rust discovery/pairing module; not part of `HttpTransportIo` unless route handlers are integrated. |
| `ZenohTransport` (`src/transports/zenoh.ts:57-180`) | Optional pub/sub transport | Candidate optional Rust transport implementation behind `Transport`; gate on config `zenoh.locator`. |
| MQTT feed plugin (`src/plugins/builtin/mqtt-publish.ts:22-23`) | Optional feed broadcast, not `hey` | Do not map to `Transport::send` for E6 unless scope changes; map to feed/event publisher if ported. |

## Rust port compatibility notes

1. Preserve HTTP route paths exactly (`/api/send`, `/api/sessions?local=true`, `/api/identity`, `/api/federation/status`) because maw-js peers hard-code them in `curlFetch()` callers (`src/commands/shared/comm-send.ts:947-950`, `src/core/transport/peers.ts:76-165`, `src/commands/shared/federation-fetch.ts:11-23`).
2. Preserve loopback bypass only if the Rust security decision explicitly accepts maw-js compatibility; maw-js currently trusts TCP loopback but not `X-Forwarded-For` (`src/lib/elysia-auth.ts:257-272`).
3. Keep `X-Maw-Signature` v1/body-unsigned outbound compatibility for maw-js interop until a coordinated cutover; v3 from-signing is the body-bound layer currently emitted by `curlFetch()` (`src/core/transport/curl-fetch.ts:74-105`).
4. Implement response parsing tolerant of queued vs delivered states; `queued` is a success state, not failure (`src/commands/shared/comm-send.ts:952-970`, `src/core/transport/peers.ts:416-435`).
5. Zenoh and MQTT are not equivalent: Zenoh is an optional message/presence/feed transport; MQTT is only optional feed publication in current source.
6. E8 auth must accept the legacy newline v3 fallback (`FROM\nSIGNED_AT\nMETHOD\nPATH\nBODY_HASH` via `x-maw-signed-at`) as well as the current colon-delimited v3 payload, or older alpha maw-js peers will be rejected (`src/lib/federation-auth.ts:419-427`, `src/lib/federation-auth.ts:535-537`).
