#!/bin/sh
set -eu
case "${MAW_SERVE_TOKEN-}" in
  *[![:space:]]*) ;;
  *) echo "maw: MAW_SERVE_TOKEN must contain a non-whitespace character" >&2; exit 64 ;;
esac
exec /usr/local/bin/maw "$@"
