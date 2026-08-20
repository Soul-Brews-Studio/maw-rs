# Implementation Plan: Fleet Enrollment and Reachability

**Branch**: `agents/spec-federation` | **Date**: 2026-08-20 | **Spec**: `./spec.md`

## Summary

The headline finding of planning: **no new crate is required, and almost nothing needs to be
built from scratch.** Every concern in the specification already has a home in this workspace,
and in three cases the primitive already exists and is simply not wired to anything. The work is
connection and correction, not construction.

## Technical Context

**Language/Version**: Rust, workspace edition 2021, toolchain pinned by `rust-toolchain.toml`
**Primary Dependencies**: existing workspace crates only — `maw-auth`, `maw-peer`, `maw-hub`,
`maw-transport`, `maw-cli`
**Storage**: JSON state files under the XDG state path (`peers.json`, `peer-key`, workspace configs)
**Testing**: `cargo test --workspace --locked --no-fail-fast`; observable behaviour additionally
validated against maw-js JSON fixtures
**Target Platform**: Linux and macOS hosts running the daemon
**Project Type**: CLI + long-running local HTTP daemon
**Constraints**: ≤250 authored changed lines per PR excluding lockfiles; `unsafe_code` forbidden;
clippy pedantic as errors
**Scale/Scope**: ~10 nodes, one owner, tens of agents per node

## What Already Exists (the reason there is no new crate)

| concern | home | state | evidence |
|---|---|---|---|
| join code: mint, shape, normalize, redact, expire | **`maw-auth::PairCodeStore`** | **exists, complete** | `generate_pair_code_from_bytes(&[u8])` — injectable entropy; `pair_api_status_plan(store, code, now_ms)` — injectable clock |
| hub/workspace client config | **`maw-hub`** | exists, 207 lines | `WorkspaceConfig{id,hub_url,token,shared_agents}`, `load_workspace_configs`, plus unused `HEARTBEAT_MS`/`RECONNECT_BASE_MS`/`RECONNECT_MAX_MS` |
| peer record, health, probe error | **`maw-peer::PeerRecord`** | exists | `url: String` (single), `last_seen`, `last_error: ProbeLastError` |
| signing / verification | **`maw-auth`** | exists | `build_from_sign_payload`, `sign_ed25519_headers_at` (unused) |
| workspace HTTP routes | **`maw-cli::serve_workspace`** | exists, orphaned | six routes, wire-parity with maw-js, no CLI surface |
| probe with distinct failure reasons | **`maw-peer`** | exists | `maw peers probe` already distinguishes REFUSED from TIMEOUT |

**`maw-hub` is a rev-pinned external crate** (`maw-crates@e46932081dfd…`), so any change there is a
two-repo operation: upstream PR into `maw-crates`, merge, capture the squash-merge SHA, then a
downstream cutover pinning that immutable SHA. Never a moving head.

### The bug this table explains

`serve_workspace.rs` computes `join_code_expires_at`, returns it to the client, and never compares
it to the clock — so the join code is unlimited-use and permanent. `maw-auth::PairCodeStore` already
implements expiry correctly, with an injectable clock. **The defect exists because the route rolled
its own code path instead of using the primitive next door.** The fix is deletion plus delegation,
not new logic.

## Constitution Check

| principle | how this plan complies |
|---|---|
| I. Green is a hypothesis | every phase ships with a red-first test and an independent reviewer asked to refute named angles |
| II. Verify the artifact | US2 acceptance is measured against a live daemon, not a mock — the 2026-08-20 probe run is already the model |
| III. Fail closed | FR-034 forbids weak-check fallback; FR-022 forbids reporting un-propagated revocation as done; FR-012/FR-013 make transport failover explicit rather than silent |
| IV. Lowest layer that can hold it | pure logic lands in leaves (`maw-auth`, `maw-peer`); I/O stays in `maw-cli`. **This principle is what produced the "no new crate" answer** |
| V. Small and reversible | each phase below is scoped to fit ≤250 lines; phases that cannot are split and say so |

**No violations to track.** The plan adds no crate, no dependency, and no new external service.

## Project Structure

