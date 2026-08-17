#!/usr/bin/env bash
# test-gate-preflight.sh — behavioural tests for gate.sh's preflight checks.
#
# Run it directly:  scripts/test-gate-preflight.sh
#
# Same standalone-shell-test convention as scripts/test-install-resolve.sh: no
# cargo, no network, seconds to run. It drives the real scripts/gate.sh with
# fake `rustup` / `rustc` / `cargo` on PATH, so it can assert on machine states
# this box does not actually have (missing wasm32 target, drifted toolchain).
#
# The load-bearing assertion is the CARGO MARKER: the fake `cargo` touches a
# file the moment gate.sh invokes it. "Fail early with a clear message rather
# than failing deep inside a plugin test" is only true if that marker is ABSENT
# when the preflight refuses — a preflight that merely printed a warning, or
# that ran after `cargo fmt`, leaves the marker behind and fails this test.
set -u -o pipefail

ROOT="$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)"
GATE="$ROOT/scripts/gate.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/maw-gate-preflight-XXXXXX")"
# Guarded cleanup. The fleet's no-rm rule (see gate-cache-refresh.sh's header)
# exists because a mis-typed rm on a GB-scale cache root caused the July disk
# crisis; this is a few KB of shell shims under a mktemp -d name, and leaking
# one dir per run on a machine that has hit 100% disk is the worse failure. The
# pattern guard makes the path impossible to mistake for anything else.
cleanup() {
    case "$WORK" in
        */maw-gate-preflight-??????) rm -rf "$WORK" ;;
        *) echo "refusing to clean unexpected path: $WORK" >&2 ;;
    esac
}
trap cleanup EXIT

FAILURES=0

fail() {
    printf 'FAIL %s\n' "$1" >&2
    FAILURES=$((FAILURES + 1))
}

pass() {
    printf 'ok   %s\n' "$1"
}

# The version gate.sh must consider correct is whatever rust-toolchain.toml
# pins — the test reads it from the same file rather than hardcoding a number,
# so bumping the pin never silently rots this test.
PINNED="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
    "$ROOT/rust-toolchain.toml" 2>/dev/null | head -1)"
if [ -z "$PINNED" ]; then
    fail "rust-toolchain.toml at repo root must pin an exact channel (found none)"
    printf '\n%s: 1 failure(s)\n' "$(basename "$0")" >&2
    exit 1
fi
pass "rust-toolchain.toml pins channel $PINNED"

# The pin must also carry the wasm32 target .github/workflows/ci.yml used to add
# by hand in both Rust jobs; that is the half of #823 the gate never mentioned.
# Match the `targets =` LINE, not the file: the file also explains wasm32 in
# prose, and a plain file-wide grep would stay green if the setting were deleted
# and only the comment survived.
if sed -n '/^[[:space:]]*targets[[:space:]]*=/p' "$ROOT/rust-toolchain.toml" |
    grep -q 'wasm32-unknown-unknown'; then
    pass "rust-toolchain.toml declares the wasm32-unknown-unknown target"
else
    fail "rust-toolchain.toml must list wasm32-unknown-unknown in targets"
fi

# ---- fake toolchain shims ----------------------------------------------------
# $1 case dir, $2 rustc version to report, $3 = "yes"/"no" wasm32 target present,
# $4 (optional) toolchain NAME rustup reports as active. It defaults to $2,
# which is what an exact-version pin looks like; pass it explicitly to model a
# channel-style pin, where the name and the rustc version differ.
make_shims() {
    local dir="$1" version="$2" wasm="$3" tc="${4:-$2}"
    local bin="$dir/bin"
    mkdir -p "$bin"

    cat >"$bin/rustc" <<EOF
#!/bin/sh
case "\$1" in
  --version) echo "rustc $version (fakehash 2026-01-01)" ;;
  *) exit 0 ;;
esac
EOF

    # `rustup target list --installed` prints one target per line. The host
    # target is always there; wasm32 is the variable under test.
    cat >"$bin/rustup" <<EOF
#!/bin/sh
if [ "\$1" = "target" ] && [ "\$2" = "list" ]; then
  echo x86_64-unknown-linux-gnu
  [ "$wasm" = yes ] && echo wasm32-unknown-unknown
  exit 0
fi
if [ "\$1" = "toolchain" ] && [ "\$2" = "list" ]; then
  echo "$tc-x86_64-unknown-linux-gnu (active, default)"
  exit 0
