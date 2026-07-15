// Fixture-based wasm-host tests (broadcast session golden + scope/injection
// guards) were removed in the repo split — the broadcast-plugin wasm fixture
// now lives in Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate
// tracked in #546. The pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn broadcast_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("broadcast"), DispatchKind::NativeError);
}
