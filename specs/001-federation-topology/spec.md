# Feature Specification: Fleet Enrollment and Reachability

**Feature Branch**: `agents/spec-federation`

**Created**: 2026-08-20

**Status**: Draft — empirical results landed 2026-08-20; three owner decisions outstanding

**Input**: User description: "i think we have wrong design so we join federation to federation if i
have 10 machine it should be paired 10x10 times? 1 to 1 it hard? se we should join a server like we
join game server / join discord" — plus the follow-up crosscheck: "can we do relay federation with
things we already have, WITHOUT change or redesign?"

## Context: What Is Actually True Today

Established by direct evidence during the 2026-08-20 audit. Every claim carries its label.

- **Enrollment is O(N²) and one-directional.** Pinning is per-direction, so a 10-node mesh costs
  **N(N-1) = 90** `maw peers add` invocations spread across 10 machines. Adding an 11th costs 20
  more, run on 11 different machines. `[source-derived]`
- **The pinned "pubkey" is a shared secret, not a public key.** `GET /api/identity` publishes
  `"pubkey": deps.peer_key` (`serve_identity.rs:169`), read straight from the local `peer-key` state
  file. The verify path uses that same value as an HMAC key —
  `verify_hmac_sig(cached, &payload, &signed.v3_sig)` (`request_verify.rs:203`). The repo's own
  documentation says so: `docs/reference/wire-protocol.md:132`. `[observed]`
- **Confirmed by measurement**, not only by reading:
  `curl -s http://127.0.0.1:3456/api/identity | jq .pubkey` returns byte-for-byte the contents of
  `~/.config/maw/peer-key`. Served unauthenticated. `[observed]`
- **Therefore any paired node can already impersonate its peers** to any third node that trusts the
  same pin. `[source-derived]`
- **A genuine Ed25519 path exists and is inert.** Nothing populates the pin store it depends on, so
  it always rejects with `ed25519-pin-missing`. `[observed]`
- **The daemon binds every interface.** `DEFAULT_SERVE_BIND = "0.0.0.0"` (`serve.rs:26`), confirmed
  listening on `0.0.0.0:3456`. "WireGuard is the perimeter" is false as deployed. `[observed]`
- **A peer has exactly one address.** `PeerRecord.url: String`. No failover. Health fields exist but
  have zero references in the send/route path — they are display-only. `[source-derived]`
- **This is what actually broke on 2026-08-19**: every peer URL pointed at down WireGuard addresses
  while the LAN path was fine. Nothing retried, because there was nothing to retry to. `[observed]`
- **Signatures do not bind the destination.** The v3 payload is
  `"{METHOD}:{path}:{timestamp}:{body_hash}:{from}"` where `from` is the *sender's* identity; the
  destination `peer_url` builds the request URL and is never passed into signing
  (`pair_consent.rs:544-556`, `reqwest_peer_http_client.rs:355-378`). Relaying is therefore
  signature-transparent. `[source-derived — empirical confirmation pending, see FR-024]`
- **A join-code API already exists and is orphaned.** Six routes under `/api/workspace/*`
  (`serve_workspace.rs`), a byte-for-byte wire-parity port of maw-js. `create` mints a workspace
  token and a 6-hex-char join code; `join` returns the **same shared token to every member**.
  In-memory only, no CLI surface. `[observed]`
- **Its expiry is decorative.** `join_code_expires_at` is computed, stored, and returned, but never
  compared against the clock. Codes are also never consumed. A leaked join code works forever and
  is unlimited-use. `[observed]`
- **A hub was proposed before and dropped.** Issue #10 "Port maw-hub from maw-js to maw-rs" was
  closed the day it was opened with a stub body. Separately, maw-js's `HubTransport` — hub message
  *delivery* — was deliberately removed in v26.6.13. `[observed]`
- **Issue #290** already designs group membership (`maw fleet join <fleet> --code <c>`) reusing the
  existing `PairCodeStore` 6-char TTL mechanism. Closed, unimplemented. `[observed]`

## User Scenarios & Testing *(mandatory)*

Priority reflects value to the fleet owner. **Build order is deliberately different** — see
"Implementation Order" — because US2 is unblocked by every open decision and repairs a failure that
has already happened.

### User Story 1 - Join a fleet with one command (Priority: P1)

An operator with a new machine runs one command on that machine, supplies a code, and the machine
can exchange messages with every other machine in the fleet. They do not open a shell on any other
machine, and they do not need every other machine to be online at that moment.

