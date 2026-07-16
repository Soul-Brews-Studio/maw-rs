// Fixture-based wasm-host tests (soul-sync push parity goldens, incl. the
// soulsync/ss aliases) were removed in the repo split — the soul-sync-plugin
// wasm fixture now lives in Soul-Brews-Studio/maw-fixtures @aecf20b6;
// rework/relocate tracked in #546. The pin below runs on the default path.

#[test]
fn soul_sync_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    for command in ["soul-sync", "soulsync", "ss"] {
        assert_eq!(
            maw_cli::dispatcher_status(command),
            maw_cli::DispatchKind::NativeError
        );
    }
}
