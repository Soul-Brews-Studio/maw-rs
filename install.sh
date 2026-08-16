#!/bin/sh
set -eu

REPO="Soul-Brews-Studio/maw-rs"
GITHUB_API="https://api.github.com/repos/$REPO/releases?per_page=100"
GITHUB_RELEASES="https://github.com/$REPO/releases"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
MAW_VERSION="${MAW_VERSION:-}"
MAW_CHANNEL="${MAW_CHANNEL:-alpha}"
MAW_LIBC="${MAW_LIBC:-}"
MAW_ADD_TO_PATH="${MAW_ADD_TO_PATH:-0}"
# Glibc loader/libc paths probed when `ldd --version` gives no answer. Space
# separated; overridable only so scripts/test-install-resolve.sh can simulate a
# host with no glibc. Mirrors UPDATE_GLIBC_LOADER_PATHS in update_plan.rs.
MAW_LIBC_LOADER_PATHS="${MAW_LIBC_LOADER_PATHS:-/lib/x86_64-linux-gnu/libc.so.6 /usr/lib/x86_64-linux-gnu/libc.so.6 /lib64/ld-linux-x86-64.so.2 /lib/ld-linux-x86-64.so.2 /usr/lib64/libc.so.6}"

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
  sh install.sh --add-to-path

Environment:
  MAW_VERSION   Release tag to install (overrides channel resolution)
  MAW_CHANNEL   Release channel: alpha or stable (default: alpha)
  MAW_LIBC      Linux C library to install: gnu or musl (default: autodetect)
                gnu  = dynamic, needs a glibc host, resolves .local/mDNS
                musl = static, portable, CANNOT resolve .local/mDNS
  MAW_ADD_TO_PATH
                Set to 1 to append INSTALL_DIR to ~/.profile (default: 0)
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
      --add-to-path)
        MAW_ADD_TO_PATH=1
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

# Like download_to, but reports a MISSING remote object separately from every
# other failure. Prints "found", "missing", or "error"; never exits.
#
# Only a 404 means "this release does not carry that asset", which is
# recoverable by choosing a different one. A 403, a 5xx, a rate-limit or a
# dropped connection must NOT be treated as absence: downgrading the artifact
# because GitHub had a bad minute would silently hand a glibc host the musl
# build, which cannot resolve .local names (#812) — reintroducing the exact bug
# this change exists to fix, invisibly. Ambiguity reports "error" and the
# caller dies loudly.
download_probe_to() {
  probe_url=$1
  probe_out=$2
  if have curl; then
    # No -f: we want the status line rather than a generic exit 22 for every
    # 4xx/5xx. The error body lands in $probe_out and is discarded by the
    # caller on any non-200.
    probe_code=$(curl -sSL -o "$probe_out" -w '%{http_code}' "$probe_url" 2>/dev/null) || {
      printf 'error\n'
      return
    }
  elif have wget; then
    # wget collapses all server errors into exit 8, so read the status line it
    # prints with -S (on stderr) instead of trusting the exit code.
    probe_headers=$(wget -S -q -O "$probe_out" "$probe_url" 2>&1) || true
    probe_code=$(printf '%s\n' "$probe_headers" |
      awk '/^[[:space:]]*HTTP\/[0-9.]+ [0-9]+/ { code = $2 } END { print code }')
    [ -n "$probe_code" ] || probe_code=000
  else
    die "need curl or wget to download releases"
  fi
  case "$probe_code" in
    200) printf 'found\n' ;;
    404) printf 'missing\n' ;;
    *) printf 'error\n' ;;
  esac
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

  case "$MAW_CHANNEL" in
    alpha|stable) ;;
    *) die "MAW_CHANNEL must be alpha or stable" ;;
  esac

  releases_json=$(download_stdout "$GITHUB_API")
  if have jq; then
    tags=$(printf '%s\n' "$releases_json" | jq -r '.[] | .tag_name // empty')
  else
    tags=$(printf '%s\n' "$releases_json" | tr '{' '\n' | sed -n 's/^[[:space:]]*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p')
  fi
  tag=$(printf '%s\n' "$tags" | awk -v channel="$MAW_CHANNEL" '
    function numeric(value) {
      return value != "" && value ~ /^[0-9]+$/ && length(value) <= 6
    }
    function newer(year, month, day, alpha) {
      return !found || year > best_year ||
        (year == best_year && month > best_month) ||
        (year == best_year && month == best_month && day > best_day) ||
        (year == best_year && month == best_month && day == best_day && alpha > best_alpha)
    }
    {
      tag = $0
      rest = substr(tag, 2)
      alpha = 0
      if (channel == "alpha") {
        count = split(rest, parts, "-alpha.")
        if (count != 2 || !numeric(parts[2])) next
        base = parts[1]
        alpha = parts[2] + 0
      } else {
        if (rest ~ /-/) next
        base = rest
      }
      count = split(base, date, ".")
      if (substr(tag, 1, 1) != "v" || count != 3 ||
          !numeric(date[1]) || !numeric(date[2]) || !numeric(date[3])) next
      year = date[1] + 0
      month = date[2] + 0
      day = date[3] + 0
      if (newer(year, month, day, alpha)) {
        found = 1
        best_year = year
        best_month = month
        best_day = day
        best_alpha = alpha
        best_tag = tag
      }
    }
    END { if (found) print best_tag }
  ')

  [ -n "$tag" ] || die "failed to resolve latest maw-rs $MAW_CHANNEL release tag"
  case "$tag" in
    v*) printf '%s\n' "$tag" ;;
    *) die "latest release tag is not a v* tag: $tag" ;;
  esac
}