fi
if [ "\$1" = "show" ]; then
  echo "$tc-x86_64-unknown-linux-gnu"
  exit 0
fi
exit 0
EOF

    # Any cargo invocation at all leaves the marker. gate.sh must not reach
    # this when the preflight refuses.
    cat >"$bin/cargo" <<EOF
#!/bin/sh
: >"$dir/cargo-was-invoked"
exit 0
EOF

    chmod +x "$bin/rustc" "$bin/rustup" "$bin/cargo"
    printf '%s' "$bin"
}

# $1 case name, $2 tier, $3 rustc version, $4 wasm yes/no, $5 toolchain name
# ("" = same as the rustc version), [$6..] extra env
run_gate() {
    local case_name="$1" tier="$2" version="$3" wasm="$4" tc="${5:-}"
    shift 5
    local dir="$WORK/$case_name"
    mkdir -p "$dir"
    local bin
    bin="$(make_shims "$dir" "$version" "$wasm" "${tc:-$version}")"
    (
        cd "$ROOT" || exit 9
        PATH="$bin:$PATH" \
            GATE_TARGET_DIR="$dir/target-gate" \
            MAW_GATE_CACHE="$dir/no-such-cache" \
            env "$@" "$GATE" "$tier"
    ) >"$dir/stdout" 2>"$dir/stderr"
    echo "$?" >"$dir/exit"
}

marker_exists() { [ -f "$WORK/$1/cargo-was-invoked" ]; }
gate_exit() { cat "$WORK/$1/exit"; }
gate_stderr() { cat "$WORK/$1/stderr" "$WORK/$1/stdout"; }

# Build an isolated repository around the real gate script. Repo-hygiene cases
# must not mutate this checkout or the common .git/info/exclude shared by its
# linked worktrees.
make_gate_repo() {
    local dir="$WORK/$1" repo="$WORK/$1/repo"
    mkdir -p "$repo/scripts"
    cp "$GATE" "$repo/scripts/gate.sh"
    cp "$ROOT/.gitignore" "$ROOT/rust-toolchain.toml" "$repo/"
    git -C "$repo" init -q
    git -C "$repo" config user.name "gate preflight test"
    git -C "$repo" config user.email "gate-preflight@example.invalid"
    git -C "$repo" config commit.gpgsign false
    git -C "$repo" config core.hooksPath /dev/null
    git -C "$repo" add -f .gitignore rust-toolchain.toml scripts/gate.sh
    git -C "$repo" commit --no-verify -qm baseline
}

# $1 case name, $2 tier. The repository must already exist via make_gate_repo.
run_gate_in_repo() {
    local tier="$2" dir="$WORK/$1" repo="$WORK/$1/repo"
    local bin
    bin="$(make_shims "$dir" "$PINNED" yes "$PINNED")"
    (
        cd "$repo" || exit 9
        PATH="$bin:$PATH" \
            GATE_TARGET_DIR="$dir/target-gate" \
            MAW_GATE_CACHE="$dir/no-such-cache" \
            "$repo/scripts/gate.sh" "$tier"
    ) >"$dir/stdout" 2>"$dir/stderr"
    echo "$?" >"$dir/exit"
}

assert_tracked() {
    local case_name="$1" path="$2"
    if ! git -C "$WORK/$case_name/repo" ls-files --error-unmatch -- "$path" \
        >/dev/null 2>&1; then
        fail "$case_name setup: $path never entered the isolated index"
    fi
}

# ---- case 1: wasm32 target missing → refuse BEFORE any cargo runs ------------
run_gate missing-wasm quick "$PINNED" no ""
if [ "$(gate_exit missing-wasm)" = 0 ]; then
    fail "missing wasm32 target: gate.sh exited 0 (must fail)"
else
    pass "missing wasm32 target: gate.sh fails (exit $(gate_exit missing-wasm))"
fi
if gate_stderr missing-wasm | grep -q 'wasm32-unknown-unknown'; then
    pass "missing wasm32 target: message names the target"
else
    fail "missing wasm32 target: message never names wasm32-unknown-unknown"
fi
if gate_stderr missing-wasm | grep -q 'rustup target add'; then
    pass "missing wasm32 target: message gives the fix command"
else
    fail "missing wasm32 target: message must give 'rustup target add ...'"
fi
if marker_exists missing-wasm; then
    fail "missing wasm32 target: cargo RAN — the preflight is not early enough"
