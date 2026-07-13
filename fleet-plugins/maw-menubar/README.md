# maw-menubar

Native macOS menu bar companion for glanceable local maw fleet status and explicit quick
actions. This package follows the `bun-dev` native-subprocess tier: TypeScript owns CLI
lifecycle orchestration and a Swift/AppKit helper will own the long-running UI.

## PR-A status

This first slice freezes the package, manifest, capabilities, and lifecycle command
surface. Every mutating/runtime command currently fails closed with an explicit skeleton
message. It does not write a plist, call `launchctl`, compile Swift, or start a process.

```console
maw menubar install
maw menubar start
maw menubar stop
maw menubar status
maw menubar uninstall
```

`maw menubar run` is reserved for the private LaunchAgent entrypoint.

## Architecture boundary

The finished helper will poll the loopback `maw serve` API on port 3456. It must not read
tmux, fleet JSON, schedule state, or process tables directly. Quick actions execute an
absolute `maw` path with argv arrays, never interpolated shell commands. V1 remains
single-node and does not auto-launch `maw serve`.

The approved architecture is documented in
[`docs/design/maw-menubar-plugin.md`](../../docs/design/maw-menubar-plugin.md).

## Development

```console
bun run typecheck
bun test
swift test
swift build -c release
```

The dependency-free Swift/AppKit helper, typed API models, polling, and structural quick
actions live under `native/`. PR-C will add deterministic LaunchAgent rendering and
lifecycle mutation after the native UI has been reviewed independently.