**Why this priority**: This is the request. It converts enrollment from N(N-1) commands across N
machines into 1 command on 1 machine, and removes the failure where an unreachable peer is skipped
and then forgotten.

**Independent Test**: Stand up a fleet of three nodes. Add a fourth using only commands executed on
the fourth machine. Verify it exchanges messages with all three, and that no shell was opened on
nodes 1-3. Verify enrollment still succeeds when one of the three is powered off.

**Acceptance Scenarios**:

1. **Given** a fleet of N enrolled nodes and a valid unexpired join code, **When** an operator runs
   the join command on a new machine, **Then** that machine can send to and receive from every
   enrolled node, and the count of commands run on other machines is zero.
2. **Given** a join code that has passed its expiry, **When** join is attempted, **Then** it is
   refused with a distinct error naming expiry as the cause.
3. **Given** a join code that has already been redeemed, **When** it is presented again, **Then** it
   is refused — unless the code was explicitly issued as multi-use.
4. **Given** one enrolled node is powered off during enrollment, **When** the new machine joins,
   **Then** enrollment succeeds, and the offline node accepts the new member once it returns without
   any manual step.

---

### User Story 2 - Reach a peer over whichever path is actually up (Priority: P2)

A peer reachable at several addresses — LAN, WireGuard, hostname — is reached over one that works.
When one path is down, traffic uses another without the operator editing configuration.

**Why this priority**: This is the failure that actually occurred. It is independent of every trust
and topology decision, and it is the smallest change in this specification.

**Independent Test**: Configure a peer with two addresses, break the first, send a message. Verify
delivery over the second and a log line naming both the attempt and the fallback.

**Acceptance Scenarios**:

1. **Given** a peer with two or more known addresses and at least one reachable, **When** a message
   is sent, **Then** it is delivered without operator intervention.
2. **Given** a peer whose first-listed address is unreachable, **When** a message is sent, **Then**
   the failure over that address does not surface as a delivery failure to the caller.
3. **Given** a peer with no reachable address, **When** a message is sent, **Then** it fails with an
   error enumerating every address tried and the reason each failed.
4. **Given** recorded health information for a peer, **When** an address is selected, **Then** the
   selection demonstrably uses that information.

---

### User Story 3 - Remove a machine and have it lose access (Priority: P3)

An operator removes a machine from the fleet. Within a bounded, stated interval, that machine can no
longer act as a member, and it cannot impersonate any member it was previously enrolled alongside.

**Why this priority**: Requested as "kick". It cannot be delivered on the current key model — a
departing node keeps material that lets it sign as its former peers — so it depends on US4.

**Independent Test**: Enroll three nodes, revoke one, and confirm from the revoked machine that both
its own sends and any attempt to sign as a former peer are refused, within the stated interval.

**Acceptance Scenarios**:

1. **Given** an enrolled node, **When** the owner revokes it, **Then** within the stated propagation
   interval no remaining node accepts a message from it.
2. **Given** a revoked node, **When** it attempts to sign as a node it was previously enrolled
   alongside, **Then** every remaining node refuses.
3. **Given** the enrollment authority is unreachable, **When** revocation is issued, **Then** the
   system states plainly that propagation is pending rather than reporting success.

---

### User Story 4 - Nodes cannot impersonate one another (Priority: P3)

Enrolling in a fleet never requires a node to transmit material that lets a recipient act as it.

**Why this priority**: It is the precondition for US3, and it is what makes the roster of US1 worth
distributing. Today it is false, and it is false in the direction that matters: the value published
under the name `pubkey` is the private signing secret.

**Independent Test**: Capture everything a node transmits during enrollment and normal operation.
Confirm no captured value permits forging that node's signature.

**Acceptance Scenarios**:

1. **Given** a node's published identity, **When** a third party obtains it in full, **Then** that
   party cannot produce a signature any node accepts as coming from the published node.
2. **Given** an enrolled peer, **When** it uses everything it holds about another peer, **Then** it
   cannot sign as that peer.
3. **Given** a field in the published identity, **When** its name implies public material, **Then**
   its contents are public material.

---

### User Story 5 - Federate two fleets as units (Priority: P4)

Two fleets, each with its own members, federate through a single link rather than pairing every
member of one with every member of the other.

**Why this priority**: Not needed for one owner and ten machines, but the addressing decision it
forces is cheap now and expensive to retrofit.

