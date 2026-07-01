#!/usr/bin/env bash
# Deploy discord-backfill (maw-discord-backfill) on oracle-world / sila user.
# Bo GO 2026-07-01 · maclab:gmgrok
set -euo pipefail

BRANCH="${BRANCH:-feat/discord-backfill-rs}"
REPO_URL="${REPO_URL:-https://github.com/MEYD-605/maw-rs.git}"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
WORK="${WORK:-$HOME/Code/github.com/MEYD-605/maw-rs}"

echo "== discord-backfill sila deploy =="
echo "branch=$BRANCH install=$INSTALL_DIR"

mkdir -p "$(dirname "$WORK")"
if [ -d "$WORK/.git" ]; then
  git -C "$WORK" fetch origin "$BRANCH"
  git -C "$WORK" checkout "$BRANCH"
  git -C "$WORK" pull --ff-only origin "$BRANCH" || true
else
  git clone --branch "$BRANCH" --depth 1 "$REPO_URL" "$WORK"
fi

cd "$WORK"
cargo build -p maw-discord-backfill --release
mkdir -p "$INSTALL_DIR"
install -m755 target/release/discord-backfill "$INSTALL_DIR/discord-backfill"

echo "== smoke =="
"$INSTALL_DIR/discord-backfill" whoami
echo "OK: $(command -v discord-backfill) ($(discord-backfill --help 2>&1 | head -1 || true))"