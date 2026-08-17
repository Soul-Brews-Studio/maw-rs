---
name: peek-before-you-trust-then-talk-back-and-keep-each-other-honest
description: Peek the paired maw-rs oracle's live pane over federation, exchange a maw hey status sync with standing-instruction format, and verify both sides are on the same commit/version/issues/PRs before doing anything else together.
installer: create-shortcut
created_at: 2026-08-17T13:55:00+07:00
created_session: a5425642
---

# /peek-before-you-trust-then-talk-back-and-keep-each-other-honest

Scoped to the `maw-rs` two-oracle federation (`black` ↔ `white`). Before trusting anything the
other oracle says about their state, look at their pane directly and diff the hard facts —
commit SHA, binary version, open issues/PRs. Born from a session where "no reply in 35 minutes"
turned out to be three different real failures (orphaned tmux target, no oracle identity to
receive into, and a shared curl-argv bug) — none of which "just wait longer" would have fixed.

## Step 0: Init

```bash
date "+🕐 %H:%M %Z (%A %d %B %Y)" && cd "$(git rev-parse --show-toplevel)"
```

## Step 1: Resolve the peer alias

Default peer alias is `whitebox` when run from `black`, `blackbox` when run from `white` — but
don't assume, check what's actually registered:

```bash
maw peers list --all
maw peers probe <alias>          # confirm reachable before trusting a peek/hey to it
```

## Step 2: Peek the other side's live pane

```bash
maw peek <alias>:<their-session-target> --lines 40
```

If this fails with `curl argv must not contain NUL/control characters`, that's #861 — you're on
a binary before the fix (`0387142f`). `git pull && cargo build --release -p maw-cli` and reinstall
before retrying; don't retry the same broken binary expecting a different result.

If it fails with `window not found`, don't assume the peer is down — check your OWN binary
version first (`maw --version`); peer-peek syntax only exists from commit `9b1bcb09` (#858)
onward.

## Step 3: Sync the hard facts (not the claimed facts)

Run these yourself — don't accept the other oracle's self-report without a check:

```bash
git fetch origin --quiet
git log --oneline -1                                   # your commit
maw --version                                           # your binary
gh issue list --state open --limit 20 --json number,title
gh pr list --state open --json number,title,headRefName
```

Ask the peer (via hey, see Step 4) to run the same four commands and report back. Diff the
results. A mismatch in commit SHA while both claim "synced" is the single most common false
handshake — catch it here, not three exchanges later.

## Step 4: maw hey — status sync with standing-instruction format

Every ask must carry: **(a)** exact repro/test command **(b)** expected result **(c)** commit SHA
to test against. This is not optional politeness — it's what turns "please verify" into
something the other oracle can act on without a follow-up question.

```bash
maw hey <alias>:<their-session> --from <your-node>:<your-handle> "<your-node>:<your-session> — status sync $(date +%Y-%m-%d\ %H:%M)

WHERE I AM
  repo   <sha>  (<ahead/behind vs origin/alpha>)
  binary <version>
  tree   <clean, or list of dirty files>

REQUEST
  command:   <exact command>
  expected:  <exact expected output>
  SHA:       <commit to run it against>

<question or handoff, one item per line, not a wall of prose>"
```

`--from` must be `node:handle` (e.g. `black:black`), never `node:reponame` — the signer checks
identity, not project name. Never start the message body with `[bracket-prefix]` — that syntax
is reserved for signed transport headers and the send will be rejected.

## Step 5: Confirm delivery, don't assume silence means failure

`maw hey` returns `queued <target>` on accepted send — that only means it was queued, not read
or acted on. If there's no reply after a reasonable interval:

```bash
maw peers probe <alias>          # rule out "peer unreachable" first
maw peek <alias>:<session>       # look at what they're actually doing — often visible directly
```

Don't re-send the same message on a timer assuming it's being dropped — in the session this
skill was born from, every "silent" reply had actually errored on the sender's end (404 target
gone, 502 no receiving identity) and re-sending wouldn't have surfaced that; only peeking or
being told the specific error did.

## Step 6: Keep the binary tidy

Every rebuild in this loop produces a multi-MB binary. Don't let old copies accumulate:

```bash
# swap, don't overwrite-in-place (avoids SIGKILL on a running process reading the old inode)
mv "$(which maw)" /tmp/maw.prev-$(git rev-parse --short HEAD) 2>/dev/null
cp target/release/maw-rs "$(dirname "$(which maw)")/maw" && chmod +x "$(which maw)"
# clean up the previous-previous backup once the new one is verified working
rm -f /tmp/maw.prev-* 2>/dev/null  # only after confirming `maw --version` looks right
```

## Related

- `.claude/skills/../ψ/writing/cheatsheets/2026-08-17_black-white-federation.md` — the raw
  command reference this skill was distilled from
- Issue #861 / PR #862 — the curl-argv bug that motivated Step 2's version check
- Issue #818 — a local session name silently shadowing a peer of the same name (why Step 1
  never assumes the alias without checking `maw peers list`)
