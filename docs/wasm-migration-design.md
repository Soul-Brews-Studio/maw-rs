# P0 WASM Migration Architecture Design

Status: design-only keystone for Issue #26 / Epic #25. No build code is included in this PR.

Scope rule: all implementation work lands in this `maw-rs` fork only. The pinned `maw-js` checkout is a read-only reference for source parity and must not be modified or used to touch the running fleet.

## Design goals

1. Replace Bun plugin execution with WASM plugin execution while preserving the `@maw-js/sdk` contract exported from `maw-js/src/sdk/index.ts`.
2. Make host functions the only I/O boundary. WASM plugins can compute freely, but every filesystem, process, tmux, network, or ssh effect crosses an audited maw-rs host function.
3. Reuse already-native Rust ports wherever possible:
   - `cmdSend` / `cmdPeek` / local tmux delivery: `maw-cli` `hey`/`send`, `run`, `send-enter`, `maw-tmux` send/capture helpers.
   - remote `cmdSend` / `cmdWake` / `curlFetch`: `crates/maw-transport/src/core_impl/part05.rs` reqwest/rustls client and signed `/api/send` + `/api/wake` request shapes.
   - target/session/worktree routing: `maw-routing`, `maw-tmux`, `maw-worktree`, `maw-auto-wake`, `maw-plugin-manifest`.
4. Preserve byte-for-byte plugin output parity with existing Bun plugins before cutover.

## Runtime shape

WASM plugins use an SDK shim that has the same source-level symbols as `@maw-js/sdk`. The shim serializes SDK calls to host functions. The maw-rs host registers functions in namespaces under `maw.*`, validates the plugin manifest capabilities, runs native Rust code or shell commands, and returns deterministic JSON envelopes.

Common host ABI:

```ts
type HostOk<T> = { ok: true; value: T; warnings?: string[] };
type HostErr = {
  ok: false;
  error: string;
  code:
    | "capability_denied"
    | "invalid_args"
    | "not_found"
    | "timeout"
    | "io_error"
    | "process_failed"
    | "network_error"
    | "unsupported";
  detail?: unknown;
};
type HostResult<T> = HostOk<T> | HostErr;
```

All host-fn arguments and return values are JSON-serializable. Byte payloads are UTF-8 strings unless explicitly marked as base64. Timeouts are milliseconds. Paths are normalized by the host before capability checks.

## 1. Host-function layer

### Capability vocabulary

The existing maw-rs manifest parser already recognizes capability namespaces: `net`, `fs`, `peer`, `sdk`, `proc`, `ffi`, `tmux`, `shell`, `attach`. P0 should keep those namespaces and add structured entries under `capabilities` rather than inventing a second policy system.

Examples:

```json
{
  "entry": { "kind": "wasm", "path": "dist/plugin.wasm", "export": "handle" },
  "sdk": "^26.6.0",
  "capabilities": [
    "sdk:config:read",
    "tmux:send:local",
    "tmux:capture:local",
    "net:https:api.github.com",
    "fs:read:maw-state",
    "fs:write:artifact-dir",
    "proc:exec:git",
    "shell:ssh"
  ]
}
```

### Host-fn table

