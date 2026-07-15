// Fixture-based wasm-host tests (contacts add/list/remove goldens + psiPath
// config) were removed in the repo split — the contacts-plugin wasm fixture
// now lives in Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate
// tracked in #546. The pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn native_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("contacts"), DispatchKind::NativeError);
    assert_eq!(dispatcher_status("contact"), DispatchKind::NativeError);
}
