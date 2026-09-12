# Wrapped-agent pane provenance (#924)

Status: proposal; no runtime trust grant or detection change is implemented here.

## Decision

Keep the explicit agent-command matcher as the conservative fallback. Never add
Node, Bun, Deno, Python, npx or uv to an agent allowlist merely because they can
host an agent. A pane label or a user-controlled argv0 is not provenance.

The native CLI owns engine identification and lifecycle provenance. maw-tmux
owns only generic pane/server identity DTOs and injected process observations;
it must not depend on a vendor-specific command catalog. The browser and guest
plugins consume display projections, not an authority-granting writable record.

## Record and verification

A host-owned launch record binds all of:

- local node identity and boot generation;
- tmux server process identity/generation and pane ID;
- launched engine process PID plus OS-observed process start generation;
- resolved executable identity and declared engine/provider identity;
- launch event generation and record format version.

A PID alone, `kill -0`, window title, directory, session UUID or pane option
is insufficient. Store records under the native host's private state directory,
with atomic replacement, owner-only permissions and symlink refusal. A pane
option may carry a lookup hint but may never be the source of truth.

On every provenance use, reobserve the server and engine process, generation,
executable and pane association. Missing, inaccessible, ambiguous or mismatching
observations produce **unknown**, never agent=true. An exited process, exec to
another executable, pane reuse or PID reuse invalidates the record. If an
engine intentionally replaces its process, only a native verified lifecycle
transition may issue a replacement record; inheritance is not automatic.

This protects routing correctness against stale or guest-controlled metadata.
It is not a security boundary against a malicious process with the same OS
user privileges that can edit private state or control that user's tmux server.

## Launch and resume coverage

| Path | Required event / verification |
|---|---|
| wake: new window | Record only after engine process is positively observed in the created pane. |
| wake: reused window | Invalidate old launch generation; verify the newly launched engine. |
| deferred attach | Revalidate at attach time; the earlier wake plan is not evidence of a live engine. |
| swarm | Issue separate records per successfully observed launch, not one batch-wide boolean. |
| work / workon | Use the same native issuance/verifier, including reused panes and failed launches. |
| self-pane | Require actual process-to-pane association; cwd or current task text cannot authorize it. |
| team resume | Revalidate a preserved record; issue anew only for a newly observed launch. |
| manual/pre-existing pane | No automatic provenance. Use conservative matcher; explicit native adoption must verify the process. |

A common native issuance/verifier must serve these paths so wrappers do not
implement subtly different trust rules. No missing path may default to trusted.

## Restart, recovery and federation

Boot or tmux-server generation changes invalidate prior records. Never recover
trust by matching a reused pane number or session display name. After restart,
perform explicit verified adoption or fall back to conservative detection.

Remote records are verified by the owning host using its own process probes;
local PID checks cannot validate a remote claim. A remote display observation
must be authenticated, node-bound, generation-bound and freshness-bounded
through existing federation transport. It does not independently authorize
sending commands: the owning host must revalidate immediately before mutation.
Disconnection, replay or unavailable observations remain unknown. Do not expose
process credentials or full command arguments in the display projection.

## Required implementation evidence before closing #924

Use injected process/server probes, a fake private record store and fake clock;
tests must not depend on the developer's live tmux server.

| Case | Expected result |
|---|---|
| Matching server/pane/PID/start/executable and native launch record | Recognized wrapped agent. |
| Bare node/bun process, even with an agent-looking pane tag | Unknown/non-agent. |
| PID reused with a different start generation | Reject provenance. |
| Same PID after executable replacement | Reject provenance. |
| Exited process or failed/inaccessible probe | Unknown; no automatic mutation. |
| Reused pane under another server generation or reboot | Reject provenance. |
| Stale/forged lookup tag, missing record, symlink or bad record owner | Reject provenance. |
| Every launch/resume path in the table above | Same verifier invoked; missing wiring fails the test. |
| Remote forged/replayed/stale observation or wrong node | Reject provenance. |
| Valid explicit agent-command fallback | Existing behavior preserved. |

Implementation PRs must first provide red tests for stale records and all
lifecycle call sites, then green injected-probe coverage and the normal gates.
This proposal adds no generic argv0 exceptions, new plugin capabilities,
credential handling or unauthenticated federation shortcuts. Platform-specific
process identity adapters must be verified before enabling their records.