| Host function | Args | Return shape | Native reuse / implementation | Capability boundary |
|---|---|---|---|---|
| `maw.exec.run` | `{cmd:string,args?:string[],cwd?:string,env?:Record<string,string>,stdin?:string,timeoutMs?:number,allowNonZero?:boolean}` | `{status:number,stdout:string,stderr:string,durationMs:number}` | New wrapper around `std::process::Command`; use for non-interactive plugin subprocesses only. | Requires `proc:exec:<cmd>` or `shell:exec:<cmd>`; cwd must be inside declared fs read/write roots; env allowlist only. Generic argv values may legitimately start with `-` (for example `git --version`), so this primitive relies on the executable capability gate plus hard-denied privileged/interactive programs/options instead of blanket leading-dash rejection. |
| `maw.exec.spawn` | `{cmd:string,args?:string[],cwd?:string,env?:Record<string,string>,stdin?:string,timeoutMs?:number,capture?:"none"|"stdout"|"stderr"|"both"}` | `{pid?:number,status?:number,stdout?:string,stderr?:string,detached:boolean}` | New non-interactive spawn wrapper; no PTY. Detached children must be supervised or short-lived. | Requires `proc:spawn:<cmd>`; deny interactive PTY, raw TTY, sudo, and undeclared cwd/env. |
| `maw.fs.read` | `{path:string,encoding?:"utf8"|"base64",maxBytes?:number}` | `{path:string,bytes:number,content:string}` | New host wrapper over `std::fs::read`. | Requires `fs:read:<root>` matching normalized path; deny symlink escapes and device files. |
| `maw.fs.write` | `{path:string,content:string,encoding?:"utf8"|"base64",mode?:"create"|"overwrite"|"append",mkdirp?:boolean}` | `{path:string,bytes:number}` | New host wrapper over `std::fs::{write,OpenOptions}`. | Requires `fs:write:<root>`; default deny overwrite unless capability or mode policy allows; no writes outside plugin artifacts/state/worktree roots. |
| `maw.fs.list` | `{path:string,recursive?:boolean,maxEntries?:number,includeDirs?:boolean}` | `{entries:{path:string,kind:"file"|"dir"|"symlink",bytes?:number,mtimeMs?:number}[]}` | New read-only directory walker. | Requires `fs:read:<root>`; recursion and max entries capped by host. |
| `maw.fs.stat` | `{path:string}` | `{exists:boolean,kind?:"file"|"dir"|"symlink",bytes?:number,mtimeMs?:number}` | New wrapper over metadata/symlink metadata. | Requires `fs:read:<root>`; never follows symlink outside root. |
| `maw.http.request` | `{method:"GET"|"POST"|"PUT"|"PATCH"|"DELETE",url:string,headers?:Record<string,string>,body?:string,timeoutMs?:number,followRedirects?:boolean}` | `{status:number,headers:Record<string,string>,body:string,url:string}` | Reuse reqwest/rustls client style from `ReqwestHttpTransportIo`; generalize beyond signed maw endpoints without adding another TLS stack. | Requires `net:http:<host>` or `net:https:<host>`; deny link-local/loopback/private addresses unless explicitly declared; redact auth headers in logs. |
| `maw.http.peer_send` | `{peerUrl:string,target:string,text:string,inbox?:boolean,from:string,peerKeyRef?:string,timestamp?:number}` | `{ok:boolean,status:number,state?:string,target?:string,lastLine?:string,error?:string}` | Reuse `PeerSendRequest`, `PeerSendResponse`, `ReqwestHttpTransportIo::send_peer` from E3. | Requires `peer:send` and `net:https:<peer-host>`; peer key is resolved by host secret store, never passed from guest as raw secret. |
| `maw.http.peer_wake` | `{peerUrl:string,target:string,task?:string,from:string,peerKeyRef?:string,timestamp?:number}` | `{ok:boolean,status:number,target?:string,error?:string}` | Reuse `PeerWakeRequest`, `PeerWakeResponse`, `ReqwestHttpTransportIo::wake_peer`. | Requires `peer:wake` and `net:https:<peer-host>`; key material remains host-only. |
| `maw.tmux.list_sessions` | `{includeWindows?:boolean,includePanes?:boolean}` | `{sessions:{name:string,created?:number,windows?:{index:number,name:string,active:boolean,panes?:TmuxPane[]}[]}[]}` | Reuse `TmuxClient::list_all`, `list_windows`, `list_panes`, parsers in `maw-tmux`. | Requires `tmux:read`; read-only, no pane mutation. |
| `maw.tmux.resolve_target` | `{query:string,currentSession?:string,mode?:"route"|"pane"|"worktree"}` | `{target:string,source:string,ambiguous?:string[]}` | Reuse `maw-routing::resolve_route_target`, `maw-tmux::resolve_pane_target_from_list_panes_output`, `maw-worktree` helpers. | Requires `tmux:read`; no mutation. |
| `maw.tmux.capture` | `{target:string,lines?:number,stripAnsi?:boolean}` | `{target:string,content:string,lines:number}` | Reuse `TmuxClient::capture` (`capture-pane`). | Requires `tmux:capture` or `tmux:read`; target must resolve through allowed local tmux target policy. |
| `maw.tmux.send_keys` | `{target:string,keys:string[],literal?:boolean,enter?:boolean,allowDestructive?:boolean,force?:boolean}` | `{target:string,sent:boolean}` | Reuse `TmuxClient::send_keys`, `send_keys_literal`, `send_enter`, and `send_command_to_pane` safety gates. | Requires `tmux:send`; if `allowDestructive` or `force` is set, require narrower `tmux:send:force`; deny AI-pane collision by default. |
| `maw.tmux.run` | `{target:string,text:string}` | `{target:string,stdout:string}` | Reuse native `maw-rs run <target> <cmd>` path: literal send then Enter. | Requires `tmux:send`; same target and destructive-send policy as `maw.tmux.send_keys`. |
| `maw.tmux.command` | `{command:string,args:string[]}` | `{command:string,args:string[],stdout:string}` | Run the fixed, shape-validated tmux command forms used by extracted window-management plugins. | Read forms require `tmux:read`; mutations require exact `tmux:raw:<command>` capabilities. Arbitrary commands and argv shapes are denied. |
| `maw.tmux.send_enter` | `{target:string,count?:number}` | `{target:string,count:number}` | Reuse native `maw-rs send-enter` path and `TmuxClient::send_enter`. | Requires `tmux:send`; count capped. |
| `maw.tmux.tags_read` | `{target:string}` | `{title:string,meta:Record<string,string>}` | Reuse `TmuxClient::read_pane_tags`. | Requires `tmux:read`. |
| `maw.tmux.tags_write` | `{target:string,title?:string,meta?:Record<string,string>}` | `{target:string}` | Reuse `TmuxClient::tag_pane`. | Requires `tmux:write-tags`; restrict keys to `@maw-*` / approved namespace. |
| `maw.ssh.exec` | `{host:string,cmd:string,args?:string[],stdin?:string,timeoutMs?:number}` | `{transport:"ssh",host:string,status:number,stdout:string,stderr:string}` | Shell out to `ssh` for non-interactive commands; matches maw-js `hostExec` remote transport behavior. | Requires `shell:ssh:<host>` plus `proc:exec:ssh`; host must be an unpadded non-empty ASCII host/alias token that does not start with `-`, and the native command uses `ssh -T -- <host>` so guest host values cannot become ssh options. Deny interactive options, local forwards, agent forwarding unless declared. |
| `maw.ssh.tmux_capture` | `{host:string,target:string,lines?:number}` | `{host:string,target:string,content:string}` | Shell out `ssh <host> tmux capture-pane ...`; no persistent session. | Requires `shell:ssh:<host>` and `tmux:capture:remote`. |
| `maw.ssh.tmux_send_keys` | `{host:string,target:string,keys:string[],literal?:boolean,enter?:boolean}` | `{host:string,target:string,sent:boolean}` | Shell out `ssh <host> tmux send-keys ...`; no interactive PTY. | Requires `shell:ssh:<host>` and `tmux:send:remote`; same destructive-send gates. |
| `maw.config.get` | `{keys?:string[]}` | `{config:unknown}` | Reuse maw-rs config/XDG loaders as they land; initial implementation may project maw-js-compatible config JSON. | Requires `sdk:config:read`; secrets omitted unless separate secret capability exists. |
| `maw.config.set` | `{patch:unknown}` | `{written:boolean}` | Reuse config writer once ported; append/audit before write. | Requires `sdk:config:write`; deny secret writes from plugin unless explicitly allowed. |
| `maw.state.get` | `{namespace:string,key?:string}` | `{value:unknown}` | Host-managed plugin state in XDG/maw state dirs. | Requires `sdk:state:read:<namespace>`; namespace constrained to plugin or declared shared namespace. |
| `maw.state.set` | `{namespace:string,key:string,value:unknown,mode?:"set"|"append"}` | `{written:boolean}` | Host-managed plugin state writer. | Requires `sdk:state:write:<namespace>`; size and schema caps. |
| `maw.fleet.query` | `{op:"sessions"|"oracle_registry"|"fleet_entries"|"worktrees"|"channels"|"signals"|"snapshots"|"audit",args?:unknown}` | Operation-specific JSON | Reuse pure/read-heavy crates where present (`maw-tmux`, `maw-worktree`, `maw-hub`, future ports); otherwise host-owned file reads behind one audited query surface. | Requires `sdk:fleet:read` plus any fs/tmux sub-cap implied by the operation. |
| `maw.fleet.mutate` | `{op:"cleanup_worktree"|"save_tab_order"|"restore_tab_order"|"take_snapshot"|"log_audit"|"write_signal"|"write_artifact"|"set_profile"|"wake"|"sleep",args:unknown}` | Operation-specific JSON | Reuse native ports where present (`maw-worktree`, `maw-auto-wake`, artifact/profile ports when implemented); high-risk operations still shell/native guarded. | Requires specific mutation cap such as `sdk:fleet:cleanup-worktree`, `fs:write:*`, `tmux:send`, or `peer:wake`; deny generic fleet mutation. |
| `maw.plugin.invoke` | `{plugin:string,source:"cli"|"api"|"peer",args:string[],stdin?:string}` | `{ok:boolean,output?:string,error?:string}` | Reuse `maw-plugin-manifest::invoke_plugin` dispatch once WASM runtime is extism-backed. | Requires `sdk:plugin:invoke:<plugin>`; prevent recursive unbounded invocation. |
| `maw.transport.send` | `{target:string,body:string,from?:string,inbox?:boolean}` | `{state:"delivered"|"queued",target:string,lastLine?:string}` | Reuse native `hey`/`send` route: local `TmuxClient` or `ReqwestHttpTransportIo::send_peer`. | Requires `peer:send` or `tmux:send` depending resolved target; route resolution is host-owned. |
| `maw.transport.wake` | `{target:string,task?:string,from?:string}` | `{target:string,woken:boolean}` | Reuse native `wake` for peer wake and future local wake port; current local fallback remains outside WASM until native-complete. | Requires `peer:wake` / `tmux:spawn` / `sdk:fleet:wake` depending route. |

