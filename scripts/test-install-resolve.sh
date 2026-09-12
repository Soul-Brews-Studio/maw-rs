#!/bin/sh
set -eu

ROOT=$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)
MAW_INSTALL_TESTING=1
export MAW_INSTALL_TESTING
# shellcheck disable=SC1090
. "$ROOT/install.sh"

fixture='[
  {"tag_name":"v26.7.16-alpha.1159","prerelease":false},
  {"tag_name":"v26.7.22-alpha.1600","prerelease":true},
  {"tag_name":"v26.7.23-alpha.9","prerelease":true},
  {"tag_name":"v26.7.23-alpha.1617","prerelease":true},
  {"tag_name":"v26.7.21","prerelease":false},
  {"tag_name":"v26.7.23","prerelease":false}
]'

# shellcheck disable=SC2329
download_stdout() {
  printf '%s\n' "$fixture"
}

assert_eq() {
  expected=$1
  actual=$2
  label=$3
  if [ "$actual" != "$expected" ]; then
    printf 'FAIL %s: expected %s, got %s\n' "$label" "$expected" "$actual" >&2
    exit 1
  fi
}

MAW_VERSION=
MAW_CHANNEL=alpha
assert_eq "v26.7.23-alpha.1617" "$(resolve_version)" "alpha release-list ordering"

# shellcheck disable=SC2034
MAW_CHANNEL=stable
assert_eq "v26.7.23" "$(resolve_version)" "stable release-list ordering"

# Exercise the no-jq parser used on minimal fresh nodes.
# shellcheck disable=SC2329
have() {
  return 1
}
# shellcheck disable=SC2034
MAW_CHANNEL=alpha
assert_eq "v26.7.23-alpha.1617" "$(resolve_version)" "alpha ordering without jq"

# shellcheck disable=SC2034
MAW_VERSION=v26.7.20-alpha.42
# shellcheck disable=SC2329
download_stdout() {
  printf 'override must not fetch releases\n' >&2
  exit 1
}
assert_eq "v26.7.20-alpha.42" "$(resolve_version)" "MAW_VERSION override"

path_root=$(mktemp -d "${TMPDIR:-/tmp}/maw-install-path.XXXXXX")
trap 'rm -rf "$path_root"' EXIT HUP INT TERM
HOME="$path_root/no-opt-in"
INSTALL_DIR="$HOME/.local/bin"
PATH=/usr/bin:/bin
MAW_ADD_TO_PATH=0
mkdir -p "$HOME"
warning=$(configure_install_path 2>&1)
[ ! -e "$HOME/.profile" ] || {
  printf 'FAIL no opt-in must not create profile\n' >&2
  exit 1
}
case "$warning" in
  *"$INSTALL_DIR is not on PATH"*) ;;
  *)
    printf 'FAIL no opt-in warning missing\n' >&2
    exit 1
    ;;
esac

parse_args --add-to-path
assert_eq 1 "$MAW_ADD_TO_PATH" "--add-to-path parsing"
configure_install_path >/dev/null
configure_install_path >/dev/null
# shellcheck disable=SC2016 # Match the literal $PATH written to the profile.
export_line=$(printf 'export PATH="%s:$PATH"' "$INSTALL_DIR")
line_count=$(grep -Fxc "$export_line" "$HOME/.profile")
assert_eq 1 "$line_count" "idempotent profile export"

HOME="$path_root/already-present"
INSTALL_DIR="$HOME/.local/bin"
PATH="$INSTALL_DIR:/usr/bin:/bin"
mkdir -p "$HOME"
already_output=$(configure_install_path 2>&1)
assert_eq "" "$already_output" "already-on-PATH no-op output"
[ ! -e "$HOME/.profile" ] || {
  printf 'FAIL already-on-PATH must not create profile\n' >&2
  exit 1
}

# ---- Linux libc selection (#812) --------------------------------------------
# A musl binary cannot resolve *.local (no glibc NSS), so prefer gnu when glibc
# is PROVEN and degrade to musl whenever it is not. These cases mirror
# update_classify_libc() in crates/maw-cli/src/core_impl/update_plan.rs — the
# installer and `maw update` must never disagree about which asset a host gets.

# shellcheck disable=SC2034
MAW_LIBC=gnu
assert_eq gnu "$(detect_linux_libc)" "MAW_LIBC=gnu forces gnu"
# shellcheck disable=SC2034
MAW_LIBC=glibc
assert_eq gnu "$(detect_linux_libc)" "MAW_LIBC=glibc forces gnu"
# shellcheck disable=SC2034
MAW_LIBC=musl
assert_eq musl "$(detect_linux_libc)" "MAW_LIBC=musl forces musl"
# shellcheck disable=SC2034
MAW_LIBC=

