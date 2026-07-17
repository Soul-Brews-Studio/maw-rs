# Engine aliases — the `X` / `X-resume` pair convention

Engines are named by **aliases**: keys under `commands` in the merged maw
config. `maw wake <oracle> -e <alias>` resolves `commands.<alias>` to a full
shell command line (env vars + binary + flags) and sends it to the pane
(`wake_resolve_engine_command`); an alias with no `commands` entry falls
through to the **literal name**, which is almost never what you want
(`-e opus48` → runs a nonexistent `opus48` binary).

## The pair rule

**Every engine alias `X` ships a `X-resume` twin** — a complete, standalone
command line that relaunches the same engine attached to its most recent
session. A twin is a *full replacement*, never a suffix: each engine family
spells "resume" differently, and the twin encodes the correct form once so no
caller ever needs to know it.

This gives two uniform call paths:

```sh
maw wake <oracle> -e claude48          # fresh
maw wake <oracle> -e claude48-resume   # resume — plain commands lookup, nothing special
```

and (once engine-aware resume lands, #615) the boolean path: a repo commits
`{"wake": {"resume": true}}` and wake resolves `commands.<engine>-resume`
for whatever engine that repo uses.

## Family rules (how each family spells "resume")

| family | fresh form | resume twin | verified via |
|---|---|---|---|
| claude-family (`claude*`, `sonnet`, `fable`, `default`, `agy`) | `… claude <flags>` | append ` --continue` | claude/agy `--help` both take `--continue` |
| codex (bare binary) | `codex <flags>` | **subcommand-first**: `codex resume --last <flags>` — and `codex resume` accepts *fewer* flags than top-level (no `--search`), so the twin drops what resume doesn't take | `codex resume --help` |
| omx wrapper (`omx`, `omx-N`) | `… omx <flags>` | append ` resume --last` (omx reorders internally) | proven `omx-resume` entry |
| thclaws | `thclaws <flags>` | append ` --resume last` | `thclaws --help` |

When adding a new engine, add both keys in the same edit and verify the
resume form against the engine's own `--help` — never assume a family's
spelling transfers.

## Calling conventions

- **One-off**: `-e <alias>` or `-e <alias>-resume` on the CLI.
- **Standing, per-repo**: commit `.maw/maw.config.NN.json` with
  `{"wake": {"engine": "<alias>"}}` or a `commands.default` entry (#600 —
  wake resolves config dir-aware against the resolved repo path, so this
  works from anywhere).
- **Repo-portable resume**: a repo using `commands.default` resolves the
  engine alias `default`, so it can commit its own `default-resume` — the
  repo's resume form with no machine-specific engine name hardcoded.
- Precedence: CLI `-e` > repo layer > user layer > builtin.

## The legacy-layer trap

`~/.config/maw/maw.config.json` (the *no-NN* file) is a **fallback-only
legacy layer**: it is read *only when zero* weighted `maw.config.NN.json`
files exist in the user config dir (`maw-xdg` `discover_config_layers`). The
moment any `maw.config.NN.json` exists, the no-NN file drops out of the
resolution chain entirely — keys unique to it resolve to null.

Consequences:

- Add user-level aliases to the **weighted** layer (`maw.config.50.json` by
  convention), never the no-NN file.
- Verify any config claim with `maw config explain <key>` (per-layer
  provenance + FINAL value) or `maw config sources` (the live chain) — file
  contents lie; the merged chain is the truth.

See also: `#615` (engine-aware `wake.resume`), `#600` (dir-aware repo config
layers), `#618` (the audit that produced this document).