Rejected P0 host-fns:

| Request | P0 decision | Reason |
|---|---|---|
| Discord-gateway 8 | Out of WASM scope → Batch 4 native rewrite | Long-lived websocket session, heartbeats, reconnect state, and event fan-out are daemon/service concerns, not one-shot plugin calls. |
| Interactive ssh-tmux / PTY | Out of WASM scope → Batch 4 native rewrite | Requires live PTY ownership and user interaction; P0 host-fns are bounded request/response calls only. |
| FFI / raw `dlopen` | Out of WASM scope → Batch 4 native rewrite | Host FFI would be a sandbox escape and cannot be capability-contained safely in the plugin guest. |

## 2. WASM SDK shim

Source of truth: `maw-js/src/sdk/index.ts` exports the stable plugin contract. Types and interfaces are compile-time-only in TypeScript; the WASM shim mirrors them as AssemblyScript types but only runtime symbols call host functions.

Mapping rule:

- Pure constants/types/helpers stay in the guest shim when deterministic and I/O-free.
- Every I/O-capable symbol becomes a wrapper around the host-fn table above.
- Existing native maw-rs code is preferred over shelling out to maw-js. Shelling out to `maw-js` is not allowed.

### SDK → host-fn mapping table

| SDK symbol(s) | Runtime kind | Host function(s) | Native reuse / notes |
|---|---|---|---|
| `loadConfig`, `cfg`, `D`, `cfgTimeout`, `cfgLimit`, `cfgInterval`, `getEnvVars`, `getGhqRoot`, `isMawXdgEnabled`, `legacyMawPath`, `mawCacheDir`, `mawConfigDir`, `mawDataDir`, `mawDataPath`, `mawMessageLogPath`, `mawStateDir`, `mawStatePath` | Config/path reads | `maw.config.get`, `maw.state.get` | Reuse maw-rs XDG/config crates as they become authoritative. |
| `saveConfig`, `resetConfig` | Config write | `maw.config.set` | Host enforces config write cap; preserve append/audit before mutation. |
| `buildCommand`, `buildCommandInDir` | Command construction / optional exec | Pure shim for string building; if executing, `maw.exec.run` | Do not expose arbitrary shell without `proc` cap. |
| `DEFAULT_ENGINES`, `defaultEngineNameForConfig`, `resolveEngine`, `EngineDef`, `EngineRegistry`, `MawConfig`, `TScope`, `TProfile` | Pure data/types | Pure shim / no host call | Keep byte-compatible defaults from maw-js until native config owns them. |
| `listPending`, `loadPending`, `loadPendingById`, `pendingDir`, `pendingPath`, `savePending`, `updatePending`, `deletePending`, `isExpired`, `TTL_MS`, `PendingMessage` | Queue store | `maw.state.get`, `maw.state.set`, optionally `maw.fs.read/write/list` | Host owns queue location and TTL; no raw path escape. |
| `listTrust`, `recordTrust`, `removeTrust`, `approveConsent`, `rejectConsent`, `ConsentAction` | Consent store | `maw.state.get`, `maw.state.set` | Security-sensitive: separate `sdk:consent:*` caps; no auto-pair approval from plugins. |
| `tmux`, `Tmux`, `tmuxCmd`, `resolveSocket`, `TmuxWindow`, `TmuxSession` | Tmux low-level | `maw.tmux.list_sessions`, `maw.exec.run` only for approved tmux subcommands | Prefer typed tmux host-fns; raw `tmuxCmd` needs `tmux:raw:<cmd>` and should be rare. |
| `withPaneLock`, `splitWindowLocked`, `SplitWindowLockedOpts` | Tmux mutation with lock | `maw.tmux.resolve_target`, future `maw.tmux.split` / `maw.state.set` lock | Split itself is not in the minimal P0 list; if needed add typed host-fn, not raw shell. |
| `tagPane`, `readPaneTags`, `TagPaneOpts`, `PaneTags` | Pane metadata | `maw.tmux.tags_write`, `maw.tmux.tags_read` | Reuse `TmuxClient::tag_pane` / `read_pane_tags`. |
| `listSessions`, `getPaneInfos`, `getPaneCommands`, `getPaneCommand`, `SshSession`, `HostExecTransport` | Local/remote tmux reads | `maw.tmux.list_sessions`, `maw.ssh.exec` | Local reads reuse `maw-tmux`; remote reads shell out through bounded ssh. |
| `capture` | Pane capture | `maw.tmux.capture`, `maw.ssh.tmux_capture` | Reuse `TmuxClient::capture`; remote is shell-out. |
| `sendKeys` | Tmux send | `maw.tmux.send_keys`, `maw.ssh.tmux_send_keys` | Reuse `TmuxClient::send_keys`, `send_keys_literal`, `send_enter`, safety gates. |
| `hostExec`, `HostExecError` | Exec / ssh exec | `maw.exec.run`, `maw.ssh.exec` | New host wrapper; no PTY. |
| `attachRemoteSession`, `SshAttachError`, `AttachRemoteSessionOptions` | Interactive attach | Out of WASM; return `unsupported` | Batch 4 native rewrite because interactive ssh/tmux PTY is not bounded request/response. |
| `curlFetch` | HTTP | `maw.http.request`, `maw.http.peer_send`, `maw.http.peer_wake` | Reuse reqwest/rustls E3 transport; signed peer calls use dedicated host-fns. |
| `getPeers`, `getFederationStatus`, `findPeerForTarget` | Peer/federation reads | `maw.fleet.query`, `maw.http.request` | Reuse native peer/routing ports and signed HTTP as available. |
| `resolveTarget`, `ResolveResult`, `resolveSessionTarget`, `resolveWorktreeTarget`, `resolveFleetWindowSessionTarget`, `normalizeTarget`, `isInfrastructureChannelSessionName` | Routing/matching | `maw.tmux.resolve_target`, `maw.fleet.query`; pure shim for normalization | Reuse `maw-routing`, `maw-matcher`, `maw-worktree`. |
| `resolveOracle`, `pickOracle`, `OracleRef`, `ResolveOracleOptions`, `OracleResolveResult`, `PickOracleOptions` | Oracle selection | `maw.fleet.query` | Host reads registries/manifests; guest can filter returned data. |
| `findWindow`, `Session`, `Window` | Window matching | Pure shim for provided sessions, else `maw.tmux.resolve_target` | Algorithm can stay guest-side; live session source is host. |
| `agentProcessNames`, `engineIdlePromptPatterns`, `isAgentCommand`, `isAgentCommandForConfig`, `matchesAgentProcessName`, `matchesEngineIdlePrompt` | Pure detection | Pure shim | Use maw-js-compatible lists/patterns; no host call unless config read is needed. |
| `checkBusyGuard`, `extractOracleName` | Busy guard | `maw.tmux.capture`, `maw.tmux.list_sessions`, pure parsing | Reuse native capture; preserve AI-pane safety before sends. |
| `loadOracleChannels`, `saveOracleChannels`, `listAllOracleChannels`, `loadRepoChannels`, `saveRepoChannels`, `getChannelEnv`, `ChannelPlugin`, `OracleChannelConfig` | Channel config | `maw.state.get`, `maw.state.set`, `maw.config.get` | File locations host-owned; write cap required. |
| `scanSignals`, `ScannedSignal`, `writeSignal` | Signal files | `maw.fleet.query`, `maw.fleet.mutate` | Host constrains signal dirs and write names. |
| `resolveOraclePane` | Communication target helper | `maw.tmux.resolve_target`, `maw.fleet.query` | Reuse native route resolution and pane discovery. |
| `runHook`, `runSleepLifecycleHooks`, `SleepLifecycleContextInput`, `LifecycleRunSummary` | Plugin lifecycle | `maw.plugin.invoke`, `maw.fleet.mutate` | Host controls recursion, timeout, and hook policy. |
| `getTriggers`, `getTriggerHistory`, `fire` | Trigger runtime | `maw.state.get`, `maw.fleet.mutate` | Trigger firing is host-mediated and audited. |
| `FLEET_DIR`, `CONFIG_DIR`, `MAW_ROOT`, `CONFIG_FILE` | Path constants | Pure shim or `maw.config.get` | Values come from host config/XDG. |
| `scanWorktrees`, `cleanupWorktree`, `WorktreeInfo` | Worktree scan/mutate | `maw.fleet.query`, `maw.fleet.mutate` | Reuse `maw-worktree`; cleanup requires explicit mutation cap. |
| `saveTabOrder`, `restoreTabOrder` | Tmux/fleet mutation | `maw.fleet.mutate`, `maw.tmux.list_sessions` | Host mediates tmux reads and state writes. |
| `takeSnapshot`, `listSnapshots`, `loadSnapshot`, `latestSnapshot` | Snapshot store | `maw.fleet.query`, `maw.fleet.mutate` | Snapshot files stay under host-approved state dir. |
| `readAudit`, `logAudit` | Audit log | `maw.fleet.query`, `maw.fleet.mutate` | Append-only; plugins cannot rewrite audit history. |
| `scanLocal`, `scanRemote`, `scanFull`, `scanAndCache`, `readCache`, `isCacheStale`, `OracleEntry`, `RegistryCache` | Fleet registry | `maw.fleet.query`, `maw.http.request`, `maw.state.set` | Network scan requires `net` caps; cache writes are host-controlled. |
| `fleetLoadDirsForRead`, `fleetLoadDirForWrite`, `loadFleetCore`, `countDisabledFleetFilesCore`, `loadDisabledFleetEntriesCore`, `loadFleetEntries`, `FleetWindow`, `FleetSession`, `FleetEntry`, `DisabledFleetEntry` | Fleet file loads | `maw.fleet.query`, `maw.config.get` | Host exposes data, not arbitrary fleet paths. |
| `cmdSleep`, `cmdWakeAll`, `cmdWake`, `fetchIssuePrompt`, `findWorktrees`, `detectSession`, `parseWakeTarget`, `ensureCloned`, `shouldAutoWake` | Wake/sleep and clone helpers | `maw.transport.wake`, `maw.fleet.mutate`, `maw.http.request`, `maw.exec.run` | `cmdWake≈wake` reuses native peer wake + future local wake. `shouldAutoWake` reuses `maw-auto-wake` semantics. Clone/gh issue fetch need `proc:exec:git` / `net:https:api.github.com`. |
| `cmdWakeAll`, `cmdSleep` | Fleet-wide wake/sleep | `maw.fleet.mutate` | High-risk: require explicit `sdk:fleet:wake-all` / `sdk:fleet:sleep` caps. |
| `cmdPulseAdd`, `cmdPulseLs` | Pulse state | `maw.state.get`, `maw.state.set` | Host stores pulse records. |
| `loadOracleRegistry`, `getOracleMembers`, `filterMembers`, `OracleMember`, `OracleTeamRegistry` | Oracle team registry | `maw.fleet.query`; pure filtering | Reads only unless registry update is separately added. |
| `loadManifest`, `findOracle`, `loadManifestCached`, `invalidateManifest`, `ORACLE_MANIFEST_DEFAULT_TTL_MS`, `OracleManifestEntry`, `OracleManifestSource` | Manifest cache | `maw.fleet.query`, `maw.state.set` | Reuse `maw-plugin-manifest` for plugin manifests; oracle manifest cache via state caps. |
| `createArtifact`, `updateArtifact`, `writeResult`, `addAttachment`, `listArtifacts`, `getArtifact`, `artifactDir`, `ArtifactMeta`, `ArtifactSummary` | Artifacts | `maw.fleet.mutate`, `maw.fleet.query`, `maw.fs.write/read` | Host pins artifact roots; plugin cannot write arbitrary files. |
| `getActiveProfile`, `loadAllProfiles`, `loadProfile`, `setActiveProfile` | Profiles | `maw.state.get`, `maw.state.set` | `setActiveProfile` requires explicit write cap. |
| `discoverPackages`, `importPluginSymbol`, `invokePlugin`, `parseManifest`, `loadManifestFromDir`, `registerCommand`, `matchCommand`, `listCommands` | Plugin registry/dispatch | `maw.plugin.invoke`, `maw.fleet.query`; pure manifest parsing possible | Reuse `maw-plugin-manifest`; `importPluginSymbol` maps to WASM export lookup, not dynamic JS import. |
| `C`, `parseFlags`, `sparkline`, `tlink`, `UserError`, `isUserError`, `assertValidOracleName`, `validateNickname` | Pure helpers | Pure shim | No host call unless a helper writes state. |
| `cmdWorkspaceCreate`, `cmdWorkspaceJoin`, `cmdWorkspaceShare`, `cmdWorkspaceUnshare`, `cmdWorkspaceLs`, `cmdWorkspaceAgents`, `cmdWorkspaceInvite`, `cmdWorkspaceLeave`, `cmdWorkspaceStatus`, `WorkspaceConfig` | Workspace state | `maw.state.get`, `maw.state.set`, `maw.fleet.query` | Host owns workspace state and write permissions. |
| `ghqFind`, `ghqList`, `ghqFindSync`, `ghqListSync` | Repo discovery | `maw.exec.run` or future native ghq scanner | Requires `proc:exec:ghq` or `fs:read:<ghq-root>`; sync variants are shim-level blocking wrappers over the same async host call. |
| `writeNickname`, `setCachedNickname` | Nickname writes/cache | `maw.state.set` | Requires `sdk:nickname:write`. |
| `cmdPeek` | Communication read | `maw.tmux.capture`, `maw.tmux.resolve_target`, `maw.fleet.query` | Reuse native capture/route paths; output must match Bun `cmdPeek`. |
| `cmdSend` | Communication send | `maw.transport.send`, `maw.tmux.send_keys`, `maw.http.peer_send` | Reuse native `hey`/`send`: local tmux send + E3 reqwest peer send. |
| `cmdSplit` | Split plugin | Future `maw.tmux.split` or native command wrapper | Reuse `maw-split` policy and `maw-tmux` split action; not a generic shell. |
| `buildAgentRows`, `AgentRow` | Agent table | `maw.fleet.query`; pure formatting | Host provides live sessions; guest formats. |
| `createTransportRouter`, `getTransportRouter`, `resetTransportRouter`, `TransportRouter`, `classifyError`, `Transport`, `TransportTarget`, `TransportMessage`, `TransportPresence`, `TransportResult`, `TransportFailureReason` | Transport abstraction | `maw.transport.send`, `maw.transport.wake`, `maw.http.request` | Guest router facade delegates all I/O to host; error classifier can be pure. |
| `cmdOracleAbout`, `cmdOracleList`, `cmdOracleScan`, `cmdOracleFleet`, `cmdOracleScanStale`, `cmdOraclePrune`, `cmdOracleRegister`, `OracleStatus` | Oracle management | `maw.fleet.query`, `maw.fleet.mutate` | Read commands use host query; prune/register require explicit mutation caps. |
| `PluginConfig`, `definePlugin`, `PluginManifest`, `LoadedPlugin`, `InvokeContext`, `InvokeResult` | Plugin contract/types | Pure shim | `definePlugin` remains validation/identity helper. |