# glibc's ldd answers on stdout
# shellcheck disable=SC2329
ldd() {
  printf 'ldd (Ubuntu GLIBC 2.39-0ubuntu8.8) 2.39\n'
}
assert_eq gnu "$(detect_linux_libc)" "glibc ldd -> gnu"

# shellcheck disable=SC2329
ldd() {
  printf 'ldd (Debian GNU libc 2.36-9) 2.36\n'
}
assert_eq gnu "$(detect_linux_libc)" "Debian GNU libc ldd -> gnu"

# musl's ldd writes to stderr and exits nonzero — still proof, and it wins even
# when a glibc loader is also on disk (gcompat hosts)
# shellcheck disable=SC2329
ldd() {
  printf 'musl libc (x86_64)\nVersion 1.2.5\nDynamic Program Loader\n' >&2
  return 1
}
assert_eq musl "$(detect_linux_libc)" "musl ldd -> musl"

# unrecognizable ldd: fall through to the loader probe
# shellcheck disable=SC2329
ldd() {
  printf 'ldd: unrecognized option\n' >&2
  return 1
}
libc_probe_root=$(mktemp -d "${TMPDIR:-/tmp}/maw-install-libc.XXXXXX")
# shellcheck disable=SC2034
MAW_LIBC_LOADER_PATHS="$libc_probe_root/absent-libc.so.6"
assert_eq musl "$(detect_linux_libc)" "ambiguous ldd + no glibc loader -> musl (safe default)"
: >"$libc_probe_root/present-libc.so.6"
# shellcheck disable=SC2034
MAW_LIBC_LOADER_PATHS="$libc_probe_root/absent-libc.so.6 $libc_probe_root/present-libc.so.6"
assert_eq gnu "$(detect_linux_libc)" "ambiguous ldd + glibc loader on disk -> gnu"
rm -rf "$libc_probe_root"

# no ldd on the host at all: loader probe still decides, absence means musl
# shellcheck disable=SC2329
ldd() {
  return 127
}
# shellcheck disable=SC2034
MAW_LIBC_LOADER_PATHS="/nonexistent/libc.so.6"
assert_eq musl "$(detect_linux_libc)" "no ldd + no glibc loader -> musl"

# detect_platform must actually thread the libc into the asset name.
case "$(uname -s):$(uname -m)" in
  Linux:x86_64|Linux:amd64)
    # shellcheck disable=SC2034
    MAW_LIBC=gnu
    assert_eq maw-rs-linux-x86_64-gnu "$(detect_platform)" "detect_platform gnu asset"
    # shellcheck disable=SC2034
    MAW_LIBC=musl
    assert_eq maw-rs-linux-x86_64-musl "$(detect_platform)" "detect_platform musl asset"
    ;;
  Darwin:arm64|Darwin:aarch64)
    assert_eq maw-rs-macos-arm64 "$(detect_platform)" "detect_platform macOS asset"
    ;;
  *)
    printf 'skip: detect_platform asset check (no prebuilt target for this host)\n'
    ;;
esac

# ---- gnu -> musl runtime fallback (#812) ------------------------------------
# The gnu asset is dynamic, so a host with an older glibc than the build cannot
# run it. install.sh proves the download starts before installing it and only
# ever downgrades gnu -> musl; macOS and musl paths must stay untouched.
fallback_root=$(mktemp -d "${TMPDIR:-/tmp}/maw-install-fallback.XXXXXX")

# shellcheck disable=SC2329
download_and_verify() {
  asset=$2
  DOWNLOADED_BIN="$3/$2"
}

gnu_runs=1
musl_runs=1
# shellcheck disable=SC2329
binary_runs_here() {
  case "$1" in
    *-gnu) [ "$gnu_runs" = 1 ] ;;
    *) [ "$musl_runs" = 1 ] ;;
  esac
}

asset=maw-rs-linux-x86_64-musl
DOWNLOADED_BIN="$fallback_root/$asset"
fallback_if_unrunnable v26.7.23 "$fallback_root"
assert_eq maw-rs-linux-x86_64-musl "$asset" "musl asset is never probed or swapped"

asset=maw-rs-macos-arm64
DOWNLOADED_BIN="$fallback_root/$asset"
fallback_if_unrunnable v26.7.23 "$fallback_root"
assert_eq maw-rs-macos-arm64 "$asset" "macOS asset is never probed or swapped"

# shellcheck disable=SC2034
gnu_runs=1
asset=maw-rs-linux-x86_64-gnu
DOWNLOADED_BIN="$fallback_root/$asset"
fallback_if_unrunnable v26.7.23 "$fallback_root"
assert_eq maw-rs-linux-x86_64-gnu "$asset" "runnable gnu asset is kept"

