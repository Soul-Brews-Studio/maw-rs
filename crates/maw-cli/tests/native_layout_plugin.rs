// Fixture-based wasm-host tests (layout plugin invoke goldens) were removed
// in the repo split — the layout-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn layout_top_level_falls_through_while_tmux_subdispatcher_stays_native() {
    assert_eq!(dispatcher_status("layout"), DispatchKind::NativeError);
    assert_eq!(dispatcher_status("tmux"), DispatchKind::Native);
}
