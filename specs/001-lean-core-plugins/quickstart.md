# Validation Quickstart: Lean-Core Workflow Plugins

This guide validates a completed child slice; it does not authorize skipping that child's frozen
contract or independent review.

## 1. Pin clean repositories

```bash
MAW_RS=/path/to/clean/maw-rs-worktree
MAW_PLUGINS=/path/to/clean/maw-plugins-worktree
git -C "$MAW_RS" status --short
git -C "$MAW_PLUGINS" status --short
git -C "$MAW_RS" rev-parse HEAD
git -C "$MAW_PLUGINS" rev-parse HEAD
```

Both status outputs must be empty. The maw-rs branch must originate at the issue's frozen
`origin/alpha`; the external branch targets `maw-plugins/main`.

A downstream checkout must show a creation base newer than or equal to the recorded accepted external
artifact evidence. Never validate a cutover branch created before artifact acceptance.

## 2. Build and pin the external artifact

```bash
maw plugin build "$MAW_PLUGINS/packages/<name>"
if command -v sha256sum >/dev/null 2>&1; then
  sha256sum "$MAW_PLUGINS/packages/<name>/plugin.wasm"
else
  shasum -a 256 "$MAW_PLUGINS/packages/<name>/plugin.wasm"
fi
```

The digest must equal the active manifest's `artifact.sha256`. Run the external repository's exact
CI/test commands and invoke the committed artifact against the child contract, not only source or a
development fallback.

Rust guests also run the maw-plugins native-pure SDK/guest test loop (`rlib` with a fake `Host`) and
their wasm32 `cdylib` builds against the vendored ABI fixture provenance before artifact acceptance.
Using the pinned clean build environment, build each scoped Rust guest twice from clean outputs;
both byte streams must match each other, the committed `plugin.wasm`, and the manifest digest.
Record the toolchain, exact commands, source SHA, both build-log hashes, and artifact SHA-256.

## 3. Run parity before native removal

For every `ParityCase`, record:

- native exit/stdout/stderr and ordered side effects on the frozen base;
- committed artifact exit/stdout/stderr and ordered host calls;
- no-call/no-mutation evidence for every rejected input;
- alias, help/version ownership, missing/refused artifact, and legacy-state cases.
- for workflow lifecycle/pane submission and every layout, batch, worktree, health, or maintenance
  operation, the typed DTO, injected-host call trace, and proof that no arbitrary `maw.cli.run`
  invocation occurs; mailbox messaging instead proves named-root state operations;
- for every new/scoped effectful route/subcommand, the host-derived invocation intent, effective
  capability intersection, exact resource plan/call budget, excluded-target refusals, and one-shot
  replay denial; for an eligible legacy route, the exact immutable path/source/hash and no-new-typed-
  capability proof;
- for lifecycle/worktree actions, issuance and consumption of the exact workspace, planned-workspace,
  engine, lifecycle-member, cwd, and mission-store refs plus forged/stale/cross-scope refusals;
- for root/document operations, proof that host construction is resolve-only, display paths cannot
  be reused as authority, archive copy is durably complete before removal, and one injected
  `nowMillis` supplies all guest timestamps;
- for authority-bearing state, semantic host mutation plus durable-record commit, unsealed-domain
  refusal, post-seal missing-record refusal, and the domain-specific migration/seal evidence;
- for every agent instruction, the exact operator/document/IssueRef/template/previous-host-write
  ContentRef origin, host-materialized bytes, durable content digest, and substitution/replay denial;
- for providers, a native-bound executable id and native schema validation of args/environment;
- for occupancy, proof that no raw environment/secret value crosses the host and that frozen
  pid/displayHome output fields cannot be reused as process/filesystem/provider authority.

Do not proceed when any field differs unless the child issue explicitly approves that correction.
The current planned corrections requiring separate explicit human approval are ordinary named-root/
operator-document/archive-tree symlink hardening; fail-closed Team pane-inventory errors (instead of
missing/fresh-wake behavior); fail-closed More/Wave pane-read errors; Swarm atomic private 0600 state
(instead of its plain write); and Codex account probe failure becoming `unknown`/not `--free`
(instead of `free`). Record the frozen native RED and approval in the child ledger before implementing
any correction.

## 4. Verify ownership after cutover

```bash
rg -n 'DispatcherEntry.*"(<canonical>|<alias>)"' "$MAW_RS/crates/maw-cli/src"
rg -n '(<canonical>|<alias>)' \
  "$MAW_RS/crates/maw-cli/src/core_impl/plugin_known_verbs.rs"
```

The first search must show no reachable native owner for an extracted verb. The known external
ownership search must show canonical package and alias coverage. Run help, completion, plugin list,
doctor, missing artifact, bad hash, old SDK, and denied capability scenarios.

## 5. Prove wake and attach stability

Use the child issue's exact focused tests twice with an isolated target. Run unrelated workflow
plugins present/absent with identical wake/attach results. Run Codex with the accepted provider for
parity, explicit missing/refused Codex for the approved pre-mutation failure, and non-Codex
wake/attach without the Codex artifact for byte-identical behavior. At minimum cover native wake
parser/launch/cold-start and attach picker, sleeping-target wake, and binary fast path fixtures. Do
not modify wake/attach expectations merely to accommodate extraction.

Example target convention:

```bash
CARGO_TARGET_DIR=/mnt/nvme1/cargo/target-omx-963-<slice> \
  cargo test -p maw-cli wake --locked --no-fail-fast
CARGO_TARGET_DIR=/mnt/nvme1/cargo/target-omx-963-<slice> \
  cargo test -p maw-cli attach --locked --no-fail-fast
```

Child issues name narrower exact filters where available.

## 6. Gate exact downstream bytes

```bash
git -C "$MAW_RS" diff --check
rg -n '^ψ/|^ψ/\*' "$MAW_RS/.gitignore"
git -C "$MAW_RS" diff --name-only | rg '^ψ/' || true

# Required for include-fragment changes until maw-rs#964 lands.
rustfmt --edition 2021 --check "$MAW_RS"/crates/maw-cli/src/core_impl/*.rs

GATE_TARGET_DIR=/mnt/nvme1/cargo/target-gate-omx-963-<slice>-quick \
  "$MAW_RS/scripts/gate.sh" quick
GATE_TARGET_DIR=/mnt/nvme1/cargo/target-gate-omx-963-<slice>-full \
  "$MAW_RS/scripts/gate.sh" full
```

Run full unpiped/non-PTY before merge or promotion. Record exact commands, exit results, final diff
SHA-256, authored line count, source/artifact SHA, and independent review in the child issue/PR.

## 7. Convergence checks

- Every spec FR/SC maps to completed tasks and issue evidence.
- Every scoped canonical verb/alias has exactly one owner.
- Every external artifact is hash-pinned and CI-validated.
- Help/completions/plugin-list/doctor agree.
- Missing/refused paths are loud and actionable.
- No raw process environment, secret, unrestricted fs/process/tmux, or trust mutation grant exists.
- Combined exact tree passes full gate and external CI before release verification.
