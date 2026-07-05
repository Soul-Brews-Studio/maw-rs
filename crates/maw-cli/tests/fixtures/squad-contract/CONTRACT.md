# squad plugin — behavioral contract (#72)

Golden source (READ-ONLY reference, extracted 2026-07-03):
`laris-co/athena-oracle/.maw/plugins/squad/{impl.ts,index.ts,plugin.json}` @ v2.0.0.

This is the behavior a maw-rs port MUST preserve. It is tier-agnostic: the same
filesystem effects and validation failures apply whether the port runs on the
bun-dev tier (TS on Bun) or the ship tier (WASM on Extism). A port either matches
these observable effects or it visibly does not.

The acceptance harness is `crates/maw-cli/tests/squad_acceptance.rs`.

---

## 1. Identity model — "the lead IS the team"

There is no wrapper entity. You run `maw squad <sub>` **from the lead oracle's repo**,
and the team name is **derived**, never typed:

```
team = basename(repo) with a trailing "-oracle" stripped
```

`repo` is `git rev-parse --show-toplevel`, falling back to the lead-repo dir when that
is not a git repo. Examples: `athena-oracle` → team `athena`; `maw-rs-oracle` →
`maw-rs`; `foo` → `foo`. Empty derivation is a hard error
(`can't derive a team name from this directory`).

**Non-obvious — how the lead repo is located under the maw-rs bun-dev runtime.** The
golden athena impl reads the lead repo from the process cwd, because athena's maw-js
runtime runs the plugin *in* the lead repo. maw-rs is different: the bun-dev runtime
spawns the entry with `current_dir = <plugin dir>` (see
`dispatcher.rs::dispatch_bun_dev_plugin`), so `process.cwd()` points at the plugin dir,
**not** the lead repo. `.current_dir()` does not rewrite the child's `PWD` env var,
though — the child inherits `PWD` from the invoking shell unchanged. So the port must
recover the lead repo from `process.env.PWD` (the dir the user actually ran
`maw squad` from), falling back to cwd only when `PWD` is unset. The team is then
`basename(git -C $PWD rev-parse --show-toplevel)` minus `-oracle`. A port that reads
`process.cwd()` here would silently derive the wrong team (the plugin dir). The
acceptance harness pins this by setting `PWD` to a git-init'd lead repo it names
`athena-oracle` while the plugin stages elsewhere — mirroring real invocation, where
lead repo and plugin dir are distinct.

## 2. On-disk layout — one folder per team

Root: `~/.claude/teams/` where `~` is `os.homedir()` (POSIX: the `HOME` env var).
A team is exactly one folder:

```
~/.claude/teams/<team>/
  config.json                     # roster + lead metadata
  inboxes/
    team-lead.json                # lead's inbox (member → lead replies land here)
    <member>.json                 # one file per member (lead → member messages)
```

All JSON files are written with `JSON.stringify(value, null, 2) + "\n"`
(2-space indent, single trailing newline). Empty inboxes are the literal 3 bytes
`[]\n`.

### config.json shape

```json
{
  "name": "<team>",
  "members": [ /* member entries, see below */ ],
  "createdAt": 1720000000000,           // ms epoch, set once at first start
  "leadSessionId": "<uuid-or-timestamp>",
  "leadRepo": "/abs/path/to/lead/repo"
}
```

`leadSessionId` is filled from `maw team-agent uuid --bare`; if that command is
absent or empty it falls back to `String(Date.now())`, so it is **always non-empty**
after `start` and never depends on `maw` being installed. `leadRepo` is set to the
lead repo path on every `start`.

### member entry shape (added by `join`)

```json
{ "agentId": "<name>@<team>", "name": "<name>", "color": "<color>",
  "repo": "/abs/path", "joinedAt": 1720000000000 }
```

## 3. Subcommand semantics

### `start` — start THIS repo's squad (adopt, never clobber)

