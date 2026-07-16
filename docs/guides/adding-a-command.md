# Adding a maw-cli command

Create a new `crates/maw-cli/src/core_impl/partNN.rs` file with:

- `const DISPATCH_NN: &[DispatcherEntry] = &[...]` for the command entries.
- The `run_*` handler functions referenced by those entries.

`crates/maw-cli/build.rs` auto-registers `core_impl/part*.rs` files in numeric order and builds the dispatcher fragment list. Do **not** edit `core_impl/mod.rs` or `core_impl/part01.rs` just to register a new command.