# Which Linux C library this host runs: prints "gnu" or "musl".
#
# Two Linux x86_64 builds ship per release and they are NOT interchangeable.
# The musl build is static and runs on any distro, but musl has no glibc NSS,
# so it cannot load mdns4_minimal from /etc/nsswitch.conf and CANNOT resolve
# *.local names at all (#812) — which breaks every cross-machine peer on a
# .local hostname. The gnu build needs a glibc host and resolves .local through
# the system resolver.
#
# Rules, in order (mirrored by update_classify_libc in the Rust updater so
# `maw update` and install.sh always agree):
#   1. MAW_LIBC=gnu|musl forces the answer.
#   2. `ldd --version` naming musl  -> musl  (musl's own ldd says "musl";
#      it writes to stderr and exits nonzero, so both streams are captured).
#   3. `ldd --version` naming GNU/GLIBC -> gnu.
#   4. a glibc dynamic loader present on disk -> gnu.
#   5. anything else -> musl. Ambiguity always degrades to the static build:
#      a binary that runs beats a dynamic one that does not.
# Strip leading/trailing whitespace and lowercase, matching Rust's
# `.trim().to_ascii_lowercase()`. The Rust side normalized and this side did
# not, so the same input picked different assets: MAW_LIBC="MUSL" resolved to
# musl for `maw update` and to gnu for the installer. Only the ends are
# trimmed — deleting interior whitespace would accept "mu sl", which Rust
# rejects, trading one divergence for another.
normalize_libc_token() {
  token=$1
  while :; do
    case $token in
      [[:space:]]*) token=${token#?} ;;
      *) break ;;
    esac
  done
  while :; do
    case $token in
      *[[:space:]]) token=${token%?} ;;
      *) break ;;
    esac
  done
  printf '%s' "$token" | tr '[:upper:]' '[:lower:]'
}

detect_linux_libc() {
  case "$(normalize_libc_token "${MAW_LIBC:-}")" in
    gnu|glibc) printf 'gnu\n'; return ;;
    musl) printf 'musl\n'; return ;;
    '') ;;
    *) warn "ignoring unknown MAW_LIBC=\"$MAW_LIBC\" (expected gnu or musl)" ;;
  esac

  ldd_out=$(ldd --version 2>&1 || true)
  case "$(printf '%s' "$ldd_out" | tr '[:upper:]' '[:lower:]')" in
    *musl*) printf 'musl\n'; return ;;
    *glibc*|*"gnu libc"*|*"gnu c library"*) printf 'gnu\n'; return ;;
    *) ;;
  esac

  # Split on spaces by hand: zsh does not word-split unquoted expansions, and
  # a `while read` loop would run in a subshell where `return` cannot answer.
  remaining=$MAW_LIBC_LOADER_PATHS
  while [ -n "$remaining" ]; do
    candidate=${remaining%% *}
    case "$remaining" in
      *' '*) remaining=${remaining#* } ;;
      *) remaining= ;;
    esac
    if [ -n "$candidate" ] && [ -e "$candidate" ]; then
      printf 'gnu\n'
      return
    fi
  done

  printf 'musl\n'
}