**Independent Test**: Two fleets of two nodes each. Establish one federation link. Verify a member of
one addresses a member of the other, and that the number of enrollment operations is one, not four.

**Acceptance Scenarios**:

1. **Given** two fleets, **When** one federation link is established, **Then** members address each
   other across it without individual pairing.
2. **Given** cross-fleet addressing, **When** two fleets contain a node of the same name, **Then**
   the two remain distinguishable.

---

### Edge Cases

- Two nodes join concurrently with the same code — does the roster converge?
- A node is revoked while offline, then returns after the propagation window.
- The enrollment authority is unreachable for longer than the roster's usable lifetime.
- A node's clock is skewed beyond the signature timestamp tolerance during enrollment.
- The authority is itself revoked or replaced.
- A node holds a roster listing an address it can no longer route to for every peer — the
  2026-08-19 outage, exactly.
- Enrollment attempted from a network where the authority is reachable but no peer is.

## Requirements *(mandatory)*

### Functional Requirements — Enrollment (US1)

- **FR-001**: The system MUST allow a machine to join a fleet using commands executed only on that
  machine.
- **FR-002**: The system MUST NOT require every existing member to be reachable at the moment a new
  member joins.
- **FR-003**: A member that was offline during another's enrollment MUST accept that member on
  return, with no manual step.
- **FR-004**: A join code MUST be refused after its stated expiry, and the refusal MUST name expiry
  as the cause. *(Today the expiry field is written and never read — `serve_workspace.rs`.)*
- **FR-005**: A single-use join code MUST be refused on second presentation. Multi-use codes MUST be
  explicitly requested at issue time.
- **FR-006**: The system MUST record which member each join code admitted.
- **FR-007**: Enrollment MUST NOT require an operator to hand-copy key material between machines.

### Functional Requirements — Reachability (US2)

- **FR-010**: A peer MUST be able to hold more than one address.
- **FR-011**: Sending MUST try alternative addresses when an address fails at the transport level.
- **FR-012**: A send that succeeds over any address MUST NOT surface a failure to the caller.
- **FR-013**: A send that exhausts all addresses MUST report every address tried and why each failed.
- **FR-014**: Recorded peer health MUST influence address selection. *(Today these fields have zero
  references in the send path.)*
- **FR-015**: Address preference MUST be deterministic and inspectable.
- **FR-016**: Transport-level failover MUST NOT retry a request that was refused on authentication.

### Functional Requirements — Membership and Revocation (US3)

- **FR-020**: The owner MUST be able to revoke a member.
- **FR-021**: Revocation MUST take effect across remaining members within a stated bounded interval.
- **FR-022**: The system MUST distinguish "revocation propagated" from "revocation recorded but not
  yet propagated", and MUST NOT report the second as the first.
- **FR-023**: Loss of the enrollment authority MUST NOT prevent already-enrolled members from
  communicating. Degradation MUST be to present behaviour, never to silence.
- **FR-024** *(resolved 2026-08-20 by experiment)*: Message relay through a third node requires no
  protocol change. Confirmed empirically against three isolated daemons: a signed request is not
  bound to its destination and verifies at any node that trusts the sender (**E3, CONFIRMED**).
  Relay therefore MAY be adopted, and MUST remain fallback-only after direct paths fail.
- **FR-025**: Adopting relay MUST NOT ship before replay protection. The same property that makes
  relay free makes replay free: there is no nonce or seen-set, and the acceptance window is ±300s
  (**E4**). A captured request is therefore replayable to any node in the fleet for up to ten
  minutes. Formalizing the relay path formalizes an accidental property, and MUST close it in the
  same change.
- **FR-026**: Signature verification MUST authenticate the claimed agent identity, not only the
  node. Forgery of the oracle identity currently succeeds because only the node key is
  authenticated (**E2**) — the same defect scope as issue #798.

### Functional Requirements — Identity (US4)

- **FR-030**: A node MUST NOT publish material that permits a recipient to sign as that node.
- **FR-031**: A published field whose name implies public material MUST contain public material.
- **FR-032**: Verification MUST NOT depend on the verifier holding the signer's signing secret.
- **FR-033**: Key rotation by one member MUST NOT require manual action on every other member.
- **FR-034**: The system MUST refuse, with a distinct and actionable error, rather than fall back to
  a weaker check when strong verification is unavailable.

### Functional Requirements — Exposure

- **FR-040**: Network-facing services MUST default to the most restrictive binding that supports
  their documented use, and any wider binding MUST be an explicit operator choice.
