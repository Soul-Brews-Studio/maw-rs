//! Pure plugin scaffold helpers ported from maw-js
//! `src/commands/shared/plugin-create-scaffold.ts`.
//!
//! This crate ports the deterministic validation/manifest helpers plus the
//! template tree-copy, Rust/AssemblyScript scaffold, and command guard
//! contracts from `test/plugin-create.test.ts`.

use std::{fs, io, path::Path};

use serde_json::{json, Map, Value};

include!("lib/plugin_create_command.rs");
include!("lib/scaffold_entrypoints.rs");
include!("lib/manifest_rewrites.rs");
include!("lib/copy_tree.rs");

#[cfg(test)]
mod tests {
    include!("lib/scaffold_regression_tests.rs");
}
