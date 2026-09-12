#!/usr/bin/env bash
# test-gate-format.sh — behavioural coverage for include-only Rust sources.
#
# Run it directly: scripts/test-gate-format.sh
#
# cargo fmt cannot discover maw-cli's core_impl sources through the generated
# OUT_DIR include list. Drive the real gate in a tiny repository to prove both
# gate tiers reject formatting drift in ordinary and nested-include fragments.
set -u -o pipefail

ROOT="$(CDPATH='' cd -- "$(dirname "$0")/.." && pwd)"
GATE="$ROOT/scripts/gate.sh"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/maw-gate-format-XXXXXX")"

cleanup() {
    case "$WORK" in
        */maw-gate-format-??????) rm -rf "$WORK" ;;
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

PINNED="$(sed -n 's/^[[:space:]]*channel[[:space:]]*=[[:space:]]*"\([^"]*\)".*/\1/p' \
    "$ROOT/rust-toolchain.toml" 2>/dev/null | head -1)"

make_repo() {
    local case_name="$1"
    local repo="$WORK/$case_name/repo" bin="$WORK/$case_name/bin"
    mkdir -p "$repo/scripts" "$repo/crates/maw-cli/src/core_impl" "$bin"
    cp "$GATE" "$repo/scripts/gate.sh"
    cp "$ROOT/.gitignore" "$ROOT/rust-toolchain.toml" "$repo/"

    cat >"$bin/rustc" <<EOF
#!/bin/sh
echo "rustc $PINNED (fakehash 2026-01-01)"
EOF
    cat >"$bin/rustup" <<EOF
#!/bin/sh
case "\$1 \${2:-}" in
  "target list") printf '%s\n' x86_64-unknown-linux-gnu wasm32-unknown-unknown ;;
  "show active-toolchain") echo "$PINNED-x86_64-unknown-linux-gnu" ;;
esac
EOF
    cat >"$bin/cargo" <<'EOF'
#!/bin/sh
exit 0
EOF
    chmod +x "$bin/rustc" "$bin/rustup" "$bin/cargo"

    git -C "$repo" init -q
    git -C "$repo" config user.name "gate format test"
    git -C "$repo" config user.email "gate-format@example.invalid"
    git -C "$repo" config commit.gpgsign false
    git -C "$repo" config core.hooksPath /dev/null
}

run_gate() {
    local case_name="$1" tier="$2"
    local repo="$WORK/$case_name/repo" bin="$WORK/$case_name/bin"
    (
        cd "$repo" || exit 9
        PATH="$bin:$PATH" \
            GATE_TARGET_DIR="$WORK/$case_name/target-gate" \
            MAW_GATE_CACHE="$WORK/$case_name/no-such-cache" \
            "$repo/scripts/gate.sh" "$tier"
    ) >"$WORK/$case_name/stdout" 2>"$WORK/$case_name/stderr"
    echo "$?" >"$WORK/$case_name/exit"
}

for tier in quick full; do
    for kind in ordinary fragment; do
        case_name="drift-$tier-$kind"
        make_repo "$case_name"
        if [ "$kind" = ordinary ]; then
            source_name="ordinary.rs"
            cat >"$WORK/$case_name/repo/crates/maw-cli/src/core_impl/$source_name" <<'EOF'
fn    badly_formatted_ordinary(  )   {   }
EOF
        else
            source_name="include_fragment.rs"
            # This shape matches attach_private_tests.rs: it is included inside
            # a surrounding module, but rustfmt accepts and checks it as a file.
            cat >"$WORK/$case_name/repo/crates/maw-cli/src/core_impl/$source_name" <<'EOF'
#[test]
    fn badly_formatted_include_fragment(  )   {   }
EOF
        fi
        git -C "$WORK/$case_name/repo" add -f .
        git -C "$WORK/$case_name/repo" commit --no-verify -qm baseline

        run_gate "$case_name" "$tier"
        if [ "$(cat "$WORK/$case_name/exit")" = 0 ]; then
            fail "$tier rejects formatting drift in include-only $kind syntax"
        else
            pass "$tier rejects formatting drift in include-only $kind syntax"
        fi
        if grep -q "$source_name" \
            "$WORK/$case_name/stdout" "$WORK/$case_name/stderr"; then
            pass "$tier reports the malformed include-only $kind source"
        else
            fail "$tier output does not show malformed $source_name"
        fi
    done
done

if [ "$FAILURES" -gt 0 ]; then
    printf '\n%s: %s failure(s)\n' "$(basename "$0")" "$FAILURES" >&2
    exit 1
fi

printf '\n%s: all checks passed\n' "$(basename "$0")"
