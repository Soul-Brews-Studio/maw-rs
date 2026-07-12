# Extracting a core verb to a WASM plugin

Use this guide when moving a `maw` CLI verb from `maw-rs` into the public
`Soul-Brews-Studio/maw-plugins` monorepo. An extraction is a coordinated PR pair,
not a file move: the native implementation must be rewritten against the WASM host
ABI, published, pinned, and exercised through dispatcher fallthrough before its core
registration is removed.

## 1. Decide whether the verb belongs outside core

Apply the lean-core criterion:

> **CORE** is what the lead needs to run the fleet. **PLUGIN** is what a user can
> live without.

Do not extract a verb merely because it is cold. First check the current extraction
plan and ADRs. Daemon, PTY, FFI, transport-runtime, or tightly coupled verbs remain
native until a safe WASM ABI exists. The end state must be ZERO-BUN: a ship-tier WASM
artifact, not a Bun-only plugin.

For a composite command, prefer extracting only cold subverbs. Keep fleet-critical
subverbs native and let the native dispatcher return the documented fallthrough code
for plugin-owned subverbs. Moving a verb on the never-extract list, or moving a plugin
back into core, requires a new ADR.

## 2. Read the source and audit callers before coding

Read the complete handler, its tests and fixtures, and the relevant host ABI. Then run
a narrow repository audit, for example:

```bash
rg -n '\b<verb>\b|<VERB>_USAGE' crates scripts docs
```

Classify every match. In particular, look for:

- internal or sub-dispatch calls;
- other verbs that shell out to `maw <verb>`;
- boot, systemd, cron, startup, deploy, and fleet-recovery paths;
- aliases and completion registrations;
- shared helpers whose removal would affect another native verb.

The `wake-all` outage is the scar behind this rule: extracting `wake` once dropped an
internal boot sub-dispatch and silently took every oracle offline. If a critical caller
exists, keep that path native or port it explicitly. Record the audit and findings in
PR-B even when the result is “no callers.”

## 3. Plan the PR pair

Create two independently reviewable branches:

- **PR-A — `maw-plugins`:** add `packages/NN-<verb>/` with the Rust WASM port,
  manifest, lockfile, and built artifact.
- **PR-B — `maw-rs`:** remove the native handler/registration and run the pinned
  PR-A artifact through plugin fallthrough tests. Add a generic host ABI only when the
  port proves one is missing.

Publish PR-A before removing the native path. PR-B should depend on PR-A and embed the
exact final `plugin.json` and `plugin.wasm` used for parity coverage.

Keep each PR within the normal diff budget where practical. Generated `plugin.wasm`
and lockfiles are excluded. If the faithful Rust port necessarily exceeds the budget,
state why in PR-A rather than splitting the repository into a broken intermediate
state.

## 4. Build PR-A with least privilege

Create a self-contained Rust `cdylib` targeting `wasm32-unknown-unknown`. The manifest
must include:

- `name` and semantic `version`;
- `sdk: "^1.0.0"`;
- `cli.command`, help text, and any real aliases;
- WASM entry/export and artifact SHA-256;
- only the capabilities the implementation actually calls.

Port I/O to the narrowest existing JSON ABI: `maw.fs.*`, `maw.tmux.*`, `maw.exec.*`,
`maw.paths.get`, and so on. Do not recreate native filesystem, process, or raw-tmux
access inside the guest. Declare command-specific process capabilities such as
`proc:exec:git`, not broad process access.

Install the published SDK in the plugin worktree and point the build at that local
toolchain root:

```bash
npm install --save-dev @maw-rs/wasm-sdk@^1.0.0
MAW_WASM_SDK_DIR="$PWD" \
  maw plugin build packages/NN-<verb>
```

For SDK development, `MAW_WASM_SDK_DIR=<maw-rs-worktree>/packages/wasm-sdk`
continues to override the published package. The npm SDK version follows the maw host
ABI contract; keep `plugin.json`'s `sdk` range aligned with the compatibility table in
`packages/wasm-sdk/README.md`.

Copy the final artifact into the package, update its manifest SHA-256, and use that
same pair in PR-B's fixture.

