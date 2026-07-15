# Squadron folder layout

Status: accepted

Issue: [#331](https://github.com/Soul-Brews-Studio/maw-rs/issues/331)

## Context

Fleet storage contains two different kinds of records:

- **session snapshots** describe machine-local tmux sessions and windows;
- **squad rosters** describe a durable unit with a name, members, and policy pointers.

Keeping both as flat JSON files makes a squad hard to inspect, copy, or back up as one
unit. However, maw-js reads session snapshots with a non-recursive directory scan, so
moving those files into folders would break the two-engine filesystem contract.

## Decision

Each fleet root uses this layout:

```text
fleet/
  squads/
    01-3e/
      squad.json
    02-ccdc/
      squad.json
  47-3e-infra.json
  63-homekeeper.json
```

`squads/NN-name/` is the portable squadron unit. Session snapshots remain flat for
maw-js compatibility and because they represent ephemeral state on one machine.

The fleet roots, in read precedence order, are:

1. the maw XDG state `fleet/` directory;
2. `~/.maw/fleet/`;
3. the maw XDG config `fleet/` directory.

An entry with the same logical `name` in an earlier root shadows later roots.

### Roster contract

`squad.json` remains a JSON object compatible with the existing fleet entry model. Its
minimum durable shape is:

```json
{
  "name": "01-3e",
  "squadName": "3e",
  "windows": [],
  "members": []
}
```

The presence of a `members` array distinguishes a squad roster from a session snapshot.
Writers must preserve unknown object and member fields so later metadata can round-trip.
The folder name and `name` should agree; `squadName` is the user-facing lookup name.

Numbers use one `01` through `99` namespace across squad folders and flat fleet files.
Creation chooses the next number unused in any configured fleet root.

### Boundaries

- Team charters stay in `psi/teams/`; their lifecycle and trust model differ from squads.
- Session snapshots stay at the fleet root and are not copied with a squad backup.
- A roster may store a token **name** assigned to a squad or member. Credential values
  remain in the configured token store or `tokenPool`; squad folders never contain keys.
- Per-squad hooks are a future sibling file, not fields silently added to `squad.json`.

## Migration

The reader performs an idempotent migration before loading each fleet root:

| Source | Canonical destination |
| --- | --- |
| `NN-name.json` with a `members` array | `squads/NN-name/squad.json` |
| `groups/NN-name/group.json` | `squads/NN-name/squad.json` |
| `squads/NN-name/group.json` | `squads/NN-name/squad.json` |

During migration, legacy `groupName` is rewritten to `squadName`. Flat JSON without a
`members` array is a session snapshot and is not moved. If the canonical destination
already exists, it wins and the duplicate legacy source is removed.

Recommended operator sequence for a manual migration or recovery:

1. Copy the complete fleet root to a backup location.
2. Move only roster files (`members` present) into `squads/NN-name/squad.json`.
3. Rename `groupName` to `squadName`; preserve all other fields.
4. Run a read command such as `maw fleet ls --json` and verify squad/member counts.
5. Confirm maw-js can still read the unchanged flat session snapshots.

No schema version, legacy mode, or dual-write period is required. Roster folders are a
maw-rs surface; the flat session layout is the compatibility boundary.

## Copy, backup, and future import/export

Copying `squads/01-3e/` captures the squad's durable roster and future squad-scoped
metadata as one unit. A future `maw fleet export/import <squad>` should:

- operate on exactly one squad folder;
- validate the folder name, JSON shape, and all relative paths before writing;
- reject number or squad-name collisions unless replacement is explicit;
- allocate a new free number when importing by logical squad name;
- write through a temporary sibling and rename atomically;
- exclude credentials and machine-local session snapshots.

## Open questions

1. Should `hooks.json` contain only `postWake`, or a versioned map of lifecycle events?
2. Should export include a checksum manifest before remote transfer is supported?
3. On import, should token names be retained, cleared, or reported when absent locally?
4. Should `maw fleet doctor --fix` repair folder/`name` disagreement automatically?
5. Should copying a squad include optional documentation such as `README.md`, and which
   filenames are reserved for maw?

## Implementation touchpoints

- `crates/maw-cli/src/core_impl/scope_find.rs` owns root discovery, migration, and load
  precedence.
- `crates/maw-cli/src/core_impl/fleet_roster.rs` owns numbering and roster creation.
- `crates/maw-cli/src/core_impl/fleet.rs` consumes loaded squads and session snapshots.

Changes to the layout must keep flat session fixtures and migration fixtures. Migration
tests should cover all three legacy sources, repeated reads, destination collisions, and
unknown-field preservation.
