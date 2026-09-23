#!/bin/sh
set -eu
case "${MAW_SERVE_TOKEN-}" in
  *[![:space:]]*) ;;
  *) echo "maw: MAW_SERVE_TOKEN must contain a non-whitespace character" >&2; exit 64 ;;
esac
# host-tmux profile only (#943): the operator passes the host's own `tmux -V`
# output so a version mismatch with the container's tmux is caught here,
# before the socket is ever touched, rather than surfacing later as an opaque
# "server exited unexpectedly" from a live host pane.
case "${MAW_HOST_TMUX_VERSION-}" in
  '') ;;
  *)
    container_tmux_version="$(tmux -V)"
    if [ "$container_tmux_version" != "$MAW_HOST_TMUX_VERSION" ]; then
      echo "maw: host-tmux profile: container tmux ($container_tmux_version) does not match the host (MAW_HOST_TMUX_VERSION=$MAW_HOST_TMUX_VERSION); refusing to start" >&2
      exit 65
    fi
    ;;
esac
exec /usr/local/bin/maw "$@"