else
    pass "missing wasm32 target: no cargo step ran (failed early)"
fi

# ---- case 2: pinned toolchain, target present → gate proceeds ----------------
# The over-fire guard. A preflight that refuses a correctly-provisioned machine
# is a bug, not caution.
run_gate happy quick "$PINNED" yes ""
if marker_exists happy; then
    pass "provisioned machine: gate.sh proceeds to cargo"
else
    fail "provisioned machine: preflight blocked a machine that has everything"
fi
if [ "$(gate_exit happy)" = 0 ]; then
    pass "provisioned machine: gate.sh exits 0"
else
    fail "provisioned machine: gate.sh exited $(gate_exit happy), want 0"
fi

# ---- case 3: toolchain drift → refuse, and name both versions ---------------
run_gate drift quick 1.94.0 yes ""
if [ "$(gate_exit drift)" = 0 ]; then
    fail "toolchain drift: gate.sh exited 0 (must fail)"
else
    pass "toolchain drift: gate.sh fails (exit $(gate_exit drift))"
fi
if gate_stderr drift | grep -q '1\.94\.0' && gate_stderr drift | grep -q "$PINNED"; then
    pass "toolchain drift: message names both the active and the pinned version"
else
    fail "toolchain drift: message must name both 1.94.0 and $PINNED"
fi
if marker_exists drift; then
    fail "toolchain drift: cargo RAN — the preflight is not early enough"
else
    pass "toolchain drift: no cargo step ran (failed early)"
fi

# ---- case 4: drift with the explicit escape hatch → proceeds ----------------
# Named escape hatches keep the guard from being a wall; the existing
# GATE_ALLOW_MISSING_197 set the precedent.
run_gate drift-allowed quick 1.94.0 yes "" GATE_ALLOW_TOOLCHAIN_DRIFT=1
if marker_exists drift-allowed; then
    pass "GATE_ALLOW_TOOLCHAIN_DRIFT=1: drift is accepted explicitly"
else
    fail "GATE_ALLOW_TOOLCHAIN_DRIFT=1: escape hatch did not let the gate run"
fi

# ---- case 5: the FULL tier preflights too -----------------------------------
# `full` is the tier the merge decision rides on, and its wasm-host subset is
# the one that actually cross-compiles. Guarding only `quick` would leave the
# expensive path unguarded.
run_gate full-missing-wasm full "$PINNED" no ""
if [ "$(gate_exit full-missing-wasm)" = 0 ]; then
    fail "full tier, missing wasm32: gate.sh exited 0 (must fail)"
else
    pass "full tier, missing wasm32: gate.sh fails (exit $(gate_exit full-missing-wasm))"
fi
if marker_exists full-missing-wasm; then
    fail "full tier, missing wasm32: cargo RAN — the preflight is not early enough"
else
    pass "full tier, missing wasm32: no cargo step ran (failed early)"
fi

# ---- case 6: the full tier no longer demands a hardcoded second toolchain ----
# It used to hard-fail (exit 4) unless rustc 1.97.0 was installed — a guess at
# CI's stable that was already wrong (CI's stable is 1.97.1). A provisioned
# machine must now get through `full` on the pin alone.
run_gate full-happy full "$PINNED" yes ""
if [ "$(gate_exit full-happy)" = 0 ]; then
    pass "full tier, provisioned machine: gate.sh exits 0 on the pin alone"
else
    fail "full tier, provisioned machine: gate.sh exited $(gate_exit full-happy), want 0"
fi
if gate_stderr full-happy | grep -q '1\.97\.0'; then
    fail "full tier: still references the hardcoded 1.97.0 guess"
else
    pass "full tier: no hardcoded second-toolchain guess left"
fi

# ---- case 7: a channel-style pin is accepted by NAME ------------------------
# Over-fire guard for a future `channel = "nightly-YYYY-MM-DD"` (or "beta"):
# such a toolchain's `rustc --version` never spells the channel name, so a
# version-only comparison would refuse a machine that IS running the pin. The
# check accepts either form. Modelled here by claiming the pin as the active
# toolchain NAME while rustc reports an unrelated version.
run_gate channel-pin quick 1.99.0-nightly yes "$PINNED"
if marker_exists channel-pin; then
    pass "channel-style pin: accepted by active-toolchain name"
else
    fail "channel-style pin: refused a machine that IS running the pin"
fi

