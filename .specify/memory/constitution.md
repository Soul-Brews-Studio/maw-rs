# maw-rs Constitution

The rules every specification, plan, and task in this repository inherits. Derived from
`CLAUDE.md` and from failures this project actually paid for. A principle here outranks
convenience, speed, and a coder's judgement in the moment.

## Core Principles

### I. Green Is A Hypothesis (NON-NEGOTIABLE)

A passing gate means nobody has proven the change wrong yet. It is the START of review, not the
end. Every change is reviewed by a party that did not write it, and that party is asked to
**refute** specific named angles — not to "please review".

A reviewer who only ever confirms is not reviewing.

Evidence for this rule: a credential-exfiltration path shipped with `tsc` clean, 136/136 tests
green, and a green build. The gate cannot see that class of bug.

### II. Verify The Artifact, Not The Report

Check the published binary, not the merge. Check real `main`, not the PR branch. Check the
rendered page, not the extractor's complaint. When a measurement disagrees with the code, suspect
the measurement — a port collision and an empty token each made working systems look broken.

Claims carry evidence labels: `[observed]` (a log, command, or record says so directly),
`[source-derived]` (read from code at a named commit), `[inference]` (our interpretation, marked
as such), `[GAP]` (unknown, and we will not guess).

### III. Fail Closed; Never Claim Unconfirmed Success

Code MUST NOT report success it cannot confirm. If a send cannot be verified as delivered, it is
an error, not a `delivered` message. Four duplicate confirm loops in this repo each returned
"delivered" for input that was never submitted; all four were removed or made to fail closed.

Where a surface is deliberately unconfirmable, it MUST say so in its own output.

### IV. Lowest Layer That Can Hold It

New logic belongs in the lowest layer that can contain it. Keep I/O out of pure core leaves;
filesystem-facing leaf adapters MUST contain and test their own I/O. `cargo tree` is the
authoritative dependency graph — not intuition about where something "feels like it goes".

### V. Small, Reviewable, Reversible

Each PR stays at or below **250 changed lines**, excluding lockfiles and generated `plugin.wasm`.
Exceeding it requires saying so explicitly in the PR body, with the reason.

Never rewrite published history: no `git push --force`, no `--force-with-lease`. Push additive
follow-up commits instead.

### VI. Stable Native Kernel

The native binary MUST retain the trusted host boundary: `maw wake`, `maw attach`/`maw a`, `maw hey`,
`maw peek`, `maw serve`, plugin discovery and verification, capability enforcement,
signed/authenticated transport, and raw tmux/process/filesystem adapters. Extraction MUST NOT move
or shadow those entrypoints or their state machines. Their binary fast paths, tmux effects, accepted
`https://god.buildwithoracle.com` serve-origin contract, and every non-Codex observable contract
remain frozen. With an accepted Codex provider installed, Codex launch/resume remains at parity;
when Codex is explicitly selected and that provider is missing or refused, the only approved change
is a loud actionable failure before mutation.

### VII. Plugins Own Optional and Product-Specific Workflows

Optional workflow verbs and engine-vendor policy MUST live in installable plugins, not in the native
dispatcher. For #963 this includes `bring`/`b`, `split`, `team`/`t` and team-only companions, plus
Codex-specific account, team, profile, and provider behavior. The native kernel MAY expose generic
typed host or provider contracts, but MUST NOT retain a second product-specific implementation after
the atomic cutover. Existing artifacts are extended rather than duplicated.

### VIII. Behavior-Frozen, Client-First Extraction

Every extraction starts with source/caller/fixture inventory and a failing parity proof. The external
artifact MUST reproduce frozen argv, stdout, stderr, exit-code, JSON, safety, and ordering contracts
before native dispatch is removed; fixtures are updated only additively. Apply the Client-First rule
below to every host/plugin or core/artifact protocol change: ship the accepting/permissive side first,
prove it against the deployed older peer, then tighten the other side. Native and plugin owners MUST
not both be reachable after atomic cutover.

### IX. Fail-Closed Typed Capability Boundary

Guests receive only narrow typed capabilities, never raw `/proc`, auth secrets, unrestricted paths,
arbitrary processes, unrestricted tmux, or arbitrary `maw.cli.run` argv. Missing, stale,
hash-invalid, SDK-incompatible, under-capable, or malformed plugins fail loudly with actionable
repair guidance. Human consent and authentication remain native. Non-idempotent host operations
return explicit ambiguous outcomes and are never retried after that boundary.

### X. Source-Proven Artifact-First Delivery

Guest source and committed `plugin.wasm` live in `Soul-Brews-Studio/maw-plugins`. Every scoped
artifact MUST be reproducible, verified by external CI, pinned by exact SHA-256, directly invoked
before its fresh downstream branch exists, and recorded with immutable evidence. The downstream
known-verb/help/doctor surface identifies its package and fails safely when absent. No product code
lands until the corresponding Spec Kit child explicitly names capabilities, parity fixtures, paired
PR ordering, bounded slices, and exact gates.

## Security Constraints

- A field name MUST NOT lie about what it holds. Serving a private key in a field called `pubkey`
  is a defect regardless of whether anyone has exploited it.
- Network-facing defaults MUST be the restrictive option. A daemon binding `0.0.0.0` by default
  makes every other protection conditional on network topology nobody verified.
