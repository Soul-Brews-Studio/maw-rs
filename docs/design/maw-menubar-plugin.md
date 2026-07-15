# maw-menubar plugin

Status: proposed

Issue: [#480](https://github.com/Soul-Brews-Studio/maw-rs/issues/480)

## Goal and boundaries

`maw-menubar` is a macOS-only companion that keeps fleet health glanceable without
turning the menu bar into another fleet implementation. It displays local session,
agent, recent activity, and schedule outcome summaries, then delegates actions to the
HTTP API or real `maw` verbs.

It is a plugin under `fleet-plugins/maw-menubar/`, not a core command and not a WASM
artifact. The plugin uses the `bun-dev` native-subprocess tier established by
`fleet-plugins/p2p-share`: Bun owns install and lifecycle commands, while a separately
built native helper owns the long-running UI. This tier is intentionally unsandboxed;
manifest capabilities are an auditable declaration, not an enforcement claim.

Non-goals for v1 are a terminal replacement, remote administration, a configuration
window, App Store distribution, Windows/Linux support, and real-time push.

## Native toolkit decision

Choose a small **Swift/AppKit executable using `NSStatusItem`**.

AppKit directly provides the status item, its button, and its pull-down menu. A Swift
helper can run as an accessory application with no Dock icon, use `URLSession` for
polling, and use `Process` with argument arrays for actions. It adds no third-party UI
runtime or WebView and matches the macOS-only v1 boundary.

The alternatives add machinery without v1 value:

- Rust `tray-icon`/`muda` is viable, but on macOS it still requires creating the tray
  icon and running its event loop on the main thread. Its cross-platform abstraction and
  additional crate/release surface do not buy anything while macOS is the only target.
- Tauri supports tray menus, but introduces an app bundle, Tauri configuration, Rust and
  frontend dependencies, and a WebView-oriented lifecycle for an interface composed
  only of an `NSMenu`.

The helper source remains in the plugin package, for example
`native/MawMenubar.swift`. CI builds a universal, release-mode helper; normal install
must use the packaged binary and must not invoke `swiftc` on the operator's machine.
The implementation should pin the helper checksum in its release metadata and exercise
both arm64 and x86_64 slices before publishing.

References: [Apple `NSStatusItem`](https://developer.apple.com/documentation/appkit/nsstatusitem),
[`tray-icon` platform notes](https://github.com/tauri-apps/tray-icon), and
[Tauri system tray documentation](https://v2.tauri.app/learn/system-tray/).

## Package and manifest

Proposed layout:

```text
fleet-plugins/maw-menubar/
  plugin.json
  src/plugin.ts
  native/MawMenubar.swift
  bin/maw-menubar
  README.md
```

Proposed manifest contract:

```json
{
  "name": "maw-menubar",
  "version": "0.1.0",
  "sdk": "^1.0.0",
  "runtime": "bun-dev",
  "target": "js",
  "entry": "src/plugin.ts",
  "description": "Native macOS menu bar companion for local maw fleet status.",
  "author": "Soul Brews Studio",
  "license": "MIT",
  "weight": 50,
  "schemaVersion": 1,
  "capabilities": [
    "net:fetch:maw-serve",
    "proc:exec:maw",
    "proc:exec:launchctl",
    "fs:read:plugin",
    "fs:write:launch-agents"
  ],
  "endpoints": {
    "maw-serve": {
      "baseUrl": "http://127.0.0.1:3456",
      "methods": ["GET"],
      "paths": [
        "/api/health",
        "/api/sessions",
        "/api/agents",
        "/api/feed",
        "/api/schedule/status"
      ]
    }
  },
  "cli": {
    "command": "menubar",
    "help": "maw menubar <install|start|stop|status|uninstall|run>"
  }
}
```

`run` is private/internal even if the dispatcher must recognize it. `install`, `start`,
`stop`, `status`, and `uninstall` are idempotent. The final capability vocabulary may
need a manifest-registry addition for `fs:write:launch-agents`; it must not be widened to
generic home-directory write access. No tmux, vault, federation secret, or arbitrary
network capability is required.

## Data flow and trust boundary

The helper is a presentation client, not a second resolver:

```text
AppKit status item
  -> URLSession GET http://127.0.0.1:3456/api/*
  <- versioned JSON snapshots from maw serve
  -> explicit menu action
  -> Process(executableURL: absoluteMawPath, arguments: fixedArgv)
  <- exit status/stdout/stderr summarized in the menu
```

Poll in a background task with a two-second request timeout. Refresh health every five
seconds and the remaining snapshots every ten seconds, coalescing overlapping polls.
Only the main thread mutates `NSStatusItem` or `NSMenu`. Keep the last good snapshot and
its age; a failed poll changes the connection state but never turns old data into current
data.

V1 consumes:

- `GET /api/health` for reachability and server identity;
- `GET /api/sessions?local=true` for local sessions/windows;
- `GET /api/agents` for the canonical agent count and targets;
- `GET /api/feed?limit=20` for recent lifecycle/done-report-like activity;
- `GET /api/schedule/status` for reservations and latest fire outcomes.

The first four routes already exist. Schedule state must be exposed by one small
read-only `maw serve` route before the schedule badge ships. That route, not the helper,
reads the stable `~/.maw/state/schedule/runs/` witness API and returns counts for active
reservations, recent failures, stale outcomes, and latest completion. The menu bar must
never read schedule files, tmux, fleet JSON, or process tables directly.

The base URL is fixed to loopback in v1. The current serve default exempts loopback from
token auth. If an operator disables that exemption and a request returns 401, the helper
shows `Authentication required`; it does not scrape config or copy a token into its
plist. Authenticated local clients are a later, separately designed capability.

Quick actions invoke an absolute `maw` binary with structural argv, never a shell string.
V1 actions are `Refresh now`, `maw peek <target>`, `maw wake <target>`, and `Quit`. A menu
click is the explicit authorization for the action. The result line includes success or
the bounded error text; secrets and full captured pane output are never placed in logs.
Attaching a terminal, free-form task entry, destructive fleet actions, and peer actions
are deferred.

## Menu and status model

The status item uses a monochrome template icon plus a short agent count. Color must not
be the only signal:

- normal: connected, current snapshot, no recent failed fire;
- warning (`!`): failed/stale schedule outcome or a snapshot older than 30 seconds;
- disconnected (`×`): health unavailable or unauthorized;
- idle (`0`): connected with no detected agents.

The menu contains connection/node identity, session and agent counts, schedule summary,
snapshot age, up to ten target rows, up to five recent activity rows, quick actions, and
Quit. Long names are truncated for display but retain the exact server-provided target
for argv. Ordering follows server order or an explicit stable sort; the helper must not
invent target ranking.

## Install and launch

Use a dedicated user LaunchAgent, **not `maw schedule`**. Schedule is a one-shot cadence,
quota, reservation, and outcome engine; using it to supervise a permanent GUI process
would corrupt both its success semantics and the menu bar lifecycle.

`maw menubar install` resolves and records absolute paths, renders
`~/Library/LaunchAgents/com.maw.menubar.plist`, validates it with `plutil`, and applies it
through `launchctl` argv. The plist specifies:

- the packaged helper as the first `ProgramArguments` item;
- `RunAtLoad=true`;
- `KeepAlive` only after unsuccessful exit, plus `ThrottleInterval=10`;
- `LimitLoadToSessionType=Aqua` and interactive process type;
- absolute stdout/stderr paths under `~/.maw/state/logs/`;
- a minimal explicit `PATH`, with no shell-profile or direnv dependency.

The absolute `maw` path and API URL are separate arguments, not interpolated shell text.
The helper verifies the maw path before enabling actions. Install writes the plist via a
temporary sibling and rename, bootstraps it, and then verifies both `launchctl print` and
`/api/health`. `status` distinguishes plist-on-disk, launchd-loaded, process-running, and
API-connected; loaded must never be reported as working by itself. `uninstall` boots out
the label before removing the plist and leaves logs for diagnosis.

Reuse the safety lessons and deterministic rendering tests from
`maw-schedule-launchd`, but do not add a schedule entry. A later shared LaunchAgent
library is reasonable only after a second implementation proves the common boundary.

## V1 scope and deferred work

V1 ships:

1. the signed/universal Swift/AppKit helper and bun-dev lifecycle plugin;
2. dedicated LaunchAgent install/status/uninstall;
3. single-node polling of health, sessions, agents, feed, and schedule status;
4. stale/disconnected/failed-fire indicators with last-good snapshot age;
5. bounded target rows and explicit peek/wake actions through real `maw` argv;
6. structured local logs and a diagnostic `maw menubar status` command.

Deferred:

- federated aggregation and remote actions;
- auto-starting or owning `maw serve`;
- WebSockets, notifications, popovers, preferences, and custom themes;
- terminal attach, arbitrary commands/tasks, and destructive actions;
- non-loopback/token-auth configuration and secret storage;
- Windows/Linux ports, App Store packaging, and automatic updater behavior.

## Resolved open questions

**OQ-1 — auto-launch `maw serve`? No.** V1 depends on an independently managed serve
process. When absent it shows `Not connected` and an instruction to run `maw serve`; it
must not create a second daemon owner, hide port/auth failures, or enter coupled restart
loops. An explicit `Start maw serve` action can be designed later once serve has one
canonical daemon lifecycle command.

**OQ-2 — single node or federated? Single local node.** Existing sessions and agents
routes are local snapshots, and v1 should establish reliable glance/action behavior
before defining partial-failure, identity, auth, and aggregation semantics across peers.
Federation belongs behind a future server-side aggregate endpoint; the helper must not
fan out to peers itself.

## Verification and rollout

- Parse the proposed manifest with the native manifest loader and assert the capability
  and endpoint policy exactly.
- Unit-test Swift JSON decoding and status derivation with fixtures for healthy, idle,
  stale, failed-fire, 401, malformed, and unavailable responses.
- Run integration tests against a fake loopback server and a fake executable that records
  argv, proving no shell interpolation and no request outside the allowlist.
- Parse the generated plist with `plutil`; fake launchctl for install/start/stop/status
  transitions and atomic replacement failures.
- Canary manually from an empty launchd environment, then test login, crash restart,
  sleep/wake, server restart, plugin upgrade, and uninstall.
- Hold v1 release until status agrees across plist, launchd, process, API health, and
  snapshot freshness. A visible icon alone is not success.
