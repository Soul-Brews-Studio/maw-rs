// Fixture-based wasm-host tests (dream porcelain golden + seeded-state write)
// were removed in the repo split — the dream-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn dream_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("dream"), DispatchKind::NativeError);
}