```
specs/001-federation-topology/
├── spec.md          # what and why (technology-agnostic)
├── plan.md          # this file
└── tasks.md         # generated next

crates/
├── maw-auth/        # EXTEND: reuse PairCodeStore for workspace join; publish only public key material
├── maw-peer/        # EXTEND: PeerRecord gains an address list; selection policy is pure and testable
└── maw-cli/
    ├── core_impl/serve_workspace.rs   # REPLACE the hand-rolled code path with PairCodeStore
    ├── core_impl/serve_identity.rs    # FIX: stop publishing the signing secret as "pubkey"
    └── core_impl/serve.rs             # DEFAULT: restrictive bind; hub-mode flag

(external) maw-crates/crates/maw-hub/  # EXTEND: roster type + validity window — two-repo cutover
```

**Structure Decision**: no new crate. Each concern goes to the lowest existing layer that can hold
it. A `maw-fleet`-style crate would be an organizational-only library — it would collect code by
feature name rather than by layer, which Principle IV exists to prevent.

The single open placement question is the **roster type**. It is pure data plus a validity window,
so it belongs in a leaf. Two candidates:

- **`maw-hub`** — conceptually correct (the hub owns membership) but external, so every iteration
  costs a two-repo cutover.
- **`maw-peer`** — in-repo and already owns the peer store the roster writes into, but widens a
  crate currently scoped to a single peer rather than a fleet.

Recommendation: **`maw-peer` while the design is still moving**, and extract to `maw-hub` once it
stops changing. Iterating across two repos on an unsettled type will cost more than the later move.
This is a reversible choice and is flagged rather than assumed.

## Phases

Ordered by dependency, matching the specification's Implementation Order.

### Phase 1 — Reachability (US2) · blocked on nothing

`PeerRecord` gains an ordered address list, keeping `url` working as the single-address case so no
existing `peers.json` breaks. Selection is a **pure function** of (addresses, health, last error) →
ordered attempt list, unit-tested with no I/O. The send path tries in order; transport failure moves
to the next address, an authentication refusal does not (FR-016). Exhaustion reports every address
and its distinct reason, reusing the REFUSED/TIMEOUT distinction `maw peers probe` already draws.

**Live acceptance case, captured 2026-08-20**: peer `blackbox` is pinned at `192.168.1.185:3467`,
which refuses. The same host answers on `192.168.1.185:3456` with
`{"ok":true,"port":3456,"server":"local","source":"maw-rs"}` and identifies as node `black`. A
running peer reads as dead because one string is stale. This is the regression test.

### Phase 2 — Enrollment (US1) · blocked on owner decision 1

Delete the hand-rolled join-code path in `serve_workspace.rs` and delegate to
`maw-auth::PairCodeStore`, which already implements expiry and shape validation against an
injectable clock. This closes the decorative-expiry defect as a side effect of the correct design
rather than as a separate patch. Add the roster type and the CLI surface. The roster writes into the
same store `maw peers add` writes into, so enrollment is additive and manual pairing survives as
bootstrap and escape hatch.

### Phase 3 — Identity (US4) · blocked on owner decision 2 (#877)

Separate a real signing keypair from the shared secret; publish only the public half; keep the HMAC
secret private. `sign_ed25519_headers_at` already exists and is unused; its verification path is
currently inert because nothing populates the pin store it reads. Wiring that store is the work.
Rename the published field so it stops lying about what it holds.

### Phase 4 — Revocation (US3) · blocked on Phase 3

Not deliverable earlier: while pins are shared secrets, a departing node keeps everything it needs
to sign as its former peers, so removing it from a roster takes nothing back. Roster gains a
validity window with the two-tier behaviour the specification states.

### Phase 5 — Relay (FR-024) · blocked on empirical result

Source reading says the v3 signed payload is `"{METHOD}:{path}:{timestamp}:{body_hash}:{from}"` with
no destination binding, and that `peer_url` never reaches the signing function — so relaying is
signature-transparent and needs no protocol change. Pending empirical confirmation. Note that
maw-js shipped hub delivery as `HubTransport` and deliberately removed it; that rationale should be
recovered before this phase is planned in detail. Relay remains fallback-only in every case.

## Complexity Tracking

| item | why it is unavoidable | cheaper alternative rejected because |
|---|---|---|
| two-repo cutover for `maw-hub` | the crate is rev-pinned external | vendoring it in-repo would fork a shared crate to avoid a process cost |
| dual-format `PeerRecord` (string or list) | existing `peers.json` files must keep loading | a flag-day migration would break every fleet member at once, including ones that are offline |
