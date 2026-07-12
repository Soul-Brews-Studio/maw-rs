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

The signed-off release path also supports macOS arm64 and static Linux x86_64 binaries:

```bash
curl -fsSLO https://github.com/Soul-Brews-Studio/maw-rs/releases/latest/download/install.sh
sh install.sh
```

Pin a CalVer release with `sh install.sh v26.7.5` or `MAW_VERSION=v26.7.5 sh install.sh`.
The installer verifies the adjacent `.sha256` asset before replacing `maw`.

## Build from source

For development builds:

```bash
cargo install --path crates/maw-cli
ln -sf "$(command -v maw-rs)" "$HOME/.local/bin/maw"
```
