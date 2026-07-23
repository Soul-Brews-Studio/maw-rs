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

printf 'install resolve_version tests: ok\n'
