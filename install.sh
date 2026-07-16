#!/bin/sh
set -eu

REPO="Soul-Brews-Studio/maw-rs"
GITHUB_API="https://api.github.com/repos/$REPO/releases/latest"
GITHUB_RELEASES="https://github.com/$REPO/releases"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
MAW_VERSION="${MAW_VERSION:-}"

say() {
  printf '%s\n' "$*"
}

warn() {
  printf 'warning: %s\n' "$*" >&2
}

die() {
  printf 'install.sh: %s\n' "$*" >&2
  exit 1
}

usage() {
  cat <<'USAGE'
maw-rs installer

Usage:
  sh install.sh [vX.Y.Z]
  sh install.sh --version vX.Y.Z
  sh install.sh --install-dir /path/to/bin

Environment:
  MAW_VERSION   Release tag to install (default: latest release)
  INSTALL_DIR   Install directory (default: ~/.local/bin)
USAGE
}

have() {
  command -v "$1" >/dev/null 2>&1
}

parse_args() {
  while [ "$#" -gt 0 ]; do
    case "$1" in
      --version)
        shift
        [ "$#" -gt 0 ] || die "--version requires a value"
        MAW_VERSION="$1"
        ;;
      --install-dir)
        shift
        [ "$#" -gt 0 ] || die "--install-dir requires a value"
        INSTALL_DIR="$1"
        ;;
      -h|--help)
        usage
        exit 0
        ;;
      v*)
        MAW_VERSION="$1"
        ;;
      *)
        die "unknown argument: $1"
        ;;
    esac
    shift
  done
}

download_to() {
  url=$1
  out=$2
  if have curl; then
    curl -fsSL -o "$out" "$url"
  elif have wget; then
    wget -q -O "$out" "$url"
  else
    die "need curl or wget to download releases"
  fi
}

download_stdout() {
  url=$1
  if have curl; then
    curl -fsSL "$url"
  elif have wget; then
    wget -q -O - "$url"
  else
    die "need curl or wget to download releases"
  fi
}

resolve_version() {
  if [ -n "$MAW_VERSION" ]; then
    case "$MAW_VERSION" in
      v*) printf '%s\n' "$MAW_VERSION" ;;
      *) die "MAW_VERSION must be a release tag starting with v" ;;
    esac
    return
  fi

  latest_json=$(download_stdout "$GITHUB_API")
  if have jq; then
    tag=$(printf '%s\n' "$latest_json" | jq -r '.tag_name // empty')
  else
    tag=$(printf '%s\n' "$latest_json" | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | sed -n '1p')
  fi

  [ -n "$tag" ] || die "failed to resolve latest maw-rs release tag"
  case "$tag" in
    v*) printf '%s\n' "$tag" ;;
    *) die "latest release tag is not a v* tag: $tag" ;;
  esac
}

detect_platform() {
  os=$(uname -s 2>/dev/null || printf unknown)
  arch=$(uname -m 2>/dev/null || printf unknown)
  case "$os:$arch" in
    Darwin:arm64|Darwin:aarch64)
      printf '%s\n' "maw-rs-macos-arm64"
      ;;
    Linux:x86_64|Linux:amd64)
      printf '%s\n' "maw-rs-linux-x86_64-musl"
      ;;
    *)
      die "no prebuilt binary for $os/$arch; build from source"
      ;;
  esac
}

sha256_file() {
  file=$1
  if have sha256sum; then
    sha256sum "$file" | awk '{print $1}'
  elif have shasum; then
    shasum -a 256 "$file" | awk '{print $1}'
  else
    die "need sha256sum or shasum to verify downloads"
  fi
}

make_tmpdir() {
  tmp=$(mktemp -d 2>/dev/null || mktemp -d -t maw-rs-install)
  printf '%s\n' "$tmp"
}

download_and_verify() {
  tag=$1
  asset=$2
  tmpdir=$3
  base="$GITHUB_RELEASES/download/$tag"
  bin="$tmpdir/$asset"
  sidecar="$tmpdir/$asset.sha256"

  say "downloading: $base/$asset"
  download_to "$base/$asset" "$bin"
  download_to "$base/$asset.sha256" "$sidecar"

  expected=$(awk 'NR == 1 {print $1}' "$sidecar")
  [ -n "$expected" ] || die "empty checksum sidecar for $asset"
  actual=$(sha256_file "$bin")
  if [ "$actual" != "$expected" ]; then
    die "checksum mismatch for $asset"
  fi
  chmod 755 "$bin"
  VERIFIED_HASH=$actual
  DOWNLOADED_BIN=$bin
}

backup_path() {
  dest=$1
  stamp=$(date +%Y%m%d%H%M%S)
  candidate="$dest.bak.$stamp"
  if [ -e "$candidate" ] || [ -L "$candidate" ]; then
    candidate="$candidate.$$"
  fi
  printf '%s\n' "$candidate"
}

install_binary() {
  bin=$1
  [ -n "$INSTALL_DIR" ] || die "INSTALL_DIR must not be empty"
  [ "$INSTALL_DIR" != "/" ] || die "refusing to install directly into /"
  mkdir -p "$INSTALL_DIR"
  dest="$INSTALL_DIR/maw"

  if [ -e "$dest" ] || [ -L "$dest" ]; then
    backup=$(backup_path "$dest")
    mv "$dest" "$backup"
    say "backed up existing maw: $backup"
  fi

  mv "$bin" "$dest"
  INSTALLED_PATH=$dest
}

path_contains_install_dir() {
  case ":$PATH:" in
    *:"$INSTALL_DIR":*) return 0 ;;
    *) return 1 ;;
  esac
}

post_install() {
  say "verified sha256: $VERIFIED_HASH"
  say "installed: $INSTALLED_PATH"
  if ! path_contains_install_dir; then
    warn "$INSTALL_DIR is not on PATH"
    warn "add this to your shell profile: export PATH=\"$INSTALL_DIR:\$PATH\""
  fi
  say "run: maw --version"
  say "hint: if you already run 'maw serve', restart it to use the new binary."
  if [ "$(uname -s 2>/dev/null || printf unknown)" = "Darwin" ]; then
    say "hint: if macOS Gatekeeper blocks maw, run: xattr -d com.apple.quarantine '$INSTALLED_PATH'"
  fi
}

main() {
  parse_args "$@"
  tmpdir=$(make_tmpdir)
  trap 'rm -rf "$tmpdir"' EXIT HUP INT TERM

  tag=$(resolve_version)
  asset=$(detect_platform)
  say "maw-rs installer"
  say "platform asset: $asset"
  say "version: $tag"
  download_and_verify "$tag" "$asset" "$tmpdir"
  install_binary "$DOWNLOADED_BIN"
  post_install
}

main "$@"
