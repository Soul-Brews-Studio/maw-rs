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
# Toolchain: every tier preflights that the ACTIVE rustc is the one
# rust-toolchain.toml pins and that the wasm32-unknown-unknown rust-std is
# installed, and refuses before running any cargo step if either is off (#823).
# The gate's verdict must not depend on whose machine ran it.
#
# Env overrides:
#   GATE_TARGET_DIR              target dir (default <repo>/target-gate)
#   MAW_GATE_CACHE               golden cache root (default ~/.maw/gate-cache)
#   GATE_ALLOW_TOOLCHAIN_DRIFT=1 skip the toolchain preflight and accept that
#                                this run does not reproduce CI's toolchain
#
# Every `cargo test` below passes --no-fail-fast on purpose (#796): without it
# cargo stops at the FIRST failing test target, so one red target hides every
# other one and the gate under-reports the true failure set — a fixer then
# round-trips through failures the first run never printed. --no-fail-fast
# changes only WHAT RUNS, not the exit code: cargo still exits non-zero when
# any test failed, so run_step still trips and gate.sh still exits 1.
set -u -o pipefail

MODE="${1:-}"
[ $# -gt 0 ] && shift

REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null)" || {
    echo "gate.sh: not inside a git worktree" >&2
    exit 2
}
CACHE_ROOT="${MAW_GATE_CACHE:-$HOME/.maw/gate-cache}"
# Default OFF the root filesystem. gate.sh passes --target-dir explicitly, which
# overrides ~/.cargo/config.toml's build.target-dir — so the global NVMe setting
# does NOT reach it, and this dir grew to 69G on / and filled the disk
# (2026-08-16). /mnt/nvme1 has 2.3T free; / holds every oracle's transcripts and
# logs, so a build filling it takes the whole box down, not just this repo.
GATE_DEFAULT_TARGET="/mnt/nvme1/cargo/target-gate"
[ -d /mnt/nvme1 ] || GATE_DEFAULT_TARGET="$REPO_ROOT/target-gate"
TARGET_DIR="${GATE_TARGET_DIR:-$GATE_DEFAULT_TARGET}"
export CARGO_TARGET_DIR="$TARGET_DIR"
TOOLCHAIN_FILE="$REPO_ROOT/rust-toolchain.toml"
WASM_TARGET="wasm32-unknown-unknown"
PINNED_CHANNEL=""

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

# ---- toolchain preflight (#823) ---------------------------------------------
# Two machine-dependent verdicts this gate used to ship silently:
#   1. rustc version. On alpha @ 5a30acab, 1.94.0 reported 2
#      clippy::redundant_iter_cloned errors in federation_routes.rs (:294,
#      :407) that 1.97.1 did not. Both called themselves "stable".
#   2. the wasm32-unknown-unknown rust-std. `maw plugin build` shells out to
#      `cargo build --release --target wasm32-unknown-unknown`, which exits 101
#      when that rust-std is absent, so
#      plugin_build_route_a_builds_dist_and_extism_loads_fixture fails
#      deterministically — far into `cargo test --workspace`, as a subprocess
#      failure that reads like a code bug rather than a missing target.
# Both are cheap to detect up front, so detect them up front.
#
# Covered by scripts/test-gate-preflight.sh (standalone; run it after touching
# anything below or after bumping the pin). It deliberately is NOT wired into
# gate.sh — the test drives gate.sh, so calling it from here would recurse.
pinned_channel() {
    sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
        "$TOOLCHAIN_FILE" 2>/dev/null | head -1
}

preflight() {
    PINNED_CHANNEL="$(pinned_channel)"
    # A missing pin is a REPO bug, not a machine state — no escape hatch.
    if [ -z "$PINNED_CHANNEL" ]; then
        echo "gate.sh: no [toolchain] channel pinned in $TOOLCHAIN_FILE" >&2
        echo "  Without a pin, 'stable' means a different rustc on every box and" >&2
        echo "  the gate's verdict depends on who ran it (#823)." >&2
        exit 4
    fi

    if [ "${GATE_ALLOW_TOOLCHAIN_DRIFT:-0}" = 1 ]; then
        echo
        echo "WARNING: toolchain preflight skipped (GATE_ALLOW_TOOLCHAIN_DRIFT=1)." >&2
        echo "  This run does NOT reproduce CI's toolchain; a green here certifies" >&2
        echo "  only this machine." >&2
        return
    fi

    # Accept EITHER form, deliberately as a union so this can only refuse less:
    #   - the running rustc's version equals the pin (this repo pins an exact
    #     version, so that is the normal path), or
    #   - rustup reports the pinned name as the active toolchain (covers a
    #     future `nightly-YYYY-MM-DD` / `beta` pin, whose rustc --version never
    #     spells the channel name).
    # A guard that refuses a machine which IS reproducing CI would be a bug.
    local active active_tc
    active="$(rustc --version 2>/dev/null | awk '{print $2}')"
    active_tc="$(rustup show active-toolchain 2>/dev/null | awk '{print $1}')"
    case "$active_tc" in
        "$PINNED_CHANNEL" | "$PINNED_CHANNEL"-*) active="$PINNED_CHANNEL" ;;
        *) ;;
    esac
    if [ "$active" != "$PINNED_CHANNEL" ]; then
        echo "gate.sh: active rustc is ${active:-none}, but $TOOLCHAIN_FILE pins $PINNED_CHANNEL" >&2
        echo "  Different rustc, different lints: 1.94.0 and 1.97.1 disagree on" >&2
        echo "  clippy::redundant_iter_cloned on this very tree (#823)." >&2
        echo "  Fix:    rustup toolchain install   (no args: installs the pin)" >&2
        echo "  Check:  rustup show active-toolchain   (a rustup override or" >&2
        echo "          RUSTUP_TOOLCHAIN in your env beats rust-toolchain.toml)" >&2
        echo "  Or:     GATE_ALLOW_TOOLCHAIN_DRIFT=1 gate.sh $MODE   (accept the gap)" >&2
        exit 4
    fi

    if ! rustup target list --installed 2>/dev/null | grep -qx "$WASM_TARGET"; then
        echo "gate.sh: rust-std for $WASM_TARGET is not installed for rustc $active" >&2
        echo "  'maw plugin build' cross-compiles to it, so cargo test --workspace" >&2
        echo "  fails deep inside plugin_build_route_a_builds_dist_and_extism_loads_fixture" >&2
        echo "  without it. $TOOLCHAIN_FILE lists it under 'targets', so this" >&2
        echo "  usually means rustup could not reach the network to fetch it." >&2
        echo "  Fix:    rustup target add $WASM_TARGET" >&2
        echo "          (or: rustup toolchain install  — installs channel + targets)" >&2
        echo "  Or:     GATE_ALLOW_TOOLCHAIN_DRIFT=1 gate.sh $MODE   (accept the gap)" >&2
        exit 4
    fi

    echo "preflight: rustc $active (pinned), $WASM_TARGET installed"
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
        run_step "test (workspace: manifest changed)" cargo test --workspace --locked --no-fail-fast
        return
    fi
    crates="$(echo "$crates" | tr ' ' '\n' | sort -u | tr '\n' ' ')"
    if [ -z "${crates// /}" ]; then
        echo "quick: no crate source changed vs merge-base(origin/alpha) — skipping tests"
        return
    fi
    local pkgs=()
    for c in $crates; do pkgs+=(-p "$c"); done
    run_step "test (affected:$crates)" cargo test "${pkgs[@]}" --no-fail-fast
}

