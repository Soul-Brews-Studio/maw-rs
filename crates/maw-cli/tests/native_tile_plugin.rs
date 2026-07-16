// Fixture-based wasm-host tests (tile spawn/swap/clean invoke goldens) were
// removed in the repo split — the tile-plugin wasm fixture now lives in
// Soul-Brews-Studio/maw-fixtures @aecf20b6; rework/relocate tracked in #546.
// The dispatcher registration pin below runs on the default test path.
use maw_cli::{dispatcher_status, DispatchKind};

#[test]
fn tile_dispatcher_fallthrough_is_preserved() {
    assert_eq!(dispatcher_status("tile"), DispatchKind::NativeError);
}