## 3. WASM build pipeline

Recommendation: **AssemblyScript first, Rust→WASM supported later.**

| Option | Pros | Cons | Decision |
|---|---|---|---|
| AssemblyScript | Closest to maw-js plugin authors and existing TypeScript SDK shape; easier to port thin TS wrappers; existing maw-js already carries WASM SDK direction; string/JSON shim can preserve current plugin ergonomics. | Less ideal for systems-level plugins; AS runtime details must be pinned for deterministic output. | Primary P0 pipeline. |
| Rust→WASM | Strong typing, excellent tooling, natural fit for maw-rs contributors, easy no-std compute plugins. | Higher rewrite cost for 124 TS wrappers; SDK surface would need Rust bindings in addition to AS; slower Batch 1 migration. | Supported as an advanced authoring path after AS parity harness is stable. |

### Manifest change

Current maw-rs already has `wasm` and `entry` fields plus `LoadedPluginKind::Wasm`; P0 should make `entry.kind="wasm"` explicit while preserving backwards-compatible `wasm` parsing during migration.

Recommended manifest shape:

```json
{
  "name": "example",
  "version": "1.0.0",
  "sdk": "^26.6.0",
  "entry": {
    "kind": "wasm",
    "path": "dist/plugin.wasm",
    "export": "handle"
  },
  "capabilities": ["tmux:read", "tmux:send"]
}
```

