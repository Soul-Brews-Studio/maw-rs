# Frozen inventory: bring and split

Baseline: `maw-rs/origin/alpha` `a17ad351ef9edc28dbd745c84fba584544f40966`.
Line references are one-based at that tree. Current behavior is authoritative unless a child issue
approves a separate correction with RED evidence.

## `bring` / `b`

### Native owner and callers

- Registration: `crates/maw-cli/src/core_impl/session_list_plan.rs:1-4`, `DISPATCH_304`.
- Implementation: `run_bring_plan` and two renderers at `session_list_plan.rs:962-1023`.
- Parser import: `crates/maw-cli/src/core_impl/dispatcher.rs:31` from `maw-bring`.
- Dependency: `crates/maw-cli/Cargo.toml:21`; remove the matching lock entry only at cutover.
- No production caller invokes `run_bring_plan` directly; dispatcher ownership is the live surface.
- Helpers immediately after the bring renderers (`push_json_opt`, `json_string`) are shared and must
  remain.

### Frozen behavior

`bring` is plan-only. It never calls `wake` or mutates tmux. Default text begins
`wake <oracle> --split`; optional engine/session/split-target/pick lines follow. JSON is
`{"command":"bring","opts":{...}}`. Missing oracle is code 2 with `bring: missing oracle name`
and usage. The pinned parser intentionally ignores `--split`, `--tab`, unknown dash flags, and later
positionals; `--engine`, `--pick`, and `--to` retain current quirks. `wake --split` is parsed elsewhere
but inert and is not activated by this extraction.

### Tests to transfer/retain

- `crates/maw-cli/tests/bring_cli.rs`
- bring rows in `auth_discover_worktree_ls_tail_edges.rs`, `ls_bring_remaining_edges.rs`,
  `worktree_ls_bring_cli_edges.rs`, `plugin_pair_route_render_edges.rs`, and
  `verb_help_flag_cli.rs`
- pinned `maw-bring` parser/fixture tests at revision `e4693208...`

Target: capability-free Rust artifact `maw-plugins/packages/bring`, canonical `bring`, alias `b`.
Cutover also updates help, completion, known external ownership, and missing/refusal guidance.

## `split`

### Native owner and callers

- Entire direct implementation/test module: `crates/maw-cli/src/core_impl/split.rs` (129 lines,
  `DISPATCH_113`). Delete only after accepted artifact parity.
- Shared action builder/validation remains in `maw-tmux`; `maw-split` is unrelated Claude-pane policy
  and remains.
- `crates/maw-cli/src/core_impl/buddy_workspace.rs` invokes the current executable as `split` for
  `bud --split` and currently discards the nested result; extraction preserves that best-effort
  behavior, with any loud propagation deferred to a separately approved issue.

### Frozen behavior and blockers

Default is horizontal 50%. Options include `-v|--vertical`, `--pct[=]`, `--cmd[=]`, and `--dry-run`
in flexible order; target and optional command are strictly guarded. Success prints `split -> target`;
dry-run prints exact `tmux split-window ...` text without mutation.

Before cutover:

1. manifest metadata must let split own first-argument `-v` (the current plugin runtime treats it as
   plugin version) and its help contract;
2. `maw.tmux.split.v1` must validate all fields before exactly one host action;
3. the existing raw tmux allowlist must not be widened;
4. `bud --split` must remain best-effort and silently discard the nested result, including a
   missing/refused plugin, until a separate behavior-change issue is approved.

### Tests to transfer/retain

- unit tests in `split.rs`
- `crates/maw-cli/tests/native_attach_view_stream_split.rs`
- split rows in `native_interactive_plugins.rs`
- `crates/maw-cli/tests/fixtures/epic56/split-dry-run.stdout`

Target: Rust artifact `maw-plugins/packages/split`, canonical `split`, typed split capability only.
