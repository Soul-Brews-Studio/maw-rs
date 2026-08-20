<!--
Sync Impact Report
- Version change: 1.0.0 -> 1.0.1
- Added principles: Stable Native Kernel; Plugin Ownership; Behavior-Frozen Extraction;
  Fail-Closed Capability Boundary; Source-Proven Artifacts; Small Verified Slices
- Added sections: Architecture Constraints; Delivery Workflow
- Clarified Stable Native Kernel: native entrypoints and all non-Codex behavior remain frozen;
  explicit Codex selection may fail before mutation only when its accepted external provider is
  missing/refused, with an actionable repair error.
- Clarified Fail-Closed Capability Boundary: #963 guests use typed, injected host-operation APIs,
  not arbitrary `maw.cli.run` argv.
- Clarified the #963 scaffolding/docs PR requires an explicit generated/program-documentation
  diff-budget exception approved in review.
- Removed sections: none (template placeholders were resolved)
- Follow-up TODOs: none
-->
# maw-rs Lean-Core Constitution

## Core Principles

### I. Stable Native Kernel

The native binary MUST retain orchestration that establishes the trusted host boundary:
`maw wake`, `maw attach`/`maw a`, `maw hey`, `maw peek`, `maw serve`, plugin discovery and verification, capability
enforcement, signed/authenticated transport, and raw tmux/process/filesystem adapters.
Extraction MUST NOT move or shadow the native `wake`, `attach`, `a`, `hey`, `peek`, or `serve` entrypoints or their
orchestration state machines. Their binary fast paths, tmux effects, and every non-Codex observable
contract MUST remain frozen, and unrelated optional plugins MUST remain unable to intercept them.
With an accepted Codex provider installed, frozen Codex launch/resume behavior MUST remain at parity.
When Codex is explicitly selected and that provider is missing or refused, the only approved
extraction-specific change is a loud actionable failure before mutation; silent fallback is forbidden.
New logic belongs in the lowest layer that can own it without moving trusted host authority into a guest.

### II. Plugins Own Optional and Product-Specific Workflows

Optional workflow verbs and engine-vendor policy MUST live in installable plugins, not in the
native dispatcher. For this program that includes `bring`/`b`, `split`, `team`/`t` and its
team-only companions, and Codex-specific account, team, profile, and provider behavior. The
native kernel MAY expose a generic provider or typed host contract, but MUST NOT retain a
second product-specific implementation after cutover. Existing artifacts MUST be extended
rather than duplicated.

### III. Behavior-Frozen Extraction

Every extraction MUST begin with a source/caller/fixture inventory and failing parity proof.
The external artifact MUST reproduce the frozen argv, stdout, stderr, exit-code, JSON, safety,
and ordering contracts before native dispatch is removed. Observable maw-js fixtures MUST be
preserved or additively updated; tests MUST NOT be deleted merely to make a cutover pass. Native
and plugin implementations MUST NOT both be reachable after the atomic cutover.

### IV. Fail-Closed Capability Boundary

Guests MUST receive only narrow, typed capabilities. They MUST NOT receive raw `/proc`, auth
secrets, unrestricted filesystem paths, arbitrary process execution, or unrestricted tmux.
Missing, stale, hash-invalid, SDK-incompatible, under-capable, or malformed plugins MUST fail
loudly with actionable repair guidance. Non-idempotent host operations MUST not be retried after
an ambiguous delivery boundary. Human consent and authentication decisions remain native.

### V. Source-Proven Artifacts

Guest source and committed `plugin.wasm` live in `Soul-Brews-Studio/maw-plugins`. Every artifact
MUST be reproducible, checked by that repository's CI, and pinned by exact SHA-256 in its active
manifest. Upstream artifact work MUST merge and be verified before a fresh downstream `maw-rs`
cutover branch is created. The downstream known-verb/help/doctor surface MUST identify the exact
external package and fail safely when it is absent.

### VI. Small Verified Slices

Each PR MUST stay at or below 250 authored changed lines, excluding lockfiles and generated
`plugin.wasm`, unless a mechanical copy/deletion exception is explicitly justified. Work MUST be
test-first where behavior changes: demonstrate RED on the frozen base, then GREEN twice on focused
tests. `maw-rs` slices MUST pass `scripts/gate.sh quick`; the exact final integration tree MUST pass
`scripts/gate.sh full` before merge or promotion. Cargo and gate targets MUST be isolated under
`/mnt/nvme1`, and `ψ/` MUST never enter a diff.

Until #964's include-fragment formatting gate is merged, every PR touching
`crates/maw-cli/src/core_impl/*.rs` MUST additionally run standalone rustfmt with edition 2021 and
`--check` over those fragments and record it; a green workspace `cargo fmt --check` alone is known
not to inspect generated-include leaves.

The initial #963 Spec Kit scaffold/program-documentation PR is a one-time disclosed exception: its
PR body MUST separate generated copied lines from authored feature-document lines, report both exact
counts, contain no product code, and obtain explicit reviewer approval for the exception before merge.
The exception does not carry into implementation PRs.

## Architecture Constraints

- Workspace Rust remains edition 2021, forbids unsafe code, and treats Clippy warnings as errors.
- Raw tmux stays behind `maw-tmux` and typed native host verbs. Guests use typed capability APIs.
  The #963 team/workflow artifacts MUST NOT receive arbitrary `maw.cli.run` argv. The plugin-manifest
  crate MUST define only DTOs and injected host-operation traits; a maw-cli adapter owns wake/done,
  pane mutation, worktree, provider, and platform I/O. Versioned semantic actions use bounded inputs,
  target scoping, time/output limits, recursion denial, and explicit ambiguous-outcome results.
- The plugin loader/native host remains beside the application; #911 may extract only a pure schema
  leaf and does not authorize moving host authority into `maw-plugins`.
- `maw-tmux` remains a Rust host boundary under #910; guest extraction does not turn it into WASM.
- Generic wake/attach engine-provider plumbing may remain native only when vendor-neutral. Codex-
  specific command construction, resume/profile/account policy, and workflow UX must cross the
  reviewed external provider boundary before this program closes.
- Branches start from current `origin/alpha`, PRs target `alpha`, and published branches are never
  force-pushed.

## Delivery Workflow

1. Run Spec Kit constitution, specification, clarification, plan, tasks, and analysis before code.
2. Maintain a GitHub umbrella and bounded child ledger; each child names owner repo, non-goals,
   capabilities, parity fixtures, paired-PR order, and exact gates.
3. Land prerequisite typed host/runtime contracts independently of product workflow code.
4. Build and verify the external artifact while native dispatch still shadows it.
5. Cut over dispatch atomically in a fresh downstream branch; prove missing-plugin and refusal paths.
6. Delete inert native implementation and tests only after external parity is independently attested.
7. Re-run wake/attach focused regression tests and the full gate at every ownership boundary.
8. Write back hard-won boundary decisions to the spec, issue, and relevant repo guide immediately.

## Governance

This constitution is authoritative for issue #963 and its child work. A change to a MUST rule
requires a documented amendment, explicit human approval, a migration impact note, and a semantic
version bump: MAJOR for incompatible principle changes, MINOR for new/materially expanded rules,
and PATCH for clarifications. Every plan, task list, PR body, and review MUST state constitution
compliance or name the approved exception. The maw-rs `AGENTS.md` contract and repository guides
remain binding where they are stricter; conflicts are resolved in favor of the stricter fail-closed
or verification requirement.

**Version**: 1.0.1 | **Ratified**: 2026-08-20 | **Last Amended**: 2026-08-20