- **FR-041**: Unauthenticated endpoints MUST NOT disclose material used to authenticate anything.

### Functional Requirements — Addressing (US5)

- **FR-050**: An address MUST distinguish agent, node, and fleet.
- **FR-051**: Identically named nodes in different fleets MUST remain distinguishable.
- **FR-052**: Existing single-fleet addresses MUST keep working unchanged.

### Key Entities

- **Fleet**: a named set of nodes under one owner; the unit that federates with another fleet.
- **Node**: one machine running the daemon; hosts many agents; holds one identity.
- **Agent**: a working context on a node; the endpoint an operator addresses.
- **Roster**: the fleet's authoritative mapping of node → identity → addresses, with a validity
  window; the artifact enrollment distributes and revocation edits.
- **Join Code**: a short-lived, optionally single-use credential admitting one node to a fleet.
- **Enrollment Authority**: whatever issues codes and signs the roster — a node in hub mode, or a
  signed file. Required for joining and revoking; **not** required for members to talk.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Adding one machine to a fleet of ten requires commands on exactly one machine. Today:
  20 commands across 11 machines.
- **SC-002**: Enrollment succeeds while at least one existing member is powered off.
- **SC-003**: A peer with at least one reachable address among several is reached without operator
  intervention, in 100% of trials.
- **SC-004**: A failed delivery names every address attempted and a distinct reason for each.
- **SC-005**: Rotating one member's key requires action on exactly one machine and propagates
  without hand-editing any other.
- **SC-006**: Revocation takes effect on every reachable member within a stated interval, and that
  interval is documented and tested.
- **SC-007**: No value a node transmits during enrollment or operation permits any recipient to
  produce a signature accepted as coming from that node.
- **SC-008**: With the enrollment authority unreachable, previously enrolled members continue
  exchanging messages for a documented duration with no degradation.
- **SC-009**: Federating two fleets of five requires one enrollment operation, not twenty-five.
- **SC-010**: Every currently working `maw hey` and `maw peek` invocation keeps working unchanged.

## Assumptions

- One owner, roughly ten machines, not multi-tenant and not internally adversarial. A member is
  trusted to be honest but may be lost, stolen, or decommissioned — which is why US3 exists.
- Machines are reachable over one or more private paths (LAN, WireGuard); no public exposure is
  intended, though the current default binding does not enforce that.
- Existing pairwise pins remain valid. Enrollment is additive: it writes into the same store manual
  pairing writes into, and manual pairing stays as bootstrap and escape hatch.
- The enrollment authority runs the same binary as every other node, distinguished by configuration
  rather than by being a separate product. Any node can take the role.
- Traffic remains direct between members by default. Relay, if adopted at all, is a fallback after
  direct paths fail — never the primary path.
- Existing message and peek semantics are unchanged; this specification adds enrollment,
  reachability, membership, and identity, and changes no user-facing verb behaviour.

## Owner Decisions Required

These are the fleet owner's to make; the specification is deliberately incomplete without them.

1. **[NEEDS CLARIFICATION: authority form]** Roster distributed as a signed file synced by existing
   means, or served by a node in hub mode? Both give one-command enrollment. The file needs no
   running service and cannot go down; the hub reuses code that already exists but is orphaned and
   in-memory.
2. **[NEEDS CLARIFICATION: identity remediation]** FR-030 through FR-034 describe fixing the
   published-secret problem, currently tracked by issue #877, closed as `not_planned` with the
   comment "This is not a security fix". Either that decision is revisited, or this specification
   drops US3 and US4 and states that any node can impersonate any peer by design.
3. **[NEEDS CLARIFICATION: default binding]** FR-040 changes the default from `0.0.0.0`. This is a
   one-line default plus a flag, and it independently reduces the blast radius of everything above —
   but it will break any current caller that reaches a daemon over a non-loopback interface without
   passing the flag.

## Implementation Order

Priority above is by value. Build order is by dependency and risk:

| order | stories | blocked on |
|---|---|---|
| 1 | US2 (reachability) | nothing — ships regardless of every decision above |
| 2 | US1 (enrollment) | decision 1 |
| 3 | US4 (identity) | decision 2 |
| 4 | US3 (revocation) | US4 — revocation is not deliverable before it |
| 5 | US5 (fleet-to-fleet) | US1; addressing choice should be made now even if built later |

US2 first is deliberate: it is the only story that repairs a failure that has already happened, and
it is unblocked by every open question.
