// Fixture-based wasm-host tests (artifact-manager lifecycle golden) were
// removed in the repo split — the artifact-manager-plugin wasm fixture now
// lives in Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked
// in #546. The pin below runs on the default test path.

#[test]
fn native_artifact_manager_registrations_are_removed() {
    for command in ["artifact-manager", "art"] {
        assert_eq!(
            maw_cli::dispatcher_status(command),
            maw_cli::DispatchKind::NativeError,
            "{command}"
        );
    }
}
