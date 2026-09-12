# Oracle Workboard: community sidecar

Workboard is discoverable here without folding its runtime into maw-rs core
(issue #970). It is a community project, not a bundled or validated ship-tier
plugin.

- [Plugin and usage instructions](https://github.com/MEYD-605/maw-workboard)
- [Oracle Board runtime](https://github.com/MEYD-605/maw-board)

## Compatibility before installation

As inspected on 2026-09-12, the upstream plugin manifest declares
`entry: ./index.ts`, SDK `^1.0.0`, and process/filesystem/network capabilities.
It does not declare a ship-tier `target: wasm` artifact. Its README requires
maw-js and Bun. Do not assume its installation recipe works in the native
maw-rs ship-tier host: that host has no Bun subprocess fallback. The issue's
reported macOS/Linux runtime checks do not prove native plugin-host parity.

Follow upstream instructions in its supported environment. Do not disable
capability enforcement to make a legacy plugin load. Treat a web terminal as
command execution: review its authentication and network binding before
exposing it beyond loopback; do not publish session URLs or passwords.

## Path to official ship-tier catalog admission

Keep the sidecar external. Before an official artifact is advertised, provide
a compatible prebuilt WASM manifest/artifact, narrowly scoped host capabilities
for the lifecycle operations, immutable artifact hashes, and native-host
install/start/status/stop plus missing/refused-path tests on supported platforms.
See [adding a plugin artifact](adding-a-plugin-artifact.md). No runtime or
registry pin is added by this discovery-only documentation.
