// Fixture-based wasm-host tests (demo plugin golden + tmux showcase) were
// removed in the repo split — the demo-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn demo_native_dispatcher_registration_is_removed() {
    assert_eq!(dispatcher_status("demo"), DispatchKind::NativeError);
}