Compatibility rule:

| Manifest form | Meaning |
|---|---|
| `{ "entry": "index.ts" }` | Legacy Bun/TS plugin until cutover; no new WASM host-fns. |
| `{ "wasm": "plugin.wasm" }` | Existing maw-rs MVP WASM form; load as `entry.kind="wasm"`, default export `handle`. |
| `{ "entry": { "kind":"wasm", "path":"dist/plugin.wasm", "export":"handle" } }` | Preferred P0 form. |

### Extism host registration model

The extism runtime should replace the current MVP no-import WASM reader for real plugins. Host-fns are registered before instantiation and are unavailable unless the plugin manifest grants their capabilities.

Conceptual Rust shape:

```rust
let manifest_caps = CapabilitySet::from_manifest(&plugin.manifest)?;
let host = MawWasmHost::new(plugin.clone(), manifest_caps, native_services);

let mut manifest = extism::Manifest::new([extism::Wasm::data(wasm_bytes)]);
manifest = manifest.with_allowed_hosts(host.allowed_http_hosts());

let mut plugin = extism::PluginBuilder::new(manifest)
    .with_wasi(false)
    .with_function("maw.http.request", [ValType::I64], [ValType::I64], host_fn_http_request)
    .with_function("maw.tmux.capture", [ValType::I64], [ValType::I64], host_fn_tmux_capture)
    .with_function("maw.transport.send", [ValType::I64], [ValType::I64], host_fn_transport_send)
    .build()?;

let output_json = plugin.call::<&str, String>(export_name, &ctx_json)?;
```