# ---- case 8: tracked files matching .gitignore fail before cargo ------------
# Exercise both public tiers. A stale branch can reintroduce a runtime artifact
# even though the ignore rule is already correct; Git keeps tracking it until a
# gate checks the index explicitly (#888).
for tier in quick full; do
    case_name="tracked-ignored-$tier"
    make_gate_repo "$case_name"
    mkdir -p "$WORK/$case_name/repo/crates/maw-cli/.maw"
    printf '{"runtime":true}\n' >"$WORK/$case_name/repo/crates/maw-cli/.maw/audit.jsonl"
    git -C "$WORK/$case_name/repo" add -f crates/maw-cli/.maw/audit.jsonl
    assert_tracked "$case_name" crates/maw-cli/.maw/audit.jsonl
    run_gate_in_repo "$case_name" "$tier"
    if [ "$(gate_exit "$case_name")" = 0 ]; then
        fail "$tier tier, tracked ignored file: gate.sh exited 0 (must fail)"
    else
        pass "$tier tier, tracked ignored file: gate.sh fails early"
    fi
    if gate_stderr "$case_name" | grep -Fq 'crates/maw-cli/.maw/audit.jsonl'; then
        pass "$tier tier, tracked ignored file: message names the path"
    else
        fail "$tier tier, tracked ignored file: message must name the path"
    fi
    if gate_stderr "$case_name" | grep -Fq 'git rm --cached'; then
        pass "$tier tier, tracked ignored file: message gives the untrack fix"
    else
        fail "$tier tier, tracked ignored file: message must give the untrack fix"
    fi
    if marker_exists "$case_name"; then
        fail "$tier tier, tracked ignored file: cargo RAN — the preflight is too late"
    else
        pass "$tier tier, tracked ignored file: no cargo step ran"
    fi
done

# ---- case 9: deliberately tracked team charters remain allowed --------------
make_gate_repo teams-visible
mkdir -p "$WORK/teams-visible/repo/.maw/teams"
printf 'name: example\n' >"$WORK/teams-visible/repo/.maw/teams/t.yaml"
git -C "$WORK/teams-visible/repo" add -f .maw/teams/t.yaml
assert_tracked teams-visible .maw/teams/t.yaml
run_gate_in_repo teams-visible quick
if [ "$(gate_exit teams-visible)" = 0 ] && marker_exists teams-visible; then
    pass "team charter re-inclusion: gate proceeds to cargo"
else
    fail "team charter re-inclusion: gate must allow .maw/teams/t.yaml"
fi

# ---- case 10: developer-local excludes do not change the repo invariant ------
# Only the repository-controlled root .gitignore defines this check. Using
# --exclude-standard would make one developer's .git/info/exclude fail a gate
# that stays green everywhere else.
make_gate_repo local-exclude
printf 'local\n' >"$WORK/local-exclude/repo/local-only.txt"
git -C "$WORK/local-exclude/repo" add -f local-only.txt
assert_tracked local-exclude local-only.txt
printf 'local-only.txt\n' >>"$WORK/local-exclude/repo/.git/info/exclude"
run_gate_in_repo local-exclude quick
if [ "$(gate_exit local-exclude)" = 0 ] && marker_exists local-exclude; then
    pass "developer-local exclude: repository-controlled .gitignore remains authoritative"
else
    fail "developer-local exclude: .git/info/exclude must not change the gate verdict"
fi

# ---- case 11: a missing root policy fails closed ----------------------------
make_gate_repo missing-ignore
git -C "$WORK/missing-ignore/repo" rm -q .gitignore
run_gate_in_repo missing-ignore quick
if [ "$(gate_exit missing-ignore)" = 0 ]; then
    fail "missing root .gitignore: gate.sh exited 0 (must fail closed)"
else
    pass "missing root .gitignore: gate.sh fails closed"
fi
if gate_stderr missing-ignore | grep -Fq 'could not inspect tracked files'; then
    pass "missing root .gitignore: message names the failed inspection"
else
    fail "missing root .gitignore: message must explain the failed inspection"
fi
if marker_exists missing-ignore; then
    fail "missing root .gitignore: cargo RAN — an unverified policy must stop early"
else
    pass "missing root .gitignore: no cargo step ran"
fi

echo
if [ "$FAILURES" -eq 0 ]; then
    echo "$(basename "$0"): all checks passed"
    exit 0
fi
printf '%s: %d failure(s)\n' "$(basename "$0")" "$FAILURES" >&2
exit 1
