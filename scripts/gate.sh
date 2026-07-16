#!/usr/bin/env bash
# gate.sh — tiered local build gate for maw-rs.
#
#   gate.sh quick            fmt + clippy(stable) + tests for crates touched by
#                            the diff vs merge-base(origin/alpha). Iteration tier.
#   gate.sh full             the 4 CI dimensions (see docs/guides/gating.md).
#                            Mandatory before merge/promote.
#   gate.sh batch <br>...    merge-train: throwaway worktree at origin/alpha,
#                            merge the listed branches, run `full` ONCE.
#
# Warm start: seeds CARGO_TARGET_DIR from the golden cache
# (~/.maw/gate-cache/<alpha-sha>/target) via APFS clonefile (cp -Rc) when the
# target dir does not exist yet. Every invocation uses an ISOLATED target dir
# (default: <worktree>/target-gate) guarded by a lock — two concurrent test
# runs must never share a live target dir (one-gate-per-target-dir scar,
# 2026-07-14: phantom FAILEDs from the other tree).
#
# Env overrides:
#   GATE_TARGET_DIR   target dir (default <repo>/target-gate)
#   MAW_GATE_CACHE    golden cache root (default ~/.maw/gate-cache)
set -u -o pipefail

MODE="${1:-}"
[ $# -gt 0 ] && shift

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "gate.sh: not inside a git worktree" >&2
    exit 2
}
CACHE_ROOT="${MAW_GATE_CACHE:-$HOME/.maw/gate-cache}"
TARGET_DIR="${GATE_TARGET_DIR:-$REPO_ROOT/target-gate}"
export CARGO_TARGET_DIR="$TARGET_DIR"

STEP_NAMES=()
STEP_SECS=()
STEP_RESULTS=()

summary() {
    echo
    echo "== gate summary ($MODE) =="
    printf '%-52s %8s %s\n' step seconds result
    local i
    for i in "${!STEP_NAMES[@]}"; do
        printf '%-52s %8s %s\n' "${STEP_NAMES[$i]}" "${STEP_SECS[$i]}" "${STEP_RESULTS[$i]}"
    done
}

run_step() {
    local name="$1"
    shift
    echo
    echo "==> $name"
    echo "    $*"
    local t0=$SECONDS
    if "$@"; then
        STEP_NAMES+=("$name"); STEP_SECS+=("$((SECONDS - t0))"); STEP_RESULTS+=("ok")
    else
        STEP_NAMES+=("$name"); STEP_SECS+=("$((SECONDS - t0))"); STEP_RESULTS+=("FAIL")
        summary
        echo "gate.sh: FAILED at: $name" >&2
        release_lock
        exit 1
    fi
}

# ---- one-gate-per-target-dir lock ------------------------------------------
LOCK_DIR="$TARGET_DIR.lock"
HAVE_LOCK=0

acquire_lock() {
    if mkdir "$LOCK_DIR" 2>/dev/null; then
        echo $$ >"$LOCK_DIR/pid"
        HAVE_LOCK=1
        return 0
    fi
    local pid
    pid="$(cat "$LOCK_DIR/pid" 2>/dev/null || true)"
    # Fail-safe: an empty/unreadable pid means a gate may be mid-acquire (or
    # crashed before writing) — treat as LIVE; never risk a double-acquire.
    if [ -z "$pid" ] || kill -0 "$pid" 2>/dev/null; then
        echo "gate.sh: another gate (pid ${pid:-unknown}) is live on $TARGET_DIR" >&2
        echo "  one-gate-per-target-dir: concurrent test runs on a shared target dir" >&2
        echo "  cross-contaminate. Wait, or rerun with GATE_TARGET_DIR=<other-dir>." >&2
        echo "  (certain it's dead? mv '$LOCK_DIR' aside and rerun)" >&2
        exit 3
    fi
    # Atomic takeover: rename the stale lock dir (exactly one contender wins
    # the mv), then take a fresh mkdir — never rm a lock another gate may hold.
    if ! mv "$LOCK_DIR" "$LOCK_DIR.stale.$$" 2>/dev/null; then
        echo "gate.sh: lost the stale-lock takeover race on $TARGET_DIR — try again" >&2
        exit 3
    fi
    rm -f "$LOCK_DIR.stale.$$/pid" && rmdir "$LOCK_DIR.stale.$$" 2>/dev/null || true
    mkdir "$LOCK_DIR" 2>/dev/null || {
        echo "gate.sh: lost the lock re-acquire race on $TARGET_DIR — try again" >&2
        exit 3
    }
    echo $$ >"$LOCK_DIR/pid"
    HAVE_LOCK=1
    echo "gate.sh: took over stale lock (pid $pid gone)"
}

