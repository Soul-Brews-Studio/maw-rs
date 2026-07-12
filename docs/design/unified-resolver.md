# Unified target resolver

Status: proposed

Epic: [#318](https://github.com/Soul-Brews-Studio/maw-rs/issues/318)

## Problem

`attach`, `wake`, `hey`, `locate`, and fleet squad verbs resolve the same human names
with different inventories, normalization, and tie-breaks. A session prefix, oracle
suffix, squad alias, or peer mapping can therefore work in one verb and fail in another.
Adding a namespace requires editing every verb, which guarantees future drift.

The repository already has part of the foundation:

- #327 added typed, pure matching in `maw-matcher`;
- #329 and #334 wired typed candidates into parts of wake and attach;
- #296 (first fixed in #310, with a follow-up in flight) demonstrated that `81-kru32`
  and `kru32` are one identity even though callers must still report the session name
  and oracle handle separately.

The remaining problem is not another matcher. It is one candidate inventory and one
resolution contract, with verb-specific action policy at the edge.

## Architecture

### 1. Pure resolver, I/O-aware catalog builder

`maw-matcher` remains a leaf crate. It accepts a query, a deterministic candidate list,
and a policy; it performs no filesystem, tmux, config, network, or clock access.

The CLI builds one `ResolverSnapshot` per invocation from:

1. local live tmux sessions and windows;
2. flat fleet session snapshots, classified live or sleeping;
3. fleet squad rosters;
4. the oracle registry/manifest cache;
5. local ghq repositories;
6. configured peers, agent mappings, and explicit route aliases.

Collection order does not select a winner. All sources are collected, de-duplicated by
typed canonical identity, and sorted before matching. This prevents an early filesystem
hit from hiding a better exact live or registry match.

Each candidate carries only non-secret resolution metadata:

```text
Candidate {
  id, kind, canonical_name, aliases, provenance, live_state, node
}
```

`kind` is one of `live-session`, `sleeping-registry`, `fleet-squad`, `oracle`,
`repo`, `window`, or `peer`. Domain payloads such as paths, URLs, and roster objects stay
in caller-owned maps keyed by `id`; the pure resolver must not become an I/O layer.

### 2. One normalization trace

The resolver keeps the raw query for display and execution, then derives comparison
forms in one place:

1. trim and lowercase for comparison only;
2. retain the complete name;
3. parse a leading numeric `NN-` session prefix with `maw-identity`;
4. derive forms with and without the terminal `-oracle` suffix;
5. add explicit aliases supplied by the catalog source.

Lossy forms never become tmux targets, paths, peer URLs, or output identities. The
resolution result includes the matched alias and normalization step so ambiguity and
diagnostic output can explain why a candidate matched.

The #296 normalization fix is the model to generalize: prefixed and unprefixed locate
input must select the same entity, while JSON keeps distinct `session` and `handle`
fields. Its follow-up implementation should replace, not duplicate,
`locate_normalized_names` with the shared identity/matcher helpers.

### 3. Unified search order

Structured forms are classified first: explicit paths/URLs, `session:window`, and
`node:agent` retain their grammar and resolve their name components through the catalog.
Unstructured names use this global match ladder:

1. raw exact canonical name or declared alias;
2. normalized exact identity;
3. live session/window segment match;
4. sleeping registry/oracle segment match;
5. numeric hash-slot owner match;
6. bounded prefix/substring fuzzy match.

Match quality is primary. Within the same quality, the verb policy ranks candidate
kinds. If multiple candidates still tie, the result is ambiguous; alphabetical order is
only for stable picker display and must never silently choose a target.

The resolver returns `Match`, `Ambiguous`, or `None`, plus the ranked candidates and
normalization trace. It never attaches, wakes, sends, prompts, or mutates state.

### 4. Verb equals policy

A `ResolvePolicy` defines accepted kinds, kind preference, allowed match levels, and
bridges to another action:

| Verb | Native preference | Bridge behavior |
| --- | --- | --- |
| `attach` | live session, window | sleeping/oracle/repo -> `wake --attach`; squad -> `fleet wake`; peer -> existing remote attach |
| `wake` | sleeping registry, oracle, repo, live | live -> reuse/select; squad -> `fleet wake`; explicit peer syntax keeps federation routing |
| `locate` | oracle, registry, repo, live | no action; enrich the chosen identity with path/session/site data |
| fleet squad verbs | fleet squad | member handles resolve through the snapshot; mutating verbs disallow fuzzy matches |
| `hey` | window, live session, peer | preserve local/peer route semantics; no implicit wake until inbox/delivery behavior is specified |

Exact non-native matches bridge before fuzzy native matches. Bridges are action plans,
not nested command strings. The CLI renders the plan, requires TTY confirmation for a
new side effect, honors `--yes`, and executes through the existing in-process handler.
Destructive consumers revalidate the chosen target against fresh state before mutation.

`hey` remains last because its `me`, `node:agent`, writable-pane, peer URL, auth, and
inbox semantics are richer than name matching. The unified resolver replaces its local
alias search, not its transport or authorization policy.

## Staged rollout

Each stage is independently reviewable and keeps production behavior explicit.

1. **Contract fixtures:** expand the typed resolver corpus with every symptom in #318,
   including cross-kind exact collisions and structured query components.
2. **Normalization:** land the #296 work and remove duplicate locate normalization. Add
   identity fixtures for numeric prefixes, `-oracle`, case, and degenerate names.
3. **Snapshot + shadow mode:** build the full deterministic catalog once. For each verb,
   run old and new pure resolution, execute only the old result, and record redacted
   disagreements in tests/audit output.
4. **Locate first:** switch this read-only verb to the new result, preserving its JSON
   schema and text/exit-code goldens. It is the safest proof that catalog enrichment and
   prefixed identity work end to end.
5. **Attach:** replace its private candidate builder with the shared snapshot; retain the
   already shipped sleeping-entry bridge and picker behavior.
6. **Wake:** collapse registry/repo early-return chains into one policy, while keeping
   explicit path and GitHub-slug overrides authoritative.
7. **Fleet squads:** use the squad policy for roster lookup and the same snapshot for
   member-to-session resolution. Require exact resolution for mutations.
8. **Hey:** adapt `maw-routing` structured syntax around the unified local resolver;
   remove the old alias chain only after local and peer fixture matrices agree.
9. **Cleanup:** delete verb-local normalization/matching only after all consumers use the
   shared contract; keep small compatibility adapters for stable output shapes.

## Fixture and parity strategy

- Copy relevant maw-js resolver fixtures into a shared JSON corpus; never rewrite or
  delete them merely to accept Rust behavior.
- Add policy fixtures containing query, candidate snapshot, policy, typed outcome, and
  planned bridge. The leaf result must be byte-stable and independent of discovery order.
- Keep each verb's existing stdout, stderr, JSON, and exit-code goldens around its adapter.
  Internal typed results may change before user-visible output does.
- Shadow mode has no side effects and never becomes an automatic fallback. Differences
  need an explicit fixture and rationale before a verb switches.
- Migrate one verb per PR. A leaf change cannot simultaneously update verb goldens.
- During dual-engine operation, intentional divergence from maw-js is a named fixture
  with an issue reference; all unrelated maw-js fixtures remain frozen.

## Open questions

1. What fuzzy threshold is safe enough for non-mutating verbs, and should attach allow it
   without a picker?
2. Should peer aliases outrank an equally exact local name, or always require `node:name`?
3. How long may a `ResolverSnapshot` be reused before live-state revalidation?
4. Should the normalization trace become a stable `--json` diagnostic surface?
5. When attaching to a squad, does confirmation wake all members or present a second
   member/session picker?

## Current implementation touchpoints

- `crates/maw-matcher/src/typed_resolver.rs`: pure kinds, normalization, and ranking.
- `crates/maw-cli/src/core_impl/attach.rs` and `wake.rs`: partial typed adapters.
- `crates/maw-cli/src/core_impl/locate.rs`: duplicate normalization to retire.
- `crates/maw-cli/src/core_impl/fleet_roster.rs`: private squad matching to retire.
- `crates/maw-routing`: structured `hey` routing to wrap, not replace wholesale.
