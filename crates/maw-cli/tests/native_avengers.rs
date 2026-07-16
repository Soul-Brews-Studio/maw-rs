// Fixture-based wasm-host tests (avengers status/json/health/missing-config
// goldens) were removed in the repo split — the avengers-plugin wasm fixture
// now lives in Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate
// tracked in #546. The pin below runs on the default test path.

#[test]
fn avengers_native_dispatcher_registration_is_removed() {
    assert_eq!(
        maw_cli::dispatcher_status("avengers"),
        maw_cli::DispatchKind::NativeError
    );
}
