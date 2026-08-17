# Config guide

- If any weighted layer (`maw.config.<N>.json` or `maw.config.<N>.local.json`) exists in the active config directory, legacy `maw.config.json` is ignored; move live keys into a weighted layer.
- To see which files are actually in the load set: `maw config sources` (full set, including cwd-dependent Project layers) or `maw xdg paths --plan-json` (`configLayers` — the user config dir only, plus `configPath` for the layer that wins there). Both read the loader; neither guesses a filename (#840).

## Dir-local layers (`.maw/maw.config.<NN>.json`)

Merged config is resolved per directory: besides the user config dir, every
ancestor of the resolution directory contributes committed
`<dir>/.maw/maw.config.<NN>.json` (and `.<NN>.local.json`) layers at Project
scope. Higher `<NN>` wins across scopes; at equal `<NN>`, Project beats User
and `.local.json` beats the plain file; a `null` value in a later layer
deletes the key (so a repo can also *unset* a user default).

### How each value type merges

| Value type | Later layer does |
| --- | --- |
| scalar (string / number / bool) | replaces |
| object | deep-merges key by key |
| `null` | **deletes** the key |
| array | replaces wholesale |
| `namedPeers` array | merges **by peer `name`** (#874) |

`namedPeers` is the one array-valued key that is a keyed collection rather than
an ordered list: each element is `{"name": …, "url": …}` and the resolver looks
entries up by `name` (the accepted alternative spelling is literally an object
keyed by name). So a later layer *adds* peers and overrides same-named ones
entry-for-entry, and peers only the earlier layer defined survive. An override
replaces the matched entry whole rather than field by field — a peer's `url`,
`pubKey` and token are one credential set, and half-merging them could pair a
new `url` with the previous host's key. To drop inherited peers instead of
adding to them, use the `null` rule: `"namedPeers": null` in a later layer
clears the key, and a layer above that can define a fresh list.

Every other array — `peers` (a list of URLs) included — still replaces
wholesale, so trimming or reordering a list in a later layer works as written.

`maw work` and `maw wake` both resolve config against the **resolved target
repo/worktree path**, not the invoking shell's cwd — `maw wake myrepo` run
from anywhere honors `myrepo/.maw/maw.config.<NN>.json` (engine `commands`,
`hooks.postWake`, and the `wake` block below). A `--repo-path` override
composes the same way, which gives `team up`/`team spawn` coders
worktree-local config automatically. Fleet *group* postWake hooks are the one
exception: a squad has no single repo path, so they keep the process-cwd read.

## Committed wake defaults (`wake` block)

```json
{
  "commands": {
    "omx-1": "CODEX_HOME=$PWD/.codex omx --direct",
    "default": "codex"
  },
  "wake": {
    "engine": "omx-1",
    "resume": false,
    "channels": true,
    "prompt": "read AGENTS.md first"
  }
}
```

Precedence is strict: **explicit CLI flag > repo-layer config > user config >
built-in default.**

- `wake.engine` applies only when no `-e`/`--engine` is given, and resolves
  through the same `commands` map.
- `wake.resume: true` behaves exactly like `--resume` (pins the codex engine
  before `wake.engine`/`commands.default`); `--fresh` opts out, explicit `-e`
  beats the pin.
- `wake.channels` / `wake.prompt` fill in only when the flag is absent.

Trust note: like `maw work`, wake executes the merged `commands` entry (and
`hooks.postWake`) as shell in the target pane. Because `/api/wake` runs the
full local wake path on the receiving node, a federation-triggered wake
executes the *target repo's* committed `.maw` config — the woken repo chooses
its own launch command. That is the same command `maw work` in that repo
already runs, and the route is signature-gated, but committed config is
executable: review `.maw/maw.config.*.json` changes like code.
