#!/usr/bin/env bash
# gate-cache-refresh.sh — (re)build the golden warm cache for scripts/gate.sh.
#
# Builds the workspace test + clippy artifacts AND the wasm-host subset at
# the current origin/alpha tip into ~/.maw/gate-cache/<sha>/target, from a
# throwaway git worktree so the caller's checkout is untouched. gate.sh warm-starts by APFS-
# cloning (cp -Rc) this dir — the cache itself is NEVER used as a live target
# dir, so it cannot be cross-contaminated by concurrent gates.
#
# Retention: keeps the 2 newest <sha> dirs; older ones are mv'd to
# ${TMPDIR:-/tmp}/gate-cache-trash-<ts> for the OS/user to purge (no rm — the
# fleet no-rm hook; and a mis-typed rm on a GB-scale cache root is the July
# disk-crisis failure mode).
#
# Schedule (no .maw/schedule.toml exists in-repo, so run it by hand or via
# maw schedule / cron), e.g. hourly:
#   0 * * * * /opt/Code/github.com/Soul-Brews-Studio/maw-rs/scripts/gate-cache-refresh.sh
#
# Env: MAW_GATE_CACHE overrides the cache root (default ~/.maw/gate-cache).
# Args: --seed <target-dir>  bootstrap the build from an existing target dir
#       (first-ever refresh); otherwise it auto-seeds from the newest cached
#       sha, so a routine refresh only recompiles what the alpha tip changed.
set -euo pipefail

REPO_ROOT="$(git rev-parse --show-toplevel)"
CACHE_ROOT="${MAW_GATE_CACHE:-$HOME/.maw/gate-cache}"
mkdir -p "$CACHE_ROOT"

# ---- whole-script lock: two overlapping refreshes would build into the SAME
# $SHA.partial live target dir then race the final mv — golden corruption.
REFRESH_LOCK="$CACHE_ROOT/.refresh.lock"
HAVE_LOCK=0
release_lock() {
    if [ "$HAVE_LOCK" = 1 ]; then
        rm -f "$REFRESH_LOCK/pid"
        rmdir "$REFRESH_LOCK" 2>/dev/null || true
        HAVE_LOCK=0
    fi
}
if ! mkdir "$REFRESH_LOCK" 2>/dev/null; then
    LOCK_PID="$(cat "$REFRESH_LOCK/pid" 2>/dev/null || true)"
    # Fail-safe: empty/unreadable pid = possibly mid-acquire -> treat as LIVE.
    if [ -z "$LOCK_PID" ] || kill -0 "$LOCK_PID" 2>/dev/null; then
        echo "gate-cache: another refresh (pid ${LOCK_PID:-unknown}) is live — exiting" >&2
        exit 3
    fi
    # Atomic takeover: rename wins exactly once; never rm a live lock.
    mv "$REFRESH_LOCK" "$REFRESH_LOCK.stale.$$" 2>/dev/null || {
        echo "gate-cache: lost stale-lock takeover race — exiting" >&2
        exit 3
    }
    rm -f "$REFRESH_LOCK.stale.$$/pid" && rmdir "$REFRESH_LOCK.stale.$$" 2>/dev/null || true
    mkdir "$REFRESH_LOCK" 2>/dev/null || {
        echo "gate-cache: lost lock re-acquire race — exiting" >&2
        exit 3
    }
    echo "gate-cache: took over stale lock (pid $LOCK_PID gone)"
fi
echo $$ >"$REFRESH_LOCK/pid"
HAVE_LOCK=1
trap release_lock EXIT

SEED=""
if [ "${1:-}" = "--seed" ]; then
    SEED="${2:?--seed needs a target dir}"
fi

git -C "$REPO_ROOT" fetch origin alpha
SHA="$(git -C "$REPO_ROOT" rev-parse origin/alpha)"

if [ -d "$CACHE_ROOT/$SHA/target" ]; then
    echo "gate-cache: $SHA already cached — nothing to do"
    exit 0
fi

BUILD_WT="$REPO_ROOT/.claude/worktrees/gate-cache-build-$$"
PARTIAL="$CACHE_ROOT/$SHA.partial"
cleanup() {
    git -C "$REPO_ROOT" worktree remove --force "$BUILD_WT" 2>/dev/null || true
    [ -d "$PARTIAL" ] && mv "$PARTIAL" "${TMPDIR:-/tmp}/gate-cache-partial-$$" 2>/dev/null || true
    release_lock
}
trap cleanup EXIT

