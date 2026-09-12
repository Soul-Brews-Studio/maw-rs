#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/.."
project="maw-922-smoke-$$"
export COMPOSE_PROGRESS=plain
export MAW_IMAGE_TAG="v0.0.0-smoke-$project" MAW_BIND_IP=127.0.0.1
export MAW_PORT="${MAW_SMOKE_PORT:-0}"
compose=(docker compose --project-name "$project")
image="ghcr.io/soul-brews-studio/maw-rs:$MAW_IMAGE_TAG"
cleanup() {
  rc=$?; trap - EXIT; set +e
  timeout 60s "${compose[@]}" down --timeout 10 --volumes --remove-orphans --rmi all; [ $? -eq 0 ] || rc=1
  containers="$(timeout 10s docker ps -aq --filter "label=com.docker.compose.project=$project")"; [ $? -eq 0 ] && [ -z "$containers" ] || rc=1
  volumes="$(timeout 10s docker volume ls -q --filter "label=com.docker.compose.project=$project")"; [ $? -eq 0 ] && [ -z "$volumes" ] || rc=1
  images="$(timeout 10s docker image ls -q --filter "reference=$image")"; [ $? -eq 0 ] && [ -z "$images" ] || rc=1
  exit "$rc"
}
trap cleanup EXIT

! env -u MAW_SERVE_TOKEN "${compose[@]}" config >/dev/null 2>&1 || { echo "smoke: missing token accepted" >&2; exit 1; }
! MAW_SERVE_TOKEN= "${compose[@]}" config >/dev/null 2>&1 || { echo "smoke: empty token accepted" >&2; exit 1; }
export MAW_SERVE_TOKEN="compose-smoke-token-$RANDOM-$RANDOM"
"${compose[@]}" config >/dev/null
timeout 1200s "${compose[@]}" build
! MAW_SERVE_TOKEN='   ' timeout 30s "${compose[@]}" run --rm maw --version >/dev/null 2>&1 || { echo "smoke: whitespace token started" >&2; exit 1; }
timeout 120s "${compose[@]}" up --detach
cid="$(timeout 10s "${compose[@]}" ps --quiet maw)"
[ "$(timeout 10s docker inspect --format '{{.HostConfig.Init}} {{.HostConfig.Privileged}} {{json .HostConfig.CapAdd}} {{json .HostConfig.CapDrop}} {{json .HostConfig.SecurityOpt}}' "$cid")" = 'true false null ["ALL"] ["no-new-privileges:true"]' ]
[ "$(timeout 10s docker inspect --format '{{range .Mounts}}{{.Type}} {{end}}' "$cid")" = 'volume volume volume volume volume ' ]
timeout 30s "${compose[@]}" exec -T maw sh -c '[ "$(id -u)" -ne 0 ] && command -v tmux git ssh curl ps lsof sqlite3 >/dev/null'
if ! container_token="$(timeout 30s "${compose[@]}" exec -T maw sh -c 'printf %s "${MAW_SERVE_TOKEN-}"')"; then
  echo "smoke: could not read container token" >&2; exit 1
fi
[ -n "$container_token" ] || { echo "smoke: container token is empty" >&2; exit 1; }
[ "$container_token" = "$MAW_SERVE_TOKEN" ] || { echo "smoke: container token mismatch" >&2; exit 1; }
if ! binding="$(timeout 10s docker inspect --format '{{(index (index .NetworkSettings.Ports "3456/tcp") 0).HostIp}} {{(index (index .NetworkSettings.Ports "3456/tcp") 0).HostPort}}' "$cid")"; then
  echo "smoke: could not inspect the container port" >&2; exit 1
fi
read -r published_ip published_port <<<"$binding"
[ "$published_ip" = 127.0.0.1 ] || { echo "smoke: container port is not loopback-only" >&2; exit 1; }
[[ "$published_port" =~ ^[0-9]+$ ]] && [ "$published_port" -gt 0 ] && [ "$published_port" -le 65535 ] || { echo "smoke: invalid published port" >&2; exit 1; }

base="http://127.0.0.1:$published_port"
http=(curl --connect-timeout 2 --max-time 5)
for _ in {1..30}; do
  "${http[@]}" --fail --silent "$base/api/health" >/dev/null && break
  sleep 2
done
health="$("${http[@]}" --fail --silent "$base/api/health")"
grep -q '"ok":true' <<<"$health" || { echo "smoke: health is not ok" >&2; exit 1; }
grep -q '"source":"maw-rs"' <<<"$health" || { echo "smoke: health source mismatch" >&2; exit 1; }
grep -q '"port":3456' <<<"$health" || { echo "smoke: health port mismatch" >&2; exit 1; }

route="$base/api/serve-core/pipeline"
body="$("${http[@]}" --silent --write-out $'\n%{http_code}' "$route")"
code="${body##*$'\n'}"
[ "$code" = 401 ] || { echo "smoke: unauthenticated API returned $code" >&2; exit 1; }
grep -q '"auth":"maw-serve-token"' <<<"$body"
code="$("${http[@]}" --silent --output /dev/null --write-out '%{http_code}' \
  -H "Authorization: Bearer wrong" "$route")"
[ "$code" = 401 ] || { echo "smoke: wrong token returned $code" >&2; exit 1; }
body="$("${http[@]}" --silent --write-out $'\n%{http_code}' \
  -H "Authorization: Bearer $container_token" "$route")"
code="${body##*$'\n'}"
[ "$code" = 200 ] || { echo "smoke: container token returned $code" >&2; exit 1; }
grep -q '"pipeline":' <<<"$body" || { echo "smoke: pipeline response missing" >&2; exit 1; }
timeout 30s "${compose[@]}" exec -T maw maw --version | grep -Eq '^maw-rs v'
paths='/home/maw/.config/maw /home/maw/.local/share/maw /home/maw/.local/state/maw /home/maw/.cache/maw /repos'
timeout 30s "${compose[@]}" exec -T maw sh -c "for d in $paths; do touch \"\$d/.maw-compose-smoke\"; done"
timeout 120s "${compose[@]}" up --detach --force-recreate --wait --wait-timeout 60
timeout 30s "${compose[@]}" exec -T maw sh -c "for d in $paths; do test -f \"\$d/.maw-compose-smoke\"; done"
timeout 30s "${compose[@]}" exec -T maw sh -c "for d in $paths; do rm \"\$d/.maw-compose-smoke\"; done"
echo "smoke: checks passed; cleaning up"
