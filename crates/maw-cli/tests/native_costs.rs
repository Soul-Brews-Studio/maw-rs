// Fixture-based wasm-host tests (costs plugin summary/daily goldens) were
// removed in the repo split — the costs-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.

#[test]
fn native_costs_dispatcher_registration_removed_for_plugin_fallthrough() {
    assert_eq!(
        maw_cli::dispatcher_status("costs"),
        maw_cli::DispatchKind::NativeError
    );
}
