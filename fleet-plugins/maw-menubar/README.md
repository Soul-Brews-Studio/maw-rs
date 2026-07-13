# maw-menubar

Native macOS menu bar companion for glanceable local maw fleet status and explicit quick
actions. This package follows the `bun-dev` native-subprocess tier: TypeScript owns CLI
lifecycle orchestration and a Swift/AppKit helper will own the long-running UI.

## Status

The plugin renders and atomically installs a dedicated user LaunchAgent, validates it
with `plutil`, and manages it through structural `launchctl` argv. `status` reports the
plist, launchd, process, and API states independently.

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

The Swift/AppKit helper source lives under `native/`. A release helper must be placed at
`bin/maw-menubar` before `install`; normal installation never compiles Swift on the
operator's machine.

## Distribution build

The distributable plugin contains a checked, universal helper at `bin/maw-menubar`;
installing it does not run Swift or require a Swift toolchain. Rebuild the helper on
macOS whenever `native/` changes:

```console
bun run build:native
```

The script performs separate release builds for `arm64` and `x86_64`, combines them
with `lipo`, applies an ad-hoc signature, and rejects output missing either slice.
`plugin.json` pins the resulting helper SHA-256 in `bundledArtifacts`. The native JS
plugin packer verifies that checksum
and copies the helper to `dist/bin/maw-menubar`, which the installer preserves.
