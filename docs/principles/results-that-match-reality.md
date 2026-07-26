# Results that match reality

> Distilled from one long federation-map debugging day (2026-07-25/26), where six
> separate bugs turned out to be the same disease. Written down because the pattern
> is worth more than any one fix: an unwritten lesson dies at the next compact.

## The disease: a result that does not carry its cause

A subsystem that reports an **outcome** without reporting **why** can lie. It fooled
us in four distinct shapes in a single day, and every one cost real time:

| shape | it looked like | it actually was | issue |
|---|---|---|---|
| **green next to empty** | `reachable: true`, `agents: []` | reachability came from a stale probe, sessions from a live fetch — two paths, both "true" and vacant at once | #671 |
| **success next to doing nothing** | bun dev-tier plugin exits 0, no output | maw *executes* the entry file; a file with only `export default` defines a function and reaches EOF — never called | #674 |
| **live-looking but frozen** | node `"local"`, oracle `"mawjs"` on a fresh card | read from the add-time peer store, never re-written from a probe; the fake `"mawjs"` default *looks* probed | #677 |
| **success but blaming the wrong party** | `curl: (22) returned error: 404` under a healthy peer's name | `maw ls --federation` GETs `/api/ls`, which no serve implements — the peer is fine | #676 |

The last one is the most expensive: **silence makes you do nothing; a wrong accusation
makes you do work in the place where nothing is broken.**

## A fifth shape: a partial check wearing the face of a complete one

`maw hey --dry-run` resolved cleanly (`-> peer m5 33-maw-rs:maw-rs via
http://192.168.1.118:3456`) and the real send failed the same second with
`federationToken is required`. Cross-node hey needs three layers — peer reachable,
route resolvable, token matching — and the dry-run only exercises two while
*presenting* itself as a go/no-go. (#681)

This is not "a result without its cause"; it is **a partial check wearing the face of
a complete one**. Same family, different shape — and it is the one that made the twin
look silent for a whole day. The fix is the same discipline as everywhere else in this
doc, aimed at a different target: a dry-run's result must carry which layers it
actually walked, not just whether the layers it walked came back clean.

## The axis problem: a correct detector can still watch the wrong thing

This recurred three times:

- `reachable` watched a probe, while the thing that failed was the fetch
- `loopback-self` watched `127.0.0.1`, while the self-reference arrived over a real
  interface IP
- (fleet, another repo) a model-identity guard watched *which model*, while the
  divergence was *which store* — same model name, two providers, two different
  corpora, guard correctly silent

None of these were broken. Each was watching an axis the failure did not travel on.
The check that catches it: **name the axis the guard watches, then ask whether the
failure you fear actually moves along it.**

## The medicine (one prescription, all four)

**Make the result carry its reason, and never let "unknown" or "wrong" wear the face
of "success."**

Concretely, in order of how often it bit us:

1. **A failure must pack its cause.** `agents: []` became one of four different things
   until `fetch_error` carried *why* (unreachable / refused / bad-URL / genuinely-none).
   And carry it all the way down — walk `std::error::Error::source()` to the OS layer;
   "error sending request" is one layer short of `No route to host (os error 65)`.

2. **A "don't know" must not look like a "yes."** A constant default (`"mawjs"`) that
   renders identically to a probed value is a silent lie. Show `?`, not a plausible
   guess. If two fields disagree in provenance (frozen vs live), say which is which.

3. **Two code paths that must agree should share one helper** so they *cannot* diverge.
   `/fed.json` fetched via reqwest with routable-IP pinning; `maw ls` shelled out to
   `curl` a nonexistent path — same job, two implementations, one silently wrong.

## Corollaries about *testing* the above

- **Test that the detector detects.** After writing the test, revert the fix and run it —
  it MUST go red. A test that stays green while the bug returns guards nothing. #669's
  test exercised the pure endpoint builder but not the call site that decides to *use*
  it: reverting the pin at the call site would have kept the suite green. Nothing
  exploited that gap in the end — the failure that outlived #669 was environmental
  (macOS Local Network permission denying the PM2 daemon), not code — but the gap was
  real, and #670 closed it.

- **Measure a thing where it happens.** A number computed inside a shared loop is the
  loop's number, not the item's — peer latency absorbed other peers' blocking DNS until
  we resolved up front and timed only `send()`. And do NOT assert "two peers to the same
  machine read similar latency": different net paths (LAN vs WireGuard tunnel) legitimately
  differ. The honest check is against an independent request to the *same resolved IP*.

- **Verify on the artifact that ships.** The Linux release is **musl** (no nss-mdns → can't
  resolve `.local`); a debug build is **glibc** (can). Testing the fix on debug and shipping
  musl means the fix that "worked" never ran. Invoke the real, shipped binary. (See also the
  sibling lesson: native vs WASM behavior only settles under a real invoke.)

- **Same-program-different-context beats different-program.** `curl` from a shell inherited
  Terminal's macOS Local Network grant; the PM2 daemon never had it — so `curl` "proved"
  the daemon should work when it couldn't. Compare the same binary run two ways, not two
  different tools.