- Authentication changes MUST state plainly what they do and do not authenticate. "Hardened"
  without a named threat is not a claim.
- A security fix widens only to the boundary of the proven mechanism. Scope creep in a security PR
  is how the fix stops being reviewable.

## Development Workflow

- **Gate**: `scripts/gate.sh quick` while iterating; `scripts/gate.sh full` before merge. All four
  CI dimensions carry `--no-fail-fast` so one red target cannot hide the rest.
- **Toolchain**: pinned by `rust-toolchain.toml`. Fix a drift by editing the pin — one line, one
  reviewable diff — never by `rustup update`.
- **Build isolation, not queueing**: every worktree gets its own
  `CARGO_TARGET_DIR=/mnt/nvme1/cargo/target-<slug>`. Never `/tmp` (root filesystem). A machine-wide
  cargo queue deadlocked coders for 20-45 minutes each, for nothing.
- **Branches**: `main` is protected and never receives a direct push. All PRs target `alpha`.
  Work branches are `agents/<type>-<issue>-<slug>` cut from `origin/alpha`.
- **maw verbs, never raw tmux.** If the verb does not exist, file the gap; do not fall back.
- **`ψ/` is never committed.** Verify before pushing.
- **Fixtures are evidence.** Update or add them when behavior changes; never delete a fixture to
  make a test pass.

### Client-First Ordering For Cross-Surface Breaks

When a change tightens what one surface accepts from another — server tightening on a client,
host on a plugin, daemon on a CLI — **ship the permissive half first.**

The order is:

1. Land the client change that sends the new thing. An older server ignores an unknown field,
   header, or subprotocol, so this is a no-op against every deployed version.
2. Verify the new client still works against the old server. Measure it; do not assume it.
3. Only then tighten the server to require it.

Reversing this breaks every client between the two releases, and the breakage is invisible in CI
because CI tests one version of both halves together.

**Evidence this rule was paid for.** #932/#937 hardened browser WebSocket upgrades to require a
one-use ticket in the subprotocol, and shipped ahead of the only client that could send one. Every
browser client broke, and the failure surfaced as an operator debugging session against a banner
that read `auth: open`. Measured afterwards on the actual binaries:

```
old server (9c1bfb9e) + old client (no subprotocol)        -> 101
old server (9c1bfb9e) + new client (sends maw.ws.v1,...)   -> 101   <- compatible, free
new server (96fd621d) + old client                         -> 401   <- the break
```

The compatible ordering was available at zero cost and was simply not considered.

**Corollary**: a deprecation window is not always the answer, but the *ordering* always is. Where a
window is also needed — offline peers that cannot upgrade in lockstep — name the removal release in
the same PR that opens the window.

### Dispatch Contract

Work handed to another agent MUST carry:

1. The mechanism, already traced, with file:line — and an instruction to verify it independently
2. Red-first: the failing test written on the exact base commit, with the named failure captured
3. Green twice, identical output
4. The exact gate command, unpiped
5. If a failure looks unrelated, reproduce on unmodified `alpha` FIRST
6. An explicit SCOPE line naming what may NOT be touched

### Review Protocol

1. The coder finishes and does NOT commit. It freezes the diff and publishes
   `git diff --cached <base> | sha256sum`.
2. The reviewer verifies the hash BEFORE reading. Mismatch means the tree moved — stop.
3. The reviewer attacks specific named angles.
4. Nothing commits until PASS.

## #963 Architecture and Delivery Constraints

- Workspace Rust remains edition 2021, forbids unsafe code, and treats Clippy warnings as errors.
  The plugin-manifest crate defines DTOs and injected host-operation traits only; `maw-cli` owns the
  production adapter and platform I/O. `maw-tmux` remains a native Rust host boundary.
- `wake`, `attach`, `hey`, `peek`, and `serve` are non-goals for extraction. Generic wake/attach
  provider plumbing remains native only when vendor-neutral; Codex-specific command construction,
  resume/profile/account policy, and workflow UX cross the reviewed external provider boundary.
- Each maw-rs slice runs isolated `scripts/gate.sh quick` and unpiped/non-PTY `scripts/gate.sh full`
  before merge. The first #963 Spec Kit scaffold is a one-time disclosed docs/generated exception:
  it contains no product code, separates copied/generated from authored lines, and requires explicit
  reviewer approval. That exception never carries into implementation slices.
- The delivery order is: Spec Kit review; narrow host/runtime contracts; external artifact build and
  committed-byte acceptance; fresh downstream core cutover; inert native deletion; independent
  review and exact-tree release verification. Child issues must record every merged SHA, artifact
  SHA-256, gate log, behavior approval, and manual closure state.

## Governance

This constitution supersedes convenience. Every spec, plan, and task set is checked against it;
a violation must be justified in writing in the artifact that violates it, or the artifact changes.

Amendments require a commit that states what changed and why, and they apply going forward — they
never retroactively bless work already merged.

Where this document and `CLAUDE.md` disagree, `CLAUDE.md` wins for repo mechanics and this document
wins for specification discipline. Neither overrides an explicit instruction from the repository
owner.

**Version**: 1.2.0 | **Ratified**: 2026-08-20 | **Last Amended**: 2026-08-20