# shellcheck disable=SC2034
gnu_runs=0
asset=maw-rs-linux-x86_64-gnu
DOWNLOADED_BIN="$fallback_root/$asset"
fallback_if_unrunnable v26.7.23 "$fallback_root" 2>/dev/null
assert_eq maw-rs-linux-x86_64-musl "$asset" "unrunnable gnu falls back to musl"
assert_eq "$fallback_root/maw-rs-linux-x86_64-musl" "$DOWNLOADED_BIN" \
  "fallback re-downloads the musl asset"

# shellcheck disable=SC2034
gnu_runs=0
# shellcheck disable=SC2034
musl_runs=0
asset=maw-rs-linux-x86_64-gnu
DOWNLOADED_BIN="$fallback_root/$asset"
if (fallback_if_unrunnable v26.7.23 "$fallback_root" >/dev/null 2>&1); then
  printf 'FAIL neither Linux asset runnable must be fatal\n' >&2
  exit 1
fi
rm -rf "$fallback_root"

# #812: a release that does not carry the gnu asset must degrade to musl rather
# than 404 and die. Every published release today is musl-only, and pinned or
# stable tags can never gain a gnu asset, so without this the installer breaks
# on every glibc host the moment the new detect_platform lands on alpha.
#
# The fallback must fire ONLY on a genuine 404. Downgrading on a 5xx or a
# rate-limit would silently hand a glibc host the musl build, which cannot
# resolve .local names — reintroducing #812 itself, invisibly.
missing_root=$(mktemp -d "${TMPDIR:-/tmp}/maw-install-missing.XXXXXX")

# The fallback tests above replace download_and_verify with a stub; re-source
# to get the real one back before exercising it here.
# shellcheck disable=SC1090
. "$ROOT/install.sh"

# shellcheck disable=SC2329
download_to() {
  case "$1" in
    *.sha256) printf 'c0ffee  asset\n' > "$2" ;;
    *) printf 'binary\n' > "$2" ;;
  esac
}
# shellcheck disable=SC2329
sha256_file() {
  printf 'c0ffee\n'
}

# shellcheck disable=SC2329
download_probe_to() {
  case "$1" in
    *-gnu) printf 'missing\n' ;;
    *) printf 'found\n' ;;
  esac
}
download_and_verify v26.7.16 maw-rs-linux-x86_64-gnu "$missing_root" >/dev/null 2>&1
assert_eq maw-rs-linux-x86_64-musl "$asset" "unpublished gnu asset falls back to musl"
assert_eq "$missing_root/maw-rs-linux-x86_64-musl" "$DOWNLOADED_BIN" \
  "missing-asset fallback re-downloads the musl asset"

# A server error is NOT absence: it must stay fatal rather than downgrade.
# shellcheck disable=SC2329
download_probe_to() {
  printf 'error\n'
}
if (download_and_verify v26.7.16 maw-rs-linux-x86_64-gnu "$missing_root" >/dev/null 2>&1); then
  printf 'FAIL server error must not silently fall back to musl\n' >&2
  exit 1
fi

# A missing musl asset is a real packaging failure, with nothing to degrade to.
# shellcheck disable=SC2329
download_probe_to() {
  printf 'missing\n'
}
if (download_and_verify v26.7.16 maw-rs-linux-x86_64-musl "$missing_root" >/dev/null 2>&1); then
  printf 'FAIL missing musl asset must be fatal\n' >&2
  exit 1
fi

# macOS has no sibling to degrade to either.
if (download_and_verify v26.7.16 maw-rs-macos-arm64 "$missing_root" >/dev/null 2>&1); then
  printf 'FAIL missing macOS asset must be fatal\n' >&2
  exit 1
fi

# A truncated or corrupted download arrives as a perfectly good HTTP 200, so
# nothing keyed on status can see it — the checksum is the only thing standing
# there. It must stay fatal and must NOT reach the gnu -> musl fallback: a
# corrupt gnu asset is a broken artifact, not an absent one, and degrading
# would hand a glibc host the musl build (no .local/mDNS, #812) while hiding a
# real packaging failure behind a silent downgrade.
# shellcheck disable=SC2329
download_probe_to() {
  printf 'found\n'
}
# shellcheck disable=SC2329
sha256_file() {
  printf 'truncated-does-not-match\n'
}
asset=maw-rs-linux-x86_64-gnu
if (download_and_verify v26.7.16 maw-rs-linux-x86_64-gnu "$missing_root" >/dev/null 2>&1); then
  printf 'FAIL a checksum mismatch must be fatal\n' >&2
  exit 1
fi
assert_eq maw-rs-linux-x86_64-gnu "$asset" "a corrupt gnu asset must not fall back to musl"
rm -rf "$missing_root"

printf 'install resolve_version + libc detection + fallback tests: ok\n'