Each registered function receives one JSON argument pointer/string from the guest and returns one JSON result pointer/string. If extism's PDK string helpers are used, the AS SDK shim can implement every symbol as:

```ts
const response = Host.call("maw.tmux.capture", JSON.stringify(args));
return unwrap<...>(JSON.parse(response));
```

WASI stays disabled for P0 unless a future capability review explicitly allows preopened directories. That keeps maw host-fns as the only I/O surface.

## 4. Security / capability gate

### Option-injection rule

Any guest, user, or manifest-controlled string that becomes a native process argument must either be rejected when it is empty, padded, or starts with `-`, or be placed after a tool-supported `--` end-of-options separator. Shape-known fields also use a narrow ASCII allowlist: ssh hosts/aliases allow letters, digits, `_`, `.`, `:`, `-`; tmux targets additionally allow `%`. Generic exec argv is the exception because leading-dash operands are normal CLI usage; it remains bounded by per-executable capabilities and hard-denied privileged/interactive option patterns.

### Declare

Plugins declare capabilities in `plugin.json` using existing `capabilities` plus optional `capabilityNamespaces` only for custom plugin-local namespaces. Core I/O namespaces remain fixed.

Capability format:

```text
<namespace>:<verb>[:<scope>...]
```

Examples:

| Capability | Meaning |
|---|---|
| `fs:read:maw-state` | Read host-resolved maw state root. |
| `fs:write:artifact-dir` | Write only the plugin artifact directory. |
| `net:https:api.github.com` | HTTPS requests only to `api.github.com`. |
| `proc:exec:git` | Run `git` with bounded args/cwd. |
| `tmux:capture` | Capture local panes. |
| `tmux:send` | Send non-force keys to local panes. |
| `peer:send` | Send maw peer messages through host transport. |
| `shell:ssh:bigboy` | Non-interactive ssh to host alias `bigboy`. |