echo "gate-cache: building golden target for origin/alpha @ ${SHA:0:12}"
git -C "$REPO_ROOT" worktree add --detach "$BUILD_WT" "$SHA"

# Seed the build (clonefile) so a routine refresh is incremental, not a
# 15-25 min cold dep build: explicit --seed dir, else the newest cached sha.
# never seed from a .partial: that can be a LIVE in-flight refresh build
[ -n "$SEED" ] || SEED="$(ls -td "$CACHE_ROOT"/*/target 2>/dev/null | grep -v '\.partial/' | head -1 || true)"
mkdir -p "$PARTIAL"
if [ -n "$SEED" ] && [ -d "$SEED" ]; then
    if cp -Rc "$SEED" "$PARTIAL/target" 2>/dev/null; then
        echo "gate-cache: seeded from $SEED (clonefile)"
        # drop per-path incremental state from the seed: useless to clone
        # consumers, and it dominates file count (= clonefile time)
        [ -d "$PARTIAL/target/debug/incremental" ] &&
            mv "$PARTIAL/target/debug/incremental" "${TMPDIR:-/tmp}/gate-cache-incr-$$" || true
    else
        mv "$PARTIAL" "${TMPDIR:-/tmp}/gate-cache-partial-seed-$$" 2>/dev/null || true
        mkdir -p "$PARTIAL"
        echo "gate-cache: clonefile seed failed — cold build"
    fi
fi

T0=$SECONDS
# CARGO_INCREMENTAL=0: incremental state is per-worktree-path and useless to
# clone consumers (workspace crates rebuild anyway); dropping it shrinks the
# golden dir and the clonefile time (clone cost scales with file count).
# The wasm-host/extism tier (~145 extra crates, the measured 27-46 min gate
# worst case) IS included: full gates are the slow pre-merge bar and must
# warm-start it too. Disk story: dedicated root, keep 2 newest, see header.
export CARGO_INCREMENTAL=0
CARGO_TARGET_DIR="$PARTIAL/target" \
    cargo build --manifest-path "$BUILD_WT/Cargo.toml" --workspace --tests --locked
# no `-D warnings` here: this is a cache builder, not a gate — a lint on the
# alpha tip must not block dep-artifact caching (gate.sh full still gates).
CARGO_TARGET_DIR="$PARTIAL/target" \
    cargo clippy --manifest-path "$BUILD_WT/Cargo.toml" --workspace --all-targets
CARGO_TARGET_DIR="$PARTIAL/target" \
    cargo build --manifest-path "$BUILD_WT/Cargo.toml" -p maw-cli -p maw-plugin-manifest --tests --features wasm-host --locked
# 1.97.0 (CI's current stable clippy) artifacts are keyed per compiler and coexist in
# the same target dir; pre-building them here (idle/scheduled time) saves
# every consumer full gate a cold whole-workspace 1.97 check.
if rustup toolchain list 2>/dev/null | grep -q '^1\.97\.0'; then
    CARGO_TARGET_DIR="$PARTIAL/target" \
        cargo +1.97.0 clippy --manifest-path "$BUILD_WT/Cargo.toml" --workspace --all-targets
fi
mv "$PARTIAL" "$CACHE_ROOT/$SHA"
echo "gate-cache: built in $((SECONDS - T0))s -> $CACHE_ROOT/$SHA ($(du -sh "$CACHE_ROOT/$SHA" | cut -f1))"

# Retention: keep the 2 newest, mv the rest out for deletion.
TRASH="${TMPDIR:-/tmp}/gate-cache-trash-$(date +%y%m%d-%H%M%S)"
# never retire a .partial: that can be a LIVE build mid-cargo
STALE="$(ls -td "$CACHE_ROOT"/*/ 2>/dev/null | grep -v '\.partial/' | tail -n +3 || true)"
if [ -n "$STALE" ]; then
    mkdir -p "$TRASH"
    echo "$STALE" | while IFS= read -r d; do
        [ -n "$d" ] && mv "$d" "$TRASH/" && echo "gate-cache: retired $d -> $TRASH/"
    done
fi
echo "gate-cache: done. cached shas:"
ls -td "$CACHE_ROOT"/*/ 2>/dev/null || true
