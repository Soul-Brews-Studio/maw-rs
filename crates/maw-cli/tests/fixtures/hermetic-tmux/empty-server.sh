#!/bin/sh
case "$1" in
  list-sessions|list-windows|list-panes) exit 0 ;;
  *) printf 'unexpected fake tmux command: %s\n' "$*" >&2; exit 64 ;;
esac