release_lock() {
    if [ "$HAVE_LOCK" = 1 ]; then
        rm -f "$LOCK_DIR/pid"
        rmdir "$LOCK_DIR" 2>/dev/null || true
        HAVE_LOCK=0
    fi
}
trap release_lock EXIT

# ---- golden-cache warm start -------------------------------------------------
warm_seed() {
    if [ -d "$TARGET_DIR" ]; then
        echo "warm-start: target dir exists ($TARGET_DIR) — incremental reuse, no seed"
        return
    fi
    local base golden=""
    base="$(git -C "$REPO_ROOT" merge-base HEAD origin/alpha 2>/dev/null || git -C "$REPO_ROOT" rev-parse HEAD)"
    if [ -d "$CACHE_ROOT/$base/target" ]; then
        golden="$CACHE_ROOT/$base/target"
    else
        # newest complete cache (never a .partial mid-refresh dir)
        golden="$(ls -td "$CACHE_ROOT"/*/target 2>/dev/null | grep -v '\.partial/' | head -1 || true)"
    fi
    if [ -z "$golden" ]; then
        echo "warm-start: no golden cache under $CACHE_ROOT — COLD build"
        echo "  (seed one with scripts/gate-cache-refresh.sh)"
        return
    fi
    local t0=$SECONDS
    if cp -Rc "$golden" "$TARGET_DIR" 2>/dev/null; then
        echo "warm-start: cloned $golden -> $TARGET_DIR in $((SECONDS - t0))s (APFS clonefile)"
    else
        # clonefile unavailable (non-APFS / cross-volume): do not fall back to a
        # slow full copy; a partial clone must not be reused.
        [ -e "$TARGET_DIR" ] && mv "$TARGET_DIR" "${TMPDIR:-/tmp}/gate-partial-clone-$$"
        echo "warm-start: clonefile failed (cross-volume or non-APFS) — COLD build"
    fi
}

# ---- affected-crate mapping (quick tier) -------------------------------------
affected_crates() {
    local base
    base="$(git -C "$REPO_ROOT" merge-base HEAD origin/alpha 2>/dev/null || git -C "$REPO_ROOT" rev-parse HEAD)"
    {
        git -C "$REPO_ROOT" diff --name-only "$base"
        git -C "$REPO_ROOT" ls-files --others --exclude-standard
    } | sort -u
}

