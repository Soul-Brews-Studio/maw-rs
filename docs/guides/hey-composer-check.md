# Local hey and composer hints

Local `maw hey` bypasses the pre-send pending-composer check. Claude Code
autocomplete suggestions can look like unsubmitted input to the prompt-marker
heuristic; they must not prevent an explicit hey delivery.

The bypass is selected only by the local `hey` call site in
`crates/maw-cli/src/core_impl/send_federation.rs`. Ordinary `send_text` and
`preflight_send_text` callers remain guarded, including serve, talk-to, wake,
and worktree-finish. Remote delivery still uses the receiving server's policy.

Force does **not** clear the composer: an actual draft may receive appended
text. It also does not disable pane-mode handling, tmux error propagation,
post-Enter confirmation, routing checks, or authorization.

Regression checks live in maw-tmux's `pending_retry_tests.rs`. Run them with
an isolated target directory:

```sh
cargo test -p maw-tmux --lib --locked --no-fail-fast
```

The original diagnosis came from Digger (dig_seq 243, commit `02ff78f`):
prompt markers alone cannot distinguish autocomplete from typed input.
