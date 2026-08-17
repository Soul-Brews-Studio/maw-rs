# maw-rs

[![CI](https://github.com/Soul-Brews-Studio/maw-rs/actions/workflows/ci.yml/badge.svg?branch=alpha)](https://github.com/Soul-Brews-Studio/maw-rs/actions/workflows/ci.yml)
[![release](https://img.shields.io/github/v/release/Soul-Brews-Studio/maw-rs?include_prereleases)](https://github.com/Soul-Brews-Studio/maw-rs/releases/latest)

**Run a fleet of AI coding agents across tmux sessions and machines, from one command line.**

Each agent — an *oracle* — lives in its own repo and tmux window. `maw` wakes them, routes
messages between them, moves their panes around, and reaches the ones running on other hosts.

```bash
maw ls                          # what is alive right now
maw wake reviewer               # start an oracle in its repo
maw hey reviewer "check PR 12"  # message it, locally or across machines
maw a reviewer                  # attach to its pane
```

---

## Install

**macOS (Apple Silicon)** — binary and zsh completions, no Rust toolchain:

```bash
brew install soul-brews-studio/maw/maw
```

**Any supported platform** — stable installer:

```bash
curl -fsSL https://github.com/Soul-Brews-Studio/maw-rs/releases/latest/download/install.sh | sh
```

Installs to `~/.local/bin/maw`, verifies the release `.sha256`, and backs up any existing
binary. Add `~/.local/bin` to your `PATH` if it isn't there.

<details>
<summary>Bleeding edge, pinned versions, and install options</summary>

```bash
# latest alpha
curl -fsSL https://raw.githubusercontent.com/Soul-Brews-Studio/maw-rs/alpha/install.sh | sh

# pin to a release (day-based CalVer, e.g. v26.7.16)
curl -fsSL https://github.com/Soul-Brews-Studio/maw-rs/releases/download/v26.7.16/install.sh | MAW_VERSION=v26.7.16 sh

# choose a target directory
INSTALL_DIR="$HOME/bin" sh install.sh
sh install.sh --version v26.7.16 --install-dir "$HOME/bin"
```

Already installed? `maw update` handles upgrades on either channel:

```bash
maw update --check              # is anything newer?
maw update                      # stable
maw update alpha                # alpha channel
```

</details>

<details>
<summary>Platforms, and the one Linux choice that matters</summary>

| Asset | Platform |
| --- | --- |
| `maw-rs-macos-arm64` | macOS Apple Silicon |
| `maw-rs-linux-x86_64-gnu` | Linux x86_64, dynamic — **resolves `.local`/mDNS** |
| `maw-rs-linux-x86_64-musl` | Linux x86_64, static and portable — **cannot** resolve `.local`/mDNS |

The installer autodetects your C library and prefers glibc, falling back to musl only when
glibc isn't proven. Force it with `MAW_LIBC=gnu` or `MAW_LIBC=musl`.

**If your fleet federates over `.local` names, you need `-gnu`.** A musl build has no glibc
NSS, so it never loads `mdns4_minimal` from `/etc/nsswitch.conf` and cannot resolve `*.local`
peers at all.

Manual install: download the binary and its `.sha256`, verify the hash, `chmod +x`, and put
it on your `PATH` as `maw`. If macOS Gatekeeper blocks it:

```bash
xattr -d com.apple.quarantine ~/.local/bin/maw
```

</details>

---

## What it does

**Wake and attach.** Oracles sleep as registry entries and wake into tmux windows on demand.

```bash
maw wake reviewer               # launch its engine pane in its repo
maw a reviewer                  # attach (wakes first if needed)
maw ls                          # live sessions;  --json, --watch, --compact
maw sleep reviewer              # stop one oracle gracefully
```

**Talk between agents — including across machines.** `maw hey` delivers over federation, so
the target can be on another host.

```bash
maw hey reviewer "rebase onto alpha"
maw hey other-box:reviewer "..."   # a peer on another host
maw peek reviewer                  # read a local pane without attaching
```

> `maw peek` is local-only today — it takes a tmux target, not a `<node>:<agent>` form.
> The HTTP API already serves cross-node capture; the CLI verb for it does not exist yet
> ([#820](https://github.com/Soul-Brews-Studio/maw-rs/issues/820)).

**Drive panes directly** when you want to type into an agent rather than message it.

```bash
maw run reviewer "cargo test"   # type it and press Enter
maw send reviewer "partial"     # type without submitting
maw send-enter reviewer         # submit later
maw send-key reviewer C-c       # one allowlisted key
```

**Work on repos and issues.** `maw work` opens a workspace, optionally seeded with task
context; `maw done` finishes and cleans up the worktree.

```bash
maw work .                      # a window for this repo
maw work owner/repo 42          # ...seeded with issue 42
maw done                        # save state, kill window, remove worktree
```

**Run teams.** Several agents on one problem, side by side.

```bash
maw swarm                       # three claude panes (the default)
maw swarm --count 5 --tiled     # five, tiled
maw squad start                 # lead-centric team flow
maw bring reviewer              # pull an oracle into your current session
```

**Grow the fleet.** New oracles bud from existing ones.

```bash
maw bud newname                 # create the workspace
maw awaken newname              # bud + wake + first trigger
```

**Background work** that outlives the current pane:

```bash
maw bg "cargo build --release" --name build
```

`maw help --all` lists every verb — there are ~195, plus installed plugins.

---

## Plugins

Native Rust→WASM plugins, loaded through Extism:

```bash
maw plugin create --rust my-plugin
cd my-plugin
maw plugin build                # → wasm32-unknown-unknown + dist/plugin.json
maw plugin ls -v                # what is installed
maw x <source-spec> --sha256 <hex>   # run one, pin-verified
```

The ship-tier WASM builder does not compile JS/TS source — it vendors no JS-to-WASM compiler
and the pinned host has no Bun subprocess fallback. This is a boundary of that deployment
path, not a ban on Bun: Bun/JS fleet plugins and dev-tier surfaces remain first-class. A JS/TS
plugin entering the ship-tier host must supply a prebuilt artifact with `target = "wasm"` and
a relative `wasm` path in `plugin.json`.

---

## Build from source

```bash
git clone https://github.com/Soul-Brews-Studio/maw-rs
cd maw-rs
cargo build --release           # binary at target/release/maw
```

The toolchain is pinned by `rust-toolchain.toml`, including the `wasm32-unknown-unknown`
target. Don't `rustup update` to fix a build — edit the pin.

```bash
scripts/gate.sh quick           # fmt + clippy + affected tests
scripts/gate.sh full            # the pre-merge bar (all CI dimensions)
```

---

## Docs

| | |
| --- | --- |
| [`docs/install.md`](docs/install.md) | upgrades, pinned CI installs, source builds |
| [`docs/guides/gating.md`](docs/guides/gating.md) | gate tiers, warm cache, merge trains |
| [`docs/`](docs/) | parity matrix, wire protocol, adding a command, WASM design |
| [`CLAUDE.md`](CLAUDE.md) · [`AGENTS.md`](AGENTS.md) | conventions for agents working in this repo |

**Contributing:** PRs target `alpha`, never `main`. `scripts/gate.sh full` must pass before
merge. Releases use day-based CalVer (`v<YY>.<M>.<DD>`, alpha suffixed `-alpha.<HMM>`).

Rust port of maw-js. BUSL-1.1.