detect_platform() {
  os=$(uname -s 2>/dev/null || printf unknown)
  arch=$(uname -m 2>/dev/null || printf unknown)
  case "$os:$arch" in
    Darwin:arm64|Darwin:aarch64)
      printf '%s\n' "maw-rs-macos-arm64"
      ;;
    Linux:x86_64|Linux:amd64)
      printf '%s\n' "maw-rs-linux-x86_64-$(detect_linux_libc)"
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
  case "$(download_probe_to "$base/$asset" "$bin")" in
    found) ;;
    missing)
      # This release predates the gnu artifact, or a pinned/stable tag will
      # never carry it. Only ever downgrade gnu -> musl; a missing musl or
      # macOS asset is a real packaging failure and must still be fatal.
      case "$asset" in
        *-gnu)
          warn "$asset is not published for $tag — falling back to the static musl build"
          warn "note the musl build CANNOT resolve .local/mDNS names (#812)"
          warn "to get the glibc build, install a release that ships it"
          asset=maw-rs-linux-x86_64-musl
          bin="$tmpdir/$asset"
          sidecar="$tmpdir/$asset.sha256"
          say "downloading: $base/$asset"
          download_to "$base/$asset" "$bin"
          ;;
        *) die "release $tag does not contain $asset" ;;
      esac
      ;;
    *) die "failed to download $asset from $tag (network or server error)" ;;
  esac
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

# Can this host actually execute the downloaded binary?
#
# The gnu asset is dynamic, so it needs a glibc at least as new as the one CI
# built it against. Rather than hardcode a floor that silently drifts every
# time the CI runner image moves, just run the thing once: an empirical answer
# never goes stale.
binary_runs_here() {
  "$1" --version >/dev/null 2>&1
}

# gnu chosen but unrunnable (glibc too old) -> re-download the static musl
# asset, which runs anywhere. Reads and updates the globals `asset` and
# `DOWNLOADED_BIN` in place (no subshell: a command substitution would throw
# the new download away). Only ever downgrades gnu -> musl, so macOS and musl
# installs behave exactly as they did before.
fallback_if_unrunnable() {
  fallback_tag=$1
  fallback_tmpdir=$2
  case "$asset" in
    *-gnu) ;;
    *) return 0 ;;
  esac
  probe_status=0
  binary_runs_here "$DOWNLOADED_BIN" || probe_status=$?
  if [ "$probe_status" -eq 0 ]; then
    return 0
  fi
  # POSIX reserves 126 for "found but could not be executed", which is what a
  # noexec mount (a standard CIS hardening on /tmp) produces. That is a
  # statement about the DIRECTORY, not about the binary — but this probe cannot
  # tell the difference, so it must not pretend to. Downgrading silently here
  # would regress hosts that install fine today, so say exactly what happened
  # and how to override it. See #812 follow-up.
  if [ "$probe_status" -eq 126 ]; then
    warn "could not execute the downloaded binary to verify it (exit 126)"
    warn "this usually means the download directory is mounted noexec, not that the build is wrong"
    warn "falling back to the static musl build, which may not be what you want here"
    warn "to keep the glibc build: re-run with MAW_LIBC=gnu and TMPDIR set to an executable directory"
  else
    warn "the glibc build will not start on this host (glibc older than the build's?)"
  fi
  warn "falling back to the static musl build — note it CANNOT resolve .local/mDNS names"
  asset=maw-rs-linux-x86_64-musl
  download_and_verify "$fallback_tag" "$asset" "$fallback_tmpdir"
  binary_runs_here "$DOWNLOADED_BIN" ||
    die "neither Linux asset runs on this host; build from source"
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

configure_install_path() {
  path_contains_install_dir && return
  if [ "$MAW_ADD_TO_PATH" != 1 ]; then
    warn "$INSTALL_DIR is not on PATH"
    warn "add this to your shell profile: export PATH=\"$INSTALL_DIR:\$PATH\""
    return
  fi

  profile="$HOME/.profile"
  # shellcheck disable=SC2016 # Persist a literal $PATH for future shells.
  export_line=$(printf 'export PATH="%s:$PATH"' "$INSTALL_DIR")
  if [ ! -f "$profile" ] || ! grep -Fqx "$export_line" "$profile"; then
    printf '%s\n' "$export_line" >>"$profile"
    say "added PATH export to $profile: $export_line"
  else
    say "PATH export already present in $profile"
  fi
  say "run: . \"$profile\" (or open a new shell)"
}

post_install() {
  say "verified sha256: $VERIFIED_HASH"
  say "installed: $INSTALLED_PATH"
  configure_install_path
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
  case "$asset" in
    *-gnu)
      say "  gnu build: dynamic (glibc host), resolves .local/mDNS via the system resolver"
      ;;
    *-musl)
      say "  musl build: static and portable, but CANNOT resolve .local/mDNS names"
      say "  on a glibc host run again with MAW_LIBC=gnu to get the dynamic build"
      ;;
    *) ;;
  esac
  say "version: $tag"
  download_and_verify "$tag" "$asset" "$tmpdir"
  fallback_if_unrunnable "$tag" "$tmpdir"
  install_binary "$DOWNLOADED_BIN"
  post_install
}

if [ "${MAW_INSTALL_TESTING:-0}" != 1 ]; then
  main "$@"
fi
