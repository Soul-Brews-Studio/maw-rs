# Native `maw schedule` verb

Status: proposed

Issue: [#456](https://github.com/Soul-Brews-Studio/maw-rs/issues/456)

Behavioral references: `~/.maw/bin/maw-schedule` and
`~/.maw/bin/maw-schedule-fire` as installed on m5 on 2026-07-12. Odin's vendored copies
remain the rollback implementation until native operation is proven.

## Decision: macOS-only v1

Version 1 supports macOS launchd only. The production contract being absorbed is a
launchd contract; adding systemd timers or a portable daemon on day one would combine a
behavior-preserving port with a second scheduler design and double the cutover risk.

The command is still compiled into Linux releases. `maw schedule --help` works, while
other subcommands return a clear unsupported-platform error before reading or changing
state. Pure schedule parsing and rendering compile and test on every target. Platform
operations sit behind a `ScheduleBackend` boundary so a later Linux backend can implement
the same typed plan without changing TOML or fire semantics.

## Command surface

The native dispatcher is `maw schedule` and mirrors the actual Python parser:

```text
maw schedule add <id> <command> --every <cadence> [--at <value>]
                 [--max-fires <N>] [--exec claude-headless|shell]
                 [--expected-output <template>] [--token-name <name>]
maw schedule ls
maw schedule peek <id>
maw schedule rm <id>
maw schedule sync [--check|--dry-run]
maw schedule run <id> [--force]
maw schedule pause <id>
maw schedule resume <id>
maw schedule logs <id> [-n <N>]
maw schedule cost
```

`--force` is additive recovery behavior: a manual run may bypass a stale or exhausted
cap, but it is always marked forced in the outcome record. The Python docstring mentions
`--cwd`, but its parser never accepts it; v1 therefore keeps the oracle repository as the
working directory instead of inventing a dormant option.

Repository discovery initially covers the same two-level roots as the script
(`/opt/Code/github.com` and `~/Code`) and requires `.maw/schedule.toml`. A later change
may use configured ghq roots, but cannot silently remove either legacy root.

## TOML compatibility

There is no schema version or migration. Existing `<oracle>/.maw/schedule.toml` bytes
must parse unchanged, and files written by Rust must remain readable by Python `tomllib`.
The root contains `[[schedule]]` array-of-table entries:

| Key | Type | Required/default |
| --- | --- | --- |
| `id` | string | required, unique within the file |
| `command` | string | required |
| `cadence` | string | required |
| `max_fires_per_day` | integer | default `24` |
| `exec` | string | default `claude-headless`; `shell` is the other v1 mode |
| `expected_output` | string | optional repository-relative deliverable template |
| `token_name` | string | optional per-job `pass` token name; default `t2` |
| `created` | RFC 3339 string | written by `add`, optional when reading |
| `at_minute` | integer | optional |
| `at_hour` | integer | optional |

Accepted cadence behavior remains:

- `every Nm` or `every Nmin` -> `StartInterval = N * 60`;
- `every 1h` -> 24 calendar entries, default minute zero;
- `every Nh` -> `24 / N` calendar entries from `at_hour` (default zero);
- `daily at HH:MM` -> one calendar entry.

The parser validates positive intervals and clock ranges instead of emitting an empty or
invalid plist. `add --every 1h` still normalizes to `every 1h`, and it also accepts the
documented `daily at HH:MM` form without adding the Python script's erroneous `every `
prefix.

Use a document-preserving TOML editor. `add` and `rm` retain comments, entry order, and
unknown keys; new strings receive valid TOML escaping. A newly created file uses the
existing generated header and field names. No field is renamed and no bulk rewrite occurs
during `sync`.

## Crate and I/O boundaries

A small leaf crate, `maw-schedule`, owns side-effect-free behavior:

- serde/TOML data shapes and validation;
- cadence parsing into typed interval/calendar plans;
- launchd plist model and deterministic XML rendering;
- pure fire-cap and outcome state transitions.

`maw-cli` owns repository discovery, file writes, logging, process execution, and the
dispatcher. A macOS backend owns `launchctl`; tmux creation goes through `maw-tmux` rather
than raw tmux commands. The leaf receives all paths and timestamps as inputs.

## Plist generation and boot recovery

`sync` generates
`~/Library/LaunchAgents/com.maw.schedule.<oracle>.<id>.plist`, preserving the existing
label, log paths, `RunAtLoad=false`, and schedule mapping. XML text is escaped by the
renderer, and fixtures compare the parsed plist shape with Python output for every live
schedule.

`ProgramArguments` invokes an absolute maw-rs binary with a private `schedule fire`
entrypoint and separate arguments for oracle, id, command, cwd, cap, and exec mode. The
optional expected-output template and token name are also separate arguments. The
plist also contains `HOME` and the legacy compatibility `PATH`, but correctness never
depends on a login shell, shell profile, or direnv.

At sync time, resolve required executables (`maw-rs`, `tmux`, `claude`, `pass`, and
`/bin/bash` where applicable) to absolute paths and reject installation if a required
binary is missing. Pass only paths and a token name to the fire process; never put a
credential value in a plist, process argument, log, or state file.

`sync` also installs a controller plist, `com.maw.schedule.sync`, with `RunAtLoad=true`.
It runs a private `maw schedule sync --boot` after login and re-bootstraps missing
configured jobs without removing healthy ones. In addition, every human-facing schedule
invocation compares desired TOML labels, plist files, and
`launchctl print gui/<uid>/<label>`, then warns about drift. `sync --check` is
non-mutating; plain `sync` repairs it. This addresses hard-freeze reboots where
plist-on-disk did not imply a bootstrapped user agent.

## Fire lifecycle: reserve, execute, finalize

The current comment says the Python counter is flocked, but its snippet imports `fcntl`
without acquiring a lock. Rust must provide real cross-process exclusion.

1. Open the per-job log before any credential, cwd, cap, or binary check.
2. Under an exclusive advisory lock on `~/.maw/state/fires.json.lock`, read the existing
   integer counter schema from `~/.maw/state/fires.json`, prune to seven days, and count
   active reservations for `<local-date>/<oracle>.<id>`.
3. If committed plus active reservations reaches the cap, record `cap-hit`, append a log
   line, and exit successfully without executing. `run --force` may bypass this check.
4. Atomically create a run reservation and outcome record, then release the lock.
5. Validate cwd, absolute binaries, and credentials; start `shell` directly or create a
   detached tmux session containing the private native execution helper.
6. Record `spawned`. The helper waits for the child, captures its exit status, and records
   whether the output file was created and received bytes.
7. Reacquire the lock. Only a successful terminal outcome commits the daily integer and
   removes the reservation. Failure removes the reservation without consuming quota, so
   a corrected job can retry immediately. Atomic temp-file + flush + rename updates
   `fires.json`; corruption fails closed and is logged rather than reset silently.

Transport success requires exit zero and, for `claude-headless`, a created, non-empty
session output file; empty shell stdout remains valid. When `expected_output` is set,
finalize expands literal `$TODAY` (`YYYY-MM-DD`) and `$HOUR` (`HH`) tokens from the run's
scheduled local time, without a shell. For example:

```toml
expected_output = "ψ/memory/huginn/$TODAY/$HOUR00_works.md"
```

The resolved path must remain inside the repository. `deliverable_written` is true only
when that file is non-empty and was created or changed after `reserved_at`; false records
`completed-without-deliverable`, warns, and does not commit the cap. It is null when no
check is configured.

Reservations prevent concurrent fires from overshooting a cap while still allowing a
failed fire to release its slot. Each stores run id, pid/tmux session, start time, cadence,
and boot identity. Reconciliation records `abandoned` and releases the slot when the
process/session is gone, reservation age is `> 2 × cadence`, or the machine boot
identity differs from the one captured at reservation time.

The legacy counter remains byte-shape compatible:

```json
{"2026-07-12":{"odin-oracle.daily-who":1}}
```

`~/.maw/schedule/runs/` is a stable, machine-readable v1 interface, not private
scratch space. Each `<run-id>.json` atomically exposes `schema_version`, job/run identity,
cadence, boot identity, timestamps, status/error/exit code, cap/forced flags, output
metadata, expected deliverable path, and `deliverable_written`. Terminal records are
immutable. `runs/latest.json` is an atomically replaced index with `schema_version`,
`generated_at`, and jobs keyed by `<oracle>.<id>`; each value includes cadence seconds,
latest run id/status/update time, `deliverable_written`, and outcome path. Readers must
tolerate additive fields; incompatible changes require a new schema version. External
oracles watch this outcome freshness instead of logs, including detecting a controller
that is loaded but no longer producing outcomes. `sync` seeds configured jobs as
`never-fired`; only configuration and lifecycle transitions update the index, so reads
cannot mask staleness. A watcher alerts when `never-fired` passes its first due time or
`updated_at` age exceeds `2 × cadence`. Python rollback can still read `fires.json`.

## Required failure behavior

### Credential hydration under launchd

launchd provides an almost empty environment and no direnv. For `claude-headless`, keep
an existing non-empty `CLAUDE_CODE_OAUTH_TOKEN`; otherwise resolve the absolute `pass`
binary and read `pass show claude/token-$CLAUDE_TOKEN_NAME`, defaulting the name to `t2`.
The per-job TOML `token_name` overrides `CLAUDE_TOKEN_NAME`, allowing different accounts
to rotate independently without changing the controller or global launchd environment.
Command substitution semantics trim trailing newlines. Empty/nonzero output records a
credential failure and releases the cap reservation. The token is passed only through
the child environment and is never rendered or persisted.

### Quote-safe command delivery

The Bash fix used `printf %q` because embedding prompts containing quotes, parentheses,
or `## WHO Matrix` in a tmux shell string killed Claude silently. Rust preserves that
guarantee structurally: tmux receives only a safe generated run id and invokes
`maw schedule exec <run-id>`. The helper loads the command from its private run file and
passes it to `claude -p` as one `Command` argument. User text is never interpolated into a
shell command. Shell mode deliberately uses `/bin/bash -c` with the command as a separate
argv value. Regression fixtures include spaces, single/double quotes, `$`, parentheses,
Unicode, newlines, and the real daily-who prompt.

### No silent exits; loaded is not working

Rust removes Bash `set -u` failures, but every return path still goes through one failure
reporter that appends a timestamped job log line and writes a terminal outcome. Plist
stdout/stderr point at the same log as a last-resort diagnostic path.

Every fire has an atomic record with `reserved_at`, `spawned_at`, `exited_at`, status,
exit code/error, `forced`, `cap_committed`, output path, `output_file_written`, and output
byte count, expected deliverable path, and `deliverable_written`. `ls` and `peek` show
configured/plist/loaded health plus the latest outcome; they never equate `launchctl`
loaded state with a working job. A missing outcome after a launchd trigger, a nonzero
exit, or missing output is visible and testable.

## Rollout and rollback

1. Capture current TOML, parsed plist, counter, log, and quoting examples as fixtures.
2. Land pure schema/cadence/plist/counter code and Linux unsupported-platform tests.
3. Run native `sync --dry-run` against every live TOML; compare parsed plist plans with
   Python without bootstrapping anything.
4. Run manual shell and headless canaries under an empty environment. Prove pass
   hydration, quote-safe delivery, expected-output checks, failure slot release, and the
   stable outcome feed.
5. Run one separate canary label for seven days; never activate Python and Rust jobs for
   the same production schedule concurrently.
6. Cut over one oracle at a time: back up TOML/plists, boot out its Python-generated
   labels, write native plists with the same labels, bootstrap, and verify a real outcome.
7. Keep both installed scripts and Odin's vendored copies unchanged through the soak.
   Rollback boots out native labels and runs the Python `sync`; TOML and `fires.json`
   require no conversion.
8. Remove loose scripts only after all jobs have seven days of successful native outcomes
   and reboot-resync has been exercised.
