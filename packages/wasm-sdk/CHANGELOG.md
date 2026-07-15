# Changelog

All notable SDK and host ABI changes are documented here.

## 1.0.0

- Publish the AssemblyScript SDK as `@maw-rs/wasm-sdk`.
- Bind `maw.time.now` for host-provided wall-clock time.
- Bind `maw.tmux.command` for capability-gated managed tmux operations.
- Document paginated `maw.fs.list` requests (`offset`) and responses
  (`nextOffset` or `done`) while retaining the 1,000-entry page cap.
