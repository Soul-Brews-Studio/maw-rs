// Fixture-based wasm-host tests (incubate dry-run/send goldens + bud
// subdispatch guards) were removed in the repo split — the incubate-plugin
// wasm fixture now lives in Soul-Brews-Studio/maw-fixtures @aecf20b6;
// rework/relocate tracked in #546. The pin below runs on the default path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn incubate_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("incubate"), DispatchKind::NativeError);
}