1. `mkdir -p ~/.claude/teams/<team>/inboxes` (recursive).
2. If `config.json` exists, **read and preserve it** (existing `members` and
   `createdAt` survive); otherwise seed `{name, members:[], createdAt:now}`.
3. Fill `leadSessionId` only if absent; always set `leadRepo`; write config.json.
4. Create `inboxes/team-lead.json` as `[]\n` **only if it does not already exist**.
5. Output is loud (leading `⚡`) and reports whether the team was "started" or
   "adopted (already existed)". Exit 0.

Idempotency is the contract: running `start` twice never drops members or resets
`createdAt`, and never truncates an existing `team-lead.json`.

### `say <member> <text...>` — append to a member's inbox (never clobber)

Multi-word text is joined with spaces (`rest.join(" ")`). On success it appends one
entry to `inboxes/<member>.json`, reading any existing array first:

```json
{ "from": "team-lead", "text": "<text>", "timestamp": "<ISO-8601>",
  "color": "cyan", "type": "message", "read": false }
```

Running `say` N times yields N entries in order — appends, **never overwrites**.
Exit 0, prints `✓ said to <member>@<team>: <text>`.

### `ls` — reflect this squad

Reads config.json (if present) and prints: team name, lead repo basename + session,
each `member: <name> (<color>) <repo>`, or `members: (none yet …)` when empty. Then
lists `inboxes:` as `<name>` or `<name> (<n> unread)` (unread = entries with
`read !== true`), and a best-effort `live tmux:` line. `ls` is read-only and
tmux-safe: with no matching live sessions (or tmux absent) the live line is
`(none)`. Exit 0.

### `join <oracle> [color]` — spawn a member (dev tier stays on `mawjs`)

Not part of the first ported surface — session creation lives in `mawjs` until
maw-rs grows a native spawn. Its **guards** (below) still fire and are tier-portable;
its spawn path is not.

## 4. Validation failures — loud, and ZERO writes

Every guard below throws **before any filesystem write or process spawn**. "Loud"
means the CLI reports failure: the process exits non-zero (an uncaught throw in the
dev-tier script exits non-zero; the golden handler surfaces `ok:false`). The
invariant the harness asserts on every guard is **no bytes written** — no inbox file
created, no message appended, roster unchanged, nothing escapes the teams root.

- **`NAME_RE`**: names must match `/^[a-z0-9][a-z0-9_-]*$/i`. Rejected set includes
  anything starting with a non-alphanumeric and anything containing `/`, `.`, or
  `..` — i.e. path traversal (`../evil`, `foo/bar`, `.`, `a.b`) is rejected. Applies
  to `say <member>` and `join <oracle>`.
- **non-member `say`**: saying to a name not in `config.members` throws
  `'<member>' is not in squad '<team>' …` and writes nothing (no inbox file is
  created for the non-member). This is the "silent message loss" guard made loud.
- **color** (`join` only): color must be one of
  `red green yellow blue purple cyan magenta white`. An invalid color (e.g. `orange`)
  throws *before* any spawn — invalid `--agent-color` would make the real spawn fail
  SILENTLY, so this is caught up front. Default color is `cyan`.
- **not started** (`say`/`join`): if `config.json` is absent, throws
  `squad '<team>' not started …` and writes nothing.

Guard ordering in `join` is: usage → NAME_RE → color → (then team lookup / session
pre-check / spawn). So invalid-name and invalid-color are reachable and assertable
**without** tmux or `mawjs`.

## 5. Session pre-check — the one live-tmux dependency

`join` enforces "one oracle, one session": before spawning it runs
`tmux ls -F '#S'` and refuses if a session equal to `<role>` or ending in
`-<role>` already exists (`maw new` exits 0 without respawning a stale session, so a
naive "joined" would lie). This is the only path that needs a live tmux server plus
`mawjs`/`maw locate`; the harness gates the full-join test `#[ignore]` for it and
keeps every other test tmux-free.
