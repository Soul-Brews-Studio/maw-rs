# Typed tmux lifecycle host ABI

Status: proposed

Issue: [#72](https://github.com/Soul-Brews-Studio/maw-rs/issues/72)

Behavioral references:

- `maw-js@746df172:src/vendor/mpr-plugins/team/`
- `laris-co/athena-oracle@e1a6bc9:.maw/plugins/squad/impl.ts`
- the current `maw-tmux` client and `maw-plugin-manifest` WASM host

## Decision: typed lifecycle, not wider raw tmux

Add a local-only `maw.tmux` lifecycle family whose JSON requests describe intent:
launch a session/window/pane, kill a pane, apply a layout, or inspect the current tmux
context. Do not add more accepted argv shapes to `maw.tmux.command` for these ports.

The existing raw command call remains a compatibility surface for already-shipped
plugins, but new Squad/Team manifests must use typed calls and must not request
`tmux:raw:*`. The invoke context stays byte-frozen; current tmux context, executable
resolution, cwd validation, and test substitution all belong behind host calls.

Version 1 controls only the local tmux server selected by the host. Remote tmux, grouped
sessions, client attach/switch, window linking, and session/window destruction are
deferred. An Extism call cannot safely take over the caller's terminal; plugins return a
created target and the top-level native CLI may attach after plugin invocation.

## Proven operation set

| reference behavior | typed operation |
| --- | --- |
| Squad `join` rejects an existing oracle session, then creates a detached session in that oracle's repo and starts Claude with team flags | `context`/existing read calls, then `launch` with `placement.kind=session` |
| Team `spawn --exec` splits the current pane horizontally at 50% and starts Claude from a validated cwd | `context`, then `launch` with `placement.kind=pane` |
| Team `bring` selects an existing workspace, wakes each oracle, and applies `main-vertical` or `tiled` | existing native `wake` through `maw.cli.run`, plus typed `context` and `layout` |
| Team `shutdown` polls live pane IDs and force-kills only remaining member panes | existing `maw.tmux.list_sessions`/read calls, then typed `kill` |
| Team `enter` submits pending input to one or more known pane IDs | existing `maw.tmux.send_enter`; no new lifecycle authority |

The ABI does not absorb the whole `wake` command. Worktree selection, engine config,
rehydration, context-limit recovery, and routing remain native. A Team plugin may declare
`cli:run:wake` and pass a structural argv array, preserving that behavior without
reimplementing it in guest code.

## Capabilities

Each mutation requires one exact capability; no umbrella `tmux:lifecycle` grant exists.

| capability | permits |
| --- | --- |
| `tmux:read` | `context` plus existing session/pane inspection |
| `tmux:send` | existing `send_enter` for Team `enter` |
| `tmux:lifecycle:create-session` | one `launch` with session placement |
| `tmux:lifecycle:create-window` | one `launch` with window placement |
| `tmux:lifecycle:create-pane` | one `launch` with pane placement |
| `tmux:lifecycle:kill-pane` | one `kill` with pane placement |
| `tmux:lifecycle:layout` | one allowlisted layout application |

A launch containing a process is co-gated by `proc:exec:<program>`. `repoEnv: true`
also requires `proc:exec:direnv`; exposing the real home requires the existing
`exec:home` grant. The cwd must canonicalize inside a named root for which the manifest
has `fs:read:<root>` or `fs:write:<root>`. Thus tmux creation cannot be used as an
unreviewed escape from process or filesystem policy.

Expected minimum additions are:

- Squad: `tmux:lifecycle:create-session`, `proc:exec:claude`,
  `proc:exec:direnv`, `exec:home`, and the named repo-root read grant needed by locate.
- Team spawn: `tmux:lifecycle:create-pane`, `proc:exec:claude`, `exec:home`.
- Team bring: `cli:run:wake`, `tmux:lifecycle:layout`, and `tmux:read`.
- Team shutdown/enter: `tmux:lifecycle:kill-pane` / existing `tmux:send`.

## Wire contract

All functions use the existing `HostResult` envelope:
`{ok:true,value,...}` or `{ok:false,error,code,detail?}`. Fields use camelCase and
unknown request fields are rejected so misspelled security options cannot fail open.

### `maw.tmux.context`

Request: `{}`.

Success:

```json
{"session":"33-maw-rs","window":"1","paneId":"%42","insideTmux":true}
```

Outside tmux succeeds with `insideTmux:false` and null target fields. This lets Team
produce its current `--exec requires an active tmux session` message without ambient
`TMUX` in the invoke context.

### `maw.tmux.launch`

Request:

```json
{
  "placement":{"kind":"session","session":"digger","window":"lead"},
  "cwd":"/opt/Code/github.com/example/digger-oracle",
  "process":{
    "program":"claude",
    "args":["--agent-id","digger@athena","--team-name","athena"],
    "env":{"CLAUDECODE":"1","CLAUDE_CODE_EXPERIMENTAL_AGENT_TEAMS":"1"},
    "repoEnv":true,
    "home":true
  }
}
```

`placement` is one of:

```text
{kind:"session", session, window?}
{kind:"window", session, window}
{kind:"pane", target, direction?:"horizontal"|"vertical",
 sizePercent?:1..99, title?}
```

`process` is optional; absence starts the host-configured user shell. `program` is a
basename resolved by the host, never a guest-provided executable path. Arguments remain
separate strings (bounded count and total bytes); Unicode, quotes, dollar signs, spaces,
and newlines are data, while NUL is rejected. Environment processing reuses the
`maw.exec.run` sanitizer: no guest `PATH`, `HOME`, `SHELL`, or secret/token-like keys.
The two boolean Claude team variables above are an explicit fixed allowlist.

Success:

```json
{"session":"digger","window":"lead","paneId":"%51","target":"digger:lead","created":true}
```

Creation is fail-if-present. There is no `reuse` flag in v1: Squad's one-oracle/one-
session guard and Team's roster updates must never report a reused or half-started target
as newly launched.

### `maw.tmux.kill`

Request: `{"kind":"pane","target":"%51"}`.

Success: `{"kind":"pane","target":"%51","killed":true}`.

Only exact pane IDs are accepted in v1. The host rechecks existence immediately before
mutation and confirms absence afterward. Killing sessions/windows or resolving fuzzy
names needs a later capability and contract; it cannot appear as an extra `kind` without
that review.

### `maw.tmux.layout`

Request: `{"target":"athena:lead","layout":"main-vertical"}`.

Success: `{"target":"athena:lead","layout":"main-vertical","applied":true}`.

Layouts are exactly `even-horizontal`, `even-vertical`, `main-horizontal`,
`main-vertical`, or `tiled`, matching `maw-tmux` validation. Plugins own fallback order
(`session:lead`, then `session:0`); the host performs one explicit mutation per call.

## Error and atomicity contract

Reuse the current error-code set so older 1.x PDK decoders remain compatible:

- `capability_denied`: missing lifecycle/process/cwd authority;
- `invalid_args`: malformed names, placement, env, size, or layout;
- `not_found`: cwd, parent session, or target does not exist;
- `process_failed`: the launch trampoline could not start the requested process;
- `timeout`: creation or postcondition verification exceeded its bound;
- `io_error`: tmux or private-plan I/O failed;
- `unsupported`: tmux is unavailable or the host is not local-capable.

Machine-actionable `detail.kind` distinguishes `already_exists`, `launch_failed`, and
`verification_failed` without adding an enum variant. Error text is for humans and must
not contain launch environment values.

The host validates every capability, path, executable, argument, and environment entry
before tmux mutation. Because tmux accepts a shell command string, the host must not
interpolate guest data into it. It atomically writes a mode-0600 launch plan under private
maw state, starts only `maw __tmux-launch <opaque-id>`, and that native helper executes
the stored program with `Command::args`. The plan contains no resolved secrets and is
deleted after pickup or bounded expiry.

After mutation the host verifies the returned pane ID. If its newly-created target
cannot pick up the plan, it removes only that target and returns `process_failed`; it
never kills a pre-existing target. `kill` reports success only after the pane disappears.
Every call audits plugin, exact capability, redacted `tmux://` target, status, and duration;
argv, env values, and prompt contents are not audited.

## Host-side versus plugin-side

The host owns capability enforcement, exact-name validation, cwd containment, executable
and repo-environment resolution, structural launch delivery, tmux invocation, rollback,
postcondition checks, and audit records. These rules live beside `maw-tmux`; the CLI owns
only the private launch trampoline entrypoint.

Plugins own command parsing, Squad color/name/team rules, oracle lookup, launch argv,
Team roster/task/inbox files, graceful-shutdown messages and polling, merge policy,
layout fallback, output bytes, and the rule that state is updated only after host success.
No host function reads or rewrites Squad/Team manifests.

## Implementation and parity rollout

1. Add pure request/response types and validation fixtures, then host dry-run/security
   tests for every capability cross-product. Keep `maw.tmux.command` unchanged.
2. Add the private structural launch plan/trampoline and injectable `maw-tmux` runner.
   Prove hostile argv (`'`, `"`, `$`, Unicode, newline) reaches a test process byte-for-byte,
   collision paths make no mutation, and failed pickup rolls back only the new target.
3. Publish additive PDK and AssemblyScript bindings as host ABI/npm SDK `1.1.0`; plugins
   using these calls declare `sdk: "^1.1.0"`.
4. Port Squad `join` first: it is the smallest session-create canary. Compare guards,
   stdout/stderr, roster/inbox writes, and a real empty-env Claude launch with the locked
   Athena reference before removing the `mawjs` pointer.
5. Port Team in increasing coupling order: `enter` (existing send), forced shutdown,
   `spawn --exec`, then `bring`. Keep native `wake` behind `maw.cli.run`; do not clone it.
6. For each slice, pin/rebuild `plugin.wasm`, run host security plus artifact invocation
   tests, and compare maw-js fixtures before changing CLI ownership. Rollback is the prior
   pinned manifest/artifact; no persistent tmux or team-state migration is introduced.
