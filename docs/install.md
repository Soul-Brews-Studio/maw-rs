# Install maw

## Homebrew (macOS Apple Silicon)

Stable releases are published as a prebuilt arm64 binary, so a Rust toolchain is not
required:

```bash
brew install soul-brews-studio/maw/maw
maw --version
maw ls
```

The formula verifies the release asset SHA-256 and installs the zsh completion generated
by `maw completions zsh`. Homebrew updates the tap during `brew update`; install the next
stable CalVer release with:

```bash
brew upgrade maw
```

To hold the currently installed release, use `brew pin maw`; use `brew unpin maw` before
upgrading. CI can pin the formula definition itself to a tap commit:

```bash
brew install --formula \
  https://raw.githubusercontent.com/Soul-Brews-Studio/homebrew-maw/<tap-commit>/Formula/maw.rb
```

`brew install soul-brews-studio/maw/maw --HEAD` is the source-build fallback and installs
Rust as a build-only dependency. The stable formula never invokes Cargo.

## Release installer

The signed-off release path also supports macOS arm64 and Linux x86_64 binaries:

```bash
curl -fsSLO https://github.com/Soul-Brews-Studio/maw-rs/releases/latest/download/install.sh
sh install.sh
```

Pin a CalVer release with `sh install.sh v26.7.5` or `MAW_VERSION=v26.7.5 sh install.sh`.
The installer verifies the adjacent `.sha256` asset before replacing `maw`.

### Linux: two binaries, and why the choice matters

Every release ships two Linux x86_64 builds from the same commit with the same
feature set. They differ only in C library:

| asset | linkage | `.local` / mDNS |
| --- | --- | --- |
| `maw-rs-linux-x86_64-gnu` | dynamic, needs a glibc host | **resolves** through the system resolver (NSS) |
| `maw-rs-linux-x86_64-musl` | static, runs on any distro | **cannot resolve** |

musl has no glibc NSS, so a musl binary never loads `mdns4_minimal` from
`/etc/nsswitch.conf` and cannot resolve `*.local` names at all (#812). On a
fleet where peers are addressed by `.local` hostnames, that breaks every
cross-machine peer except loopback — so a glibc host wants `-gnu`.

`install.sh` and `maw update` detect the host libc the same way and prefer
`-gnu` when glibc is proven, falling back to `-musl` whenever the evidence is
unclear (a static binary that runs beats a dynamic one that does not):

1. `MAW_LIBC=gnu|musl` forces the answer.
2. `ldd --version` naming musl means musl.
3. `ldd --version` naming GNU/GLIBC means glibc.
4. a glibc dynamic loader on disk (e.g. `/lib/x86_64-linux-gnu/libc.so.6`) means glibc.
5. otherwise musl.

```bash
MAW_LIBC=gnu sh install.sh     # force the glibc build
MAW_LIBC=musl sh install.sh    # force the static build
```

The `-gnu` asset is built against the CI runner's glibc, so a host with an
older glibc cannot run it. Rather than hardcode a version floor that drifts
with the runner image, `install.sh` runs the downloaded binary once
(`--version`) before installing it and silently re-downloads the musl asset if
it will not start. `maw update` proves the new binary the same way, restores
the previous one when the proof fails, and tells you to retry with
`MAW_LIBC=musl`.

## Build from source

For development builds:

```bash
cargo install --path crates/maw-cli --features wasm-host
ln -sf "$(command -v maw-rs)" "$HOME/.local/bin/maw"
```

`--features wasm-host` compiles in the Extism runtime that runs ship-tier WASM
plugin verbs (the fleet plugins). Omitting it gives the lean dev build — ~44%
fewer crates to compile — which keeps every native verb plus plugin discovery,
manifest parsing, and sha256 pin verification, but errors with a rebuild hint
when a WASM plugin verb is invoked.