### Enforce

Every host-fn starts with the same sequence:

1. Decode JSON and validate schema.
2. Resolve derived resources in host space: canonical path, peer URL host, tmux target, executable basename, ssh host alias.
3. Check manifest-derived `CapabilitySet` against operation and resolved resource.
4. Apply hard safety denies independent of manifest: no `/root`, no secret env dump, no `sudo`, no raw PTY, no device files, no symlink escape, no private-network HTTP unless declared, no Discord gateway, no FFI.
5. Run native operation with timeout and byte limits.
6. Return `HostResult<T>` and write an audit event containing plugin name, host-fn, capability matched, sanitized resource, status, and duration.

Pseudo-code:

```rust
fn host_tmux_send(ctx: HostCtx, input: TmuxSendArgs) -> HostResult<TmuxSendReply> {
    let target = ctx.tmux.resolve_target(&input.target)?;
    ctx.caps.require("tmux", "send", &target)?;
    deny_ai_pane_collision_unless_force_cap(&ctx, &target, input.force)?;
    deny_destructive_unless_cap(&ctx, &input.keys, input.allow_destructive)?;
    ctx.tmux.send_keys(target, input.keys)
}
```

### No sandbox escape rules

- No WASI preopens in P0. WASM has no filesystem/network/process access unless host-fn provides it.
- No raw `maw.exec.run` for shell strings; command and args are separate.
- No inherited arbitrary environment. Host builds a minimal allowlisted env.
- No secret bytes cross into guest. Host accepts secret references (`peerKeyRef`) and signs internally.
- No host-fn for FFI, persistent sockets, or interactive PTY.
- No pairing/access approval host-fn. Pairing must remain human-at-terminal only.

