// Fixture-based wasm-host tests (mega ls/status/tree/kill against the fake
// tmux) were removed in the repo split — the mega-plugin wasm fixture now
// lives in Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked
// in #546. The dispatcher registration pin below runs on the default path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn mega_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("mega"), DispatchKind::NativeError);
}