# ---- tiers -------------------------------------------------------------------
gate_quick() {
    preflight
    acquire_lock
    warm_seed
    run_step "fmt --check" cargo fmt --all -- --check
    run_step "clippy (pinned $PINNED_CHANNEL)" cargo clippy --workspace --all-targets -- -D warnings
    quick_tests
    summary
    echo
    echo "quick tier only — run 'gate.sh full' (or ride a merge-train batch) before merge."
}

gate_full() {
    preflight
    acquire_lock
    warm_seed
    run_step "fmt --check" cargo fmt --all -- --check
    LEAK_BEFORE="$(leak_count)"
    run_step "test --workspace" cargo test --workspace --locked --no-fail-fast
    run_step "temp-fixture leak (#851)" leak_check "$LEAK_BEFORE"
    # Dimension 3 used to run clippy TWICE: once on whatever `stable` happened
    # to mean locally, once on a hardcoded `+1.97.0` guess at CI's stable —
    # because neither was KNOWN to be CI's toolchain, and a missing 1.97.0
    # hard-failed the gate (exit 4). Both halves were wrong by 2026-08-16: CI
    # installs `stable`, which resolves to 1.97.1, so the "CI's current stable"
    # run was 1.97.0 — itself a guess, and stale. rust-toolchain.toml now pins
    # the toolchain and preflight() has already proven the active rustc IS that
    # pin, so ONE run certifies the dimension exactly instead of two runs
    # approximating it. No dimension is dropped; the guess is.
    run_step "clippy (pinned $PINNED_CHANNEL = CI's stable)" \
        cargo clippy --workspace --all-targets -- -D warnings
    run_step "test (wasm-host subset)" \
        cargo test -p maw-cli -p maw-plugin-manifest --features wasm-host --locked --no-fail-fast
    run_step "clippy (wasm-host subset)" \
        cargo clippy -p maw-cli -p maw-plugin-manifest --all-targets --features wasm-host --locked -- -D warnings
    summary
}

# --- #851: temp-fixture leak guard -------------------------------------------
# The suite creates per-test directories under $TMPDIR and mostly never removes
# them. Measured 2026-08-16: 100,673 stale entries on white and 166,952 on
# black, oldest three days, across ~1,237 distinct fixture families.
#
# This is a CORRECTNESS risk, not untidiness. Names embed the pid, pid_max here
# is 4,194,304, and observed pids reach 4,108,102 — so pid reuse is live, and a
# fresh test process can be handed a fixture path that already exists and is
# already populated. #839 is the case that failed loudly: a test asserting a
# repo was ABSENT got flipped by a leftover directory it never created.
#
# It also makes cross-host flake-rate comparison unsound, because the rate
# becomes a function of how long since /tmp was last cleared.
#
# Reports the delta. Fails only when GATE_LEAK_STRICT=1, so this lands as a
# measurement first and becomes a bar once the families are migrated — turning
# it on today would red every run for a leak that predates the check.
leak_count() { ls "${TMPDIR:-/tmp}" 2>/dev/null | grep -cE '^(maw|schedule-)' || true; }

leak_check() {
    local before="$1" after delta
    after="$(leak_count)"
    delta=$(( after - before ))
    if [ "$delta" -le 0 ]; then
        echo "    no temp fixtures leaked"
        return 0
    fi
    echo "    leaked $delta temp fixture dirs (${before} -> ${after}) — see #851" >&2
    if [ "${GATE_LEAK_STRICT:-0}" = "1" ]; then
        echo "    GATE_LEAK_STRICT=1: failing on the leak" >&2
        return 1
    fi
    echo "    not failing the gate: this leak predates the check (GATE_LEAK_STRICT=1 to enforce)"
    return 0
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
