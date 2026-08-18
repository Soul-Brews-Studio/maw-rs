#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
project="maw-922-smoke-$$"
export COMPOSE_PROGRESS=plain
export MAW_IMAGE_TAG="v0.0.0-smoke-$project" MAW_BIND_IP=127.0.0.1
export MAW_PORT="${MAW_SMOKE_PORT:-13456}"
compose=(docker compose --project-name "$project")
cleanup() { "${compose[@]}" down --volumes --remove-orphans --rmi all >/dev/null 2>&1 || true; }
trap cleanup EXIT

! env -u MAW_SERVE_TOKEN "${compose[@]}" config >/dev/null 2>&1 || { echo "smoke: missing token accepted" >&2; exit 1; }
! MAW_SERVE_TOKEN= "${compose[@]}" config >/dev/null 2>&1 || { echo "smoke: empty token accepted" >&2; exit 1; }
export MAW_SERVE_TOKEN="compose-smoke-token-$RANDOM-$RANDOM"
"${compose[@]}" config >/dev/null
"${compose[@]}" build
"${compose[@]}" up --detach
cid="$("${compose[@]}" ps --quiet maw)"
[ "$(docker inspect --format '{{.HostConfig.Init}} {{.HostConfig.Privileged}} {{json .HostConfig.CapAdd}} {{json .HostConfig.CapDrop}} {{json .HostConfig.SecurityOpt}}' "$cid")" = 'true false null ["ALL"] ["no-new-privileges:true"]' ]
[ "$(docker inspect --format '{{range .Mounts}}{{.Type}} {{end}}' "$cid")" = 'volume volume volume volume volume ' ]
"${compose[@]}" exec -T maw sh -c '[ "$(id -u)" -ne 0 ] && command -v tmux git ssh curl ps lsof sqlite3 >/dev/null'

base="http://127.0.0.1:$MAW_PORT"
http=(curl --connect-timeout 2 --max-time 5)
for _ in {1..30}; do
  "${http[@]}" --fail --silent "$base/api/health" >/dev/null && break
  sleep 2
done
"${http[@]}" --fail --silent "$base/api/health" >/dev/null

route="$base/api/feed"
body="$("${http[@]}" --silent --write-out $'\n%{http_code}' "$route")"
code="${body##*$'\n'}"
[ "$code" = 401 ] || { echo "smoke: unauthenticated API returned $code" >&2; exit 1; }
grep -q '"auth":"maw-serve-token"' <<<"$body"
code="$("${http[@]}" --silent --output /dev/null --write-out '%{http_code}' \
  -H "Authorization: Bearer wrong" "$route")"
[ "$code" = 401 ] || { echo "smoke: wrong token returned $code" >&2; exit 1; }
"${http[@]}" --fail --silent -H "Authorization: Bearer $MAW_SERVE_TOKEN" \
  "$route" >/dev/null
"${compose[@]}" exec -T maw maw --version | grep -Eq '^maw-rs v'
paths='/home/maw/.config/maw /home/maw/.local/share/maw /home/maw/.local/state/maw /home/maw/.cache/maw /repos'
"${compose[@]}" exec -T maw sh -c "for d in $paths; do touch \"\$d/.maw-compose-smoke\"; done"
"${compose[@]}" up --detach --force-recreate --wait --wait-timeout 60
"${compose[@]}" exec -T maw sh -c "for d in $paths; do test -f \"\$d/.maw-compose-smoke\"; done"
"${compose[@]}" exec -T maw sh -c "for d in $paths; do rm \"\$d/.maw-compose-smoke\"; done"
echo "smoke: compose/auth/health/version/persistence passed"
