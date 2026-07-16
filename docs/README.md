# maw-rs docs

- [install.md](install.md) — Install instructions: Homebrew tap (arm64 prebuilt), release installer script, and build-from-source.

## Guides — how-tos for humans and agents

- [guides/adding-a-command.md](guides/adding-a-command.md) — 8-line how-to for adding a new maw-cli command via core_impl/partNN.rs auto-registration (build.rs picks up part files; no manual dispatcher registration).
- [guides/adding-a-plugin-artifact.md](guides/adding-a-plugin-artifact.md) — How-to for the dev-Bun→ship-WASM plugin artifact ladder (manifest roles, sha256 pin lifecycle).
- [guides/codex-team-spawn.md](guides/codex-team-spawn.md) — Canonical playbook for spawning/healing codex coder teams (maw team up vs per-worker maw wake, gotchas, charter/preflight).
- [guides/config-guide.md](guides/config-guide.md) — Weighted config layers (maw.config.<N>.json) supersede legacy maw.config.json.
- [guides/extracting-a-verb.md](guides/extracting-a-verb.md) — How-to for extracting a core CLI verb into a ship-tier WASM plugin in maw-plugins (lean-core criterion, PR-pair choreography, fallthrough).
- [guides/release-and-calver.md](guides/release-and-calver.md) — Condensed release + day-based CalVer guide (promotion flow alpha→main, tag workflow, Homebrew tap verification).

## Reference — stable technical references (mostly maw-js porting snapshots)

- [reference/maw-js-cli-dispatch-chain.md](reference/maw-js-cli-dispatch-chain.md) — Source-derived reference of the maw-js CLI routing model (entry flow, dispatch ladder, registries) the Rust port must preserve.
- [reference/maw-js-plugin-invoke-reference.md](reference/maw-js-plugin-invoke-reference.md) — Source-derived reference for the maw-js plugin invoke path (InvokeContext/InvokeResult contract, registry discovery, TS and WASM invoke protocols).
- [reference/serve-daemon-surface.md](reference/serve-daemon-surface.md) — Survey of maw-js serve-* lifecycle plugins on one Bun gateway (route registries, pipeline order, central auth) as design input for the native serve-daemon.
- [reference/wire-protocol.md](reference/wire-protocol.md) — Reverse-engineered maw-js wire protocol (pinned v26.6.13): serve/gateway routes, hey deliver path, HMAC/v3 auth headers, federation-sync, with capture evidence.

## Design — architecture decisions and proposals

- [design/wasm-migration-design.md](design/wasm-migration-design.md) — P0 design keystone (issue #26 / epic #25) for replacing Bun plugin execution with Extism WASM: host-function I/O boundary, SDK shim, capability gate.
- [design/maw-menubar-plugin.md](design/maw-menubar-plugin.md) — Proposed design (#480) for a macOS Swift/AppKit menubar companion as a bun-dev tier plugin in maw-plugins.
- [design/native-schedule-verb.md](design/native-schedule-verb.md) — Proposed design (#456) for a native macOS launchd-backed `maw schedule` verb absorbing Odin's Python scripts.
- [design/squadron-folder-layout.md](design/squadron-folder-layout.md) — Accepted ADR (#331) for fleet/squads/NN-name/squad.json folder layout while session snapshots stay flat for maw-js compatibility.
- [design/tmux-lifecycle-abi.md](design/tmux-lifecycle-abi.md) — Proposed design (#72) for a typed maw.tmux lifecycle host ABI (launch/kill/layout/context) replacing raw tmux argv for new plugins.
- [design/unified-resolver.md](design/unified-resolver.md) — Proposed architecture (epic #318) for one target-resolution contract: pure maw-matcher resolver + per-invocation ResolverSnapshot catalog.

## Parity — maw-js → maw-rs tracking

- [parity/parity-matrix.md](parity/parity-matrix.md) — The maw-js → maw-rs parity finish-line checklist (issue #76): 133 verbs classified native/WASM/stub/not-ported with evidence.
- [parity/scorecard.json](parity/scorecard.json) — Frozen denominator-v3 parity scorecard (2026-07-09): 118-verb surface, gate thresholds, critical-verb set, amendment policy.

## Archive — historical relics, untouched

- [archive/zero-bun/](archive/zero-bun/) — ZERO-BUN-era Bun bridge script; fully superseded.
