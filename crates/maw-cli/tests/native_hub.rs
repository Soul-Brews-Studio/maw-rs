// Fixture-based wasm-host tests (hub plugin validation/loader/usage goldens)
// were removed in the repo split — the hub-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.

#[test]
fn hub_dispatcher_removes_only_hub_dispatch() {
    assert_eq!(
        maw_cli::dispatcher_status("hub"),
        maw_cli::DispatchKind::NativeError
    );
    assert_eq!(
        maw_cli::dispatcher_status("xdg"),
        maw_cli::DispatchKind::Native
    );
}
