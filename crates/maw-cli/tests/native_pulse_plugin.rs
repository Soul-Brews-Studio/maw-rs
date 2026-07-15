// Fixture-based wasm-host tests (pulse list/override/cleanup invoke goldens)
// were removed in the repo split — the pulse-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn pulse_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("pulse"), DispatchKind::NativeError);
}