## 5. Keep the invoke context byte-frozen

The shared plugin invoke-context JSON is a compatibility contract. Do **not** add
verb-specific timestamps, paths, fixture data, environment values, or options to
`invoke_context_json`. Even an “optional” field changes every plugin invocation and
breaks committed WASM parity goldens.

Use these escape hatches, in order:

1. an existing CLI argument or plugin-local deterministic option;
2. an existing generic host ABI and a least-privilege capability;
3. a new generic, reusable host ABI with security tests and audit coverage.

`maw.time.now` is the precedent: time was a legitimate cross-plugin need, so it became
a generic host call rather than a `dreamEpoch` field injected into every invocation.
Apply the same rule to future ABI gaps. Keep the default context byte-identical and run
the invoke-context and WASM parity tests whenever runtime serialization is touched.

## 6. Build PR-B and prove parity

On a fresh `origin/alpha` branch:

1. delete the native handler file or registration;
2. remove native-only completion/help registration;
3. preserve unrelated shared helpers and similarly named subcommands;
4. install the pinned PR-A fixture under a temporary `MAW_PLUGINS_DIR` in the existing
   integration test;
5. change dispatcher assertions from native to plugin fallthrough (`NativeError` in
   the registration-only status API);
6. compare plugin output with the committed native golden byte-for-byte;
7. cover meaningful side effects, filters, aliases, and option-injection guards.

Never delete a fixture to make the extraction pass. If the host ABI cannot represent
an observable behavior, stop and add a generic ABI or document a deliberate parity
plan; do not silently weaken the test.

PR-B's body must include all three mandatory checklist entries:

1. **Sub-dispatch/internal-caller audit:** query, classified findings, and the explicit
   wake-all conclusion.
2. **ADR note:** why the verb is allowed to move, or the new ADR authorizing it.
3. **Parity plan:** exact goldens/side effects covered and any production smoke the
   lead must run.

## 7. Run isolated targeted gates

Never wait for another Cargo process. Give the worktree its own target directory and
run immediately; Cargo resolves its package-cache lock itself.

```bash
# PR-A
MAW_WASM_SDK_DIR=<maw-rs-worktree>/packages/wasm-sdk \
  maw plugin build packages/NN-<verb>
cargo clippy --manifest-path packages/NN-<verb>/Cargo.toml \
  --target wasm32-unknown-unknown -- -D warnings

# PR-B
CARGO_TARGET_DIR=/tmp/maw-rs-target-<worktree> \
  cargo test -p maw-cli --test <targeted-parity-test> -- --nocapture
CARGO_TARGET_DIR=/tmp/maw-rs-target-<worktree> \
  cargo clippy -p maw-cli -p maw-plugin-manifest --all-targets -- -D warnings
```

Add focused host-security or parity-harness tests when PR-B changes an ABI. The coder
runs the requested targeted gates; the lead runs the full workspace gate and
production parity smoke unless the task explicitly says otherwise. Report the exact
commands, not just “tests green.”

## 8. Serialize against integration churn

Extraction waves touch dispatcher tables, host allowlists, fixtures, and shared plugin
runtime code. Avoid stale PR-Bs:

1. finish and push PR-A;
2. immediately before opening PR-B, fetch `origin/alpha` and rebase the work branch;
3. resolve conflicts from repository truth, especially dispatcher registrations and
   host ABI allowlists;
4. rerun the targeted parity test and clippy after the rebase;
5. do not overwrite another extraction's ABI or renumbering work;
6. coordinate overlapping host-ABI changes with the lead and merge them serially.

If PR-A changes after review, rebuild it, update the SHA and PR-B fixture, rerun parity,
and push both branches. Never modify the shared invoke context as a shortcut around
serialization or coordination.

## 9. Open and report both PRs

Open PR-A against `maw-plugins`' integration branch and PR-B against `maw-rs` `alpha`.
Put the issue-closing reference in PR-B as directed by the task. Report both URLs,
exact gate evidence, the extraction/root-cause summary, and any production smoke the
lead still owns:

```bash
maw hey <lead> "done #N PR-A <url> PR-B <url> gates green: <commands>; root cause: <summary>"
```
