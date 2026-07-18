// Exercises MawWasmHost (wasm-host feature): run via
// `cargo test -p maw-plugin-manifest --features wasm-host` (the wasm-host CI job).
#![allow(clippy::unwrap_used, clippy::expect_used)] // test code: panicking on unexpected state is idiomatic
#![cfg(feature = "wasm-host")]

include!("wasm_host_security/common.rs");
include!("wasm_host_security/manifest_fs_localserver.rs");
include!("wasm_host_security/exec_ssh_capabilities.rs");
include!("wasm_host_security/config_consent.rs");
include!("wasm_host_security/consent_http_network.rs");
include!("wasm_host_security/net_fetch.rs");
include!("wasm_host_security/time.rs");
include!("wasm_host_security/tmux_contracts.rs");
include!("wasm_host_security/filesystem_protection.rs");
include!("wasm_host_security/manifest_roots.rs");
