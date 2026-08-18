# Docker Compose quick-start

This runs a container-owned maw runtime for evaluation or headless use. It does
not integrate with host tmux and mounts neither host/Docker sockets.

```bash
export MAW_SERVE_TOKEN="$(openssl rand -hex 32)"
docker compose pull                 # GHCR `alpha` image
docker compose up -d                # pull alpha; build only when image is missing
docker compose ps
docker compose exec maw maw --version
```

The required token makes `docker compose config` fail before startup when it is
unset or empty. The API binds only to `127.0.0.1:3456` by default. Override with
`MAW_BIND_IP` and `MAW_PORT` only when the exposure is intentional.

```bash
curl http://127.0.0.1:3456/api/health
curl -H "Authorization: Bearer $MAW_SERVE_TOKEN" \
  http://127.0.0.1:3456/api/serve-core/pipeline
```

Treat the token like a password: keep it out of source control and shell
history. Protected `/api/*` routes reject requests without the Bearer token;
health remains unauthenticated for container monitoring.

Named volumes persist XDG config/data/state/cache and `/repos`. Inspect them
with `docker volume ls`; `docker compose down` preserves them, while
`docker compose down --volumes` deletes them. Select another published image
with `MAW_IMAGE_TAG`. For a deterministic local build, run
`docker compose build` followed by `docker compose up --build -d`; do not rely
on registry network or authentication failures falling back to a build.
Maintainers must verify the GHCR package is public after its first publish.

The container owns its tmux server and agent processes. It cannot see host tmux,
the Docker daemon, or host repositories unless content is copied/cloned into
`/repos`. `.local`/mDNS and host networking differ between native Linux and
Docker Desktop; do not assume LAN peers or host services are transparently
reachable from the container.

This containment does not fix #877's underlying identity/key model. Only the
documented serve and version path is covered; legacy `$HOME/.maw` writes are
not among the persisted XDG volumes.

Run the bounded Linux smoke check (it tears down its test volumes/image):

```bash
scripts/docker-compose-smoke.sh
```
