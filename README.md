# maw-rs

[![CI](https://github.com/Soul-Brews-Studio/maw-rs/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/Soul-Brews-Studio/maw-rs/actions/workflows/ci.yml)
![version](https://img.shields.io/badge/version-v0.1.0--alpha.1-blue)
![coverage](https://img.shields.io/badge/coverage-99.85%25-brightgreen)

Rust port of the maw-js portable core.

## Install maw-rs

On macOS Apple Silicon, install the stable prebuilt binary and zsh completions without a
Rust toolchain:

```bash
brew install soul-brews-studio/maw/maw
```

See [`docs/install.md`](docs/install.md) for upgrades, pinned CI installs, the release
installer, and source builds.

macOS Apple Silicon and Linux x86_64 prebuilt binaries are published on tagged releases.
The installer downloads the matching asset, verifies its `.sha256` sidecar, backs
up any existing `maw`, and installs to `~/.local/bin/maw` by default.

Stable release installer:

```bash
curl -fsSL https://github.com/tonkmac/maw-rs/releases/latest/download/install.sh | sh
```

Bleeding-edge installer from `alpha`:

```bash
curl -fsSL https://raw.githubusercontent.com/tonkmac/maw-rs/alpha/install.sh | sh
```

Pin the installer and binary to a specific release:

```bash
curl -fsSL https://github.com/tonkmac/maw-rs/releases/download/v0.1.0-alpha.X/install.sh | MAW_VERSION=v0.1.0-alpha.X sh
```

Options:

```bash
MAW_VERSION=v0.1.0-alpha.X sh install.sh
INSTALL_DIR="$HOME/bin" sh install.sh
sh install.sh --version v0.1.0-alpha.X --install-dir "$HOME/bin"
```

Supported prebuilt platforms:

- `maw-rs-macos-arm64` — macOS Apple Silicon
- `maw-rs-linux-x86_64-musl` — Linux x86_64 static binary

Manual fallback:

1. Download the matching binary and `.sha256` sidecar from a release.
2. Verify the SHA-256 hash.
3. `chmod +x` the binary and move or symlink it as `maw` in your `PATH`.

If `~/.local/bin` is not on `PATH`, add it to your shell profile. If macOS
Gatekeeper blocks the binary, run:

```bash
xattr -d com.apple.quarantine ~/.local/bin/maw
```

`maw-rs` is intentionally starting with deterministic, side-effect-free crates.
Each crate copies the same JSON fixture contract from `maw-js/test/spec/` and
must pass those fixtures in Rust before runtime IO, transports, or CLI commands
move over.

## Plugin build/dev support

`maw-rs` supports native Rust-WASM plugin builds. The supported authoring path is:

```bash
maw plugin create --rust my-plugin
cd my-plugin
maw plugin build
```

That path builds a `wasm32-unknown-unknown` artifact with Cargo, writes a
`dist/plugin.json` artifact contract, and is loaded through the native Extism
WASM runtime.

The `maw-rs` ship-tier WASM builder does not yet compile JS/TS source: it vendors
no JS-to-WASM compiler, and the pinned host has no Bun subprocess fallback. This
is a boundary of that deployment path, not a ban on Bun; Bun/JS fleet plugins and
dev-tier surfaces remain first-class alongside Rust. A JS/TS plugin entering the
ship-tier WASM host must currently provide a prebuilt artifact with `target =
"wasm"` and a relative `wasm` path in `plugin.json`. A future Javy/QuickJS-style
toolchain would need a separate design and security review.

## Phase 1 status

Cargo workspace scaffolded and pushed to `main`.

| Crate | maw-js source | Portable fixture |
| --- | --- | --- |
| `maw-matcher` | `src/core/matcher/resolve-target.ts`, `normalize-target.ts` | `matcher-resolve-target.fixtures.json`, `normalize-target.fixtures.json` |
| `maw-calver` | `scripts/calver.ts` | `calver.fixtures.json` |
| `maw-policy` | `src/plugin/default-active.ts`, `src/plugin/tier.ts`, `src/plugin/manifest-constants.ts` | `plugin-policy.fixtures.json` |
| `maw-worktree` | `src/core/fleet/worktree-window-match.ts` | `worktree-window-match.fixtures.json` |
| `maw-transport` | `src/core/transport/transport.ts` | `transport-router.fixtures.json` |
| `maw-routing` | `src/core/routing.ts` | `routing.fixtures.json` |
| `maw-identity` | `src/core/fleet/session-name.ts`, `src/core/fleet/node-identity.ts`, `src/core/fleet/validate.ts` | `canonical-session-name.fixtures.json`, `canonical-node-identity.fixtures.json`, `test/validate-oracle-name.test.ts` |
| `maw-bring` | `src/commands/shared/bring-flags.ts` | `bring-to-flag.fixtures.json`, `bring-to-target.fixtures.json`, `bring-self-guard.fixtures.json` |
| `maw-split` | `src/vendor/mpr-plugins/split/impl.ts`, `src/commands/plugins/tmux/safety.ts` | `split-policy.fixtures.json` |
| `maw-peer` | `src/commands/shared/peer-sources.ts` | `peer-source-resolver.fixtures.json` |
| `maw-tmux` | `src/core/transport/tmux-class.ts`, `src/commands/shared/discover-live-state.ts` | tmux parser unit tests, `discover-tmux-live-state.fixtures.json` |
| `maw-hub` | `src/transports/hub-config.ts` | `test/isolated/hub-config.test.ts`, hub config loader coverage |
| `maw-feed` | `src/lib/feed.ts` | `test/isolated/feed-lib-coverage.test.ts` |
| `maw-auth` | `src/lib/federation-auth.ts` | federation auth pure helper and O6 decision tests |
| `maw-xdg` | `src/core/xdg.ts`, `src/core/paths.ts`, `src/cli/instance-preset.ts` | `test/core-xdg.test.ts`, `test/paths.test.ts`, `test/00-resolve-home.test.ts` |
| `maw-bind` | `src/core/bind-host.ts` | `test/bind-heuristic.test.ts` |
| `maw-fuzzy` | `src/core/util/fuzzy.ts` | `test/fuzzy-match.test.ts` |
| `maw-plugin-scaffold` | `src/commands/shared/plugin-create-scaffold.ts` | `test/plugin-create.test.ts` (pure validation/manifest cases) |
| `maw-plugin-manifest` | `src/plugin/manifest-validate.ts` | `test/plugin-manifest-validate-edges.test.ts` (cli/api validators) |

Current local gates:

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Phase 2 plan

1. Add side-effecting transport implementations behind the `maw-transport` policy:
   - tmux via the `tmux` CLI
   - HTTP federation via injectable IO first; runtime HTTP client wiring later
   - Zenoh via the Rust stylos/themion ecosystem
2. Add runtime adapters for fleet/worktree/session discovery around the pure crates.
3. Keep maw-js and maw-rs running side-by-side until command parity is proven.

## Phase 3 plan

1. Add a `maw-rs` CLI with `clap`.
2. Port high-value fast-path commands first: `ls`, `hey`, `peek`, and target resolution helpers.
3. Validate each command against maw-js fixtures or captured golden outputs before replacing any default `maw` entrypoint.