quick_tests() {
    local f crates="" wide=0
    while IFS= read -r f; do
        [ -n "$f" ] || continue
        case "$f" in
            crates/*/*) c="${f#crates/}"; crates="$crates ${c%%/*}" ;;
            Cargo.toml | Cargo.lock) wide=1 ;;
            *) ;; # scripts/docs/ci-only changes need no crate tests
        esac
    done <<<"$(affected_crates)"
    if [ "$wide" = 1 ]; then
        echo "quick: root Cargo.toml/Cargo.lock changed -> full workspace tests"
        run_step "test (workspace: manifest changed)" cargo test --workspace --locked
        return
    fi
    crates="$(echo "$crates" | tr ' ' '\n' | sort -u | tr '\n' ' ')"
    if [ -z "${crates// /}" ]; then
        echo "quick: no crate source changed vs merge-base(origin/alpha) — skipping tests"
        return
    fi
    local pkgs=()
    for c in $crates; do pkgs+=(-p "$c"); done
    run_step "test (affected:$crates)" cargo test "${pkgs[@]}"
}

# ---- tiers -------------------------------------------------------------------
gate_quick() {
    acquire_lock
    warm_seed
    run_step "fmt --check" cargo fmt --all -- --check
    run_step "clippy (stable)" cargo clippy --workspace --all-targets -- -D warnings
    quick_tests
    summary
    echo
    echo "quick tier only — run 'gate.sh full' (or ride a merge-train batch) before merge."
}

gate_full() {
    acquire_lock
    warm_seed
    run_step "fmt --check" cargo fmt --all -- --check
    run_step "test --workspace" cargo test --workspace --locked
    run_step "clippy (stable)" cargo clippy --workspace --all-targets -- -D warnings
    if rustup toolchain list 2>/dev/null | grep -q '^1\.97\.0'; then
        run_step "clippy (+1.97.0 = CI's current stable)" cargo +1.97.0 clippy --workspace --all-targets -- -D warnings
    elif [ "${GATE_ALLOW_MISSING_197:-0}" = 1 ]; then
        echo
        echo "WARNING: toolchain 1.97.0 absent — clippy dimension SKIPPED (GATE_ALLOW_MISSING_197=1)."
        STEP_NAMES+=("clippy (+1.97.0)"); STEP_SECS+=(0); STEP_RESULTS+=("SKIPPED (allowed by env)")
    else
        # A full gate that silently skips a CI dimension is a false green a
        # lead will promote on — missing toolchain is a FAILURE by default.
        STEP_NAMES+=("clippy (+1.97.0)"); STEP_SECS+=(0); STEP_RESULTS+=("MISSING TOOLCHAIN")
        summary
        echo "gate.sh: toolchain 1.97.0 (CI's current stable) is not installed —" >&2
        echo "  full cannot certify all 4 CI dimensions." >&2
        echo "  Fix:    rustup toolchain install 1.97.0 --component clippy" >&2
        echo "  Or:     GATE_ALLOW_MISSING_197=1 gate.sh full   (accept the gap explicitly)" >&2
        exit 4
    fi
    run_step "test (wasm-host subset)" \
        cargo test -p maw-cli -p maw-plugin-manifest --features wasm-host --locked
    run_step "clippy (wasm-host subset)" \
        cargo clippy -p maw-cli -p maw-plugin-manifest --all-targets --features wasm-host --locked -- -D warnings
    summary
}

gate_batch() {
    [ $# -ge 1 ] || {
        echo "usage: gate.sh batch <branch> [<branch>...]" >&2
        exit 2
    }
    git -C "$REPO_ROOT" fetch origin alpha
    local train
    train="$REPO_ROOT/.claude/worktrees/gate-train-$(date +%y%m%d-%H%M%S)-$$"
    echo "merge-train: worktree $train at origin/alpha + $*"
    git -C "$REPO_ROOT" worktree add --detach "$train" origin/alpha || exit 1
    local br ref
    for br in "$@"; do
        ref="$br"
        git -C "$train" rev-parse --verify --quiet "$br^{commit}" >/dev/null 2>&1 ||
            ref="origin/$br"
        # rerere disabled: a user's recorded conflict resolutions must never
        # silently auto-replay into a train that then gets gated as "clean".
        if ! git -C "$train" -c rerere.enabled=false merge --no-ff --no-edit "$ref"; then
            echo "merge-train: '$br' does not merge cleanly onto the train — rebase it and retry." >&2
            echo "  inspect: $train (worktree kept)" >&2
            exit 1
        fi
    done
    if (cd "$train" && GATE_TARGET_DIR="$train/target-gate" "$train/scripts/gate.sh" full); then
        echo "merge-train: GREEN — the $# branch(es) are safe to land on alpha as a batch."
        mv "$train/target-gate" "${TMPDIR:-/tmp}/gate-train-target-$$" 2>/dev/null || true
        git -C "$REPO_ROOT" worktree remove --force "$train"
    else
        echo "merge-train: RED — bisect by re-running with fewer branches. Worktree kept: $train" >&2
        exit 1
    fi
}

case "$MODE" in
    quick) gate_quick ;;
    full) gate_full ;;
    batch) gate_batch "$@" ;;
    *)
        sed -n '2,20p' "$0"
        exit 2
        ;;
esac