## 5. Parity strategy

Acceptance rule: for migrated plugins, WASM-plugin output must byte-match the existing Bun-plugin output for the same `InvokeContext`, host fixture state, environment, and plugin inputs.

### Golden fixture harness

Use the same pattern as the existing portable fixture tests in maw-rs crates: stable JSON fixtures checked into the maw-rs repo, deterministic host fakes, and one expected output per scenario.

Fixture layout:

```text
crates/maw-plugin-manifest/tests/fixtures/wasm-parity/
  <plugin-name>/
    manifest.json
    context.cli.basic.json
    context.peer.basic.json
    host-state.json
    bun.stdout
    bun.stderr
    bun.result.json
    wasm.result.json
```

Harness stages:

| Stage | Action | Pass condition |
|---|---|---|
| Capture | Run existing Bun plugin in an isolated temp MAW_HOME against fake host state, never against live fleet. | Captured stdout/stderr/result JSON committed as golden. |
| Build | Compile AssemblyScript plugin wrapper to WASM with pinned compiler/runtime versions. | Deterministic `.wasm` hash or intentionally updated hash in fixture metadata. |
| Replay | Run WASM through maw-rs extism runtime with fake host-fns seeded from `host-state.json`. | Host calls match expected sequence and caps. |
| Compare | Compare normalized result envelope and raw output bytes. | `stdout`, `stderr`, `InvokeResult.output`, and error text byte-match unless fixture marks an approved intentional delta. |
| Native integration | For selected plugins, run against real maw-rs native services in dry-run/temp roots. | No live fleet mutation; fmt/clippy/test still green. |

Golden outputs are captured once and committed. Tests must not invoke the live `maw-js` checkout; they read `golden.<argscase>.json` from `crates/maw-plugin-manifest/tests/fixtures/wasm-parity/<plugin>/`. To refresh after intentionally bumping the maw-js reference, run this on a maintainer machine where the real maw-js clone exists:

```bash
MAW_JS_REF_DIR=/path/to/Soul-Brews-Studio/maw-js scripts/refresh-wasm-parity-goldens.sh
```

The refresh command runs the ignored Rust fixture-generation test in an isolated temporary `MAW_HOME`, writes each committed golden, and stamps `metadata.json` with `mawJsVersion` and `mawJsCommit` so every captured output is traceable to the source checkout. CI does not need `MAW_JS_REF_DIR`; setting it to a nonexistent path must not affect normal parity tests.

### Host fake contract

Fake host-fns must be deterministic and record calls:

```json
{
  "calls": [
    { "fn": "maw.tmux.capture", "args": { "target": "%1", "lines": 80 } },
    { "fn": "maw.transport.send", "args": { "target": "nova", "body": "hi" } }
  ],
  "responses": {
    "maw.tmux.capture:%1": { "ok": true, "value": { "content": "..." } }
  }
}
```

The parity test should fail on:

- output byte drift,
- missing or extra host calls,
- undeclared capability use,
- non-deterministic ordering,
- dependency on real machine paths outside fixture roots.

### Migration batch gates

| Batch | Gate |
|---|---|
| Batch 1 pure logic | No host calls except `maw.config.get` when fixture explicitly grants it; byte parity required. |
| Batch 2 fs/tmux-only | Host call transcript must contain only `fs`, `tmux`, `state`, and pure config calls. |
| Batch 3 net/exec/git | Network and process calls must use declared host/command scopes; no raw shell. |
| Batch 4 hard subset | Not migrated to WASM; native rewrite PRs with their own tests and review. |

## Implementation checklist for follow-up PRs

1. Extend `maw-plugin-manifest` to parse `entry` object while preserving `wasm` compatibility.
2. Replace MVP no-import WASM runtime with extism plugin runtime and JSON host-call helpers.
3. Implement `CapabilitySet` from manifest and enforce at every host-fn.
4. Add AS SDK shim package exposing every symbol in `maw-js/src/sdk/index.ts`.
5. Port host-fns in this order: config/state read, tmux read/capture, transport send/wake via existing Rust, fs, http request, exec, ssh shell-out.
6. Add golden parity harness before migrating any real plugin.

## Source evidence used

- Issue #26: design-only keystone and five required areas.
- Issue #25: Option B full Bun removal, hard subset, phase/batch strategy.
- `maw-js/src/sdk/index.ts`: stable SDK surface and exported symbol list.
- `crates/maw-transport/src/core_impl/part05.rs`: reqwest/rustls signed `/api/send` and `/api/wake` client.
- `crates/maw-cli/src/core_impl/part28.rs`: native `run` / `send-enter` tmux paths.
- `crates/maw-cli/src/core_impl/part29.rs`: native `hey` / `send` / peer wake paths.
- `crates/maw-tmux/src/core_impl/part02_2.rs`: send/capture/tag tmux primitives.
- `docs/wire-protocol.md`: E1/E3 wire behavior for local tmux and HTTP federation.
