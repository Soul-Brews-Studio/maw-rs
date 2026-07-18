use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
struct Session {
    name: String,
    windows: Vec<FleetWindow>,
}

impl Named for Session {
    fn name(&self) -> &str {
        &self.name
    }
}

impl FleetWindowSessionLike for Session {
    fn windows(&self) -> &[FleetWindow] {
        &self.windows
    }
}

fn session(name: &str) -> Session {
    Session {
        name: name.to_owned(),
        windows: Vec::new(),
    }
}

#[test]
fn numeric_fleet_prefix_helper_preserves_dash_boundary_rule() {
    assert_eq!(
        resolve_numeric_fleet_stem_prefix("homeke", &[session("20-homekeeper")]),
        ResolveResult::Fuzzy {
            matched: session("20-homekeeper")
        }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_prefix("mawjs", &[session("114-mawjs-no2")]),
        ResolveResult::None { hints: None }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_prefix(
            "homeke",
            &[session("20-homekeeper"), session("21-homekey")],
        ),
        ResolveResult::Ambiguous {
            candidates: vec![session("20-homekeeper"), session("21-homekey")]
        }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_exact("homekeeper", &[session("20-homekeeper")]),
        ResolveResult::Exact {
            matched: session("20-homekeeper")
        }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_exact(
            "homekeeper",
            &[session("20-homekeeper"), session("21-homekeeper")]
        ),
        ResolveResult::Ambiguous {
            candidates: vec![session("20-homekeeper"), session("21-homekeeper")]
        }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_exact("homekeeper", &[session("not-homekeeper")]),
        ResolveResult::None { hints: None }
    );
}

#[test]
fn fleet_window_helper_uses_window_and_repo_oracle_aliases() {
    let items = vec![
        Session {
            name: "23-discord-admin".to_owned(),
            windows: vec![FleetWindow {
                name: Some("discord-oracle".to_owned()),
                repo: Some("Soul-Brews-Studio/discord-oracle".to_owned()),
            }],
        },
        Session {
            name: "114-mawjs-no2".to_owned(),
            windows: vec![FleetWindow {
                name: Some("mawjs-no2".to_owned()),
                repo: Some("Soul-Brews-Studio/mawjs-no2".to_owned()),
            }],
        },
    ];

    assert_eq!(
        resolve_fleet_window_session_target("discord", &items),
        ResolveResult::Fuzzy {
            matched: items[0].clone()
        }
    );
    assert_eq!(
        resolve_fleet_window_session_target("mawjs", &items),
        ResolveResult::None { hints: None }
    );
}
#[test]
fn matcher_edge_branches_cover_empty_str_alias_and_ambiguous_windows() {
    let borrowed: &str = "borrowed";
    assert_eq!(Named::name(&borrowed), "borrowed");

    assert_eq!(
        resolve_numeric_fleet_stem_prefix("", &[session("20-homekeeper")]),
        ResolveResult::None { hints: None }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_prefix("home", &[session("homekeeper")]),
        ResolveResult::None { hints: None }
    );
    assert_eq!(
        resolve_numeric_fleet_stem_prefix("homekeeper", &[session("20-homekeeper")]),
        ResolveResult::None { hints: None }
    );

    assert_eq!(
        resolve_fleet_window_session_target("", &[session("20-homekeeper")]),
        ResolveResult::None { hints: None }
    );

    let ambiguous = vec![
        Session {
            name: "a".to_owned(),
            windows: vec![FleetWindow {
                name: Some("shared-oracle".to_owned()),
                repo: None,
            }],
        },
        Session {
            name: "b".to_owned(),
            windows: vec![FleetWindow {
                name: None,
                repo: Some("org/shared-oracle".to_owned()),
            }],
        },
    ];
    assert_eq!(
        resolve_fleet_window_session_target("shared", &ambiguous),
        ResolveResult::Ambiguous {
            candidates: ambiguous
        }
    );
}

fn window(index: u32, name: &str, current_command: &str) -> WindowInfo {
    WindowInfo {
        index,
        name: name.to_owned(),
        current_command: current_command.to_owned(),
    }
}

#[test]
fn bare_session_resolves_the_only_non_shell_window() {
    let windows = vec![window(0, "shell", "zsh"), window(1, "agent", "codex")];

    assert_eq!(
        resolve_engine_window_target("fleet", &windows),
        EngineWindowResolution::Engine(ResolvedWindowTarget {
            target: "fleet:1".to_owned(),
            window: windows[1].clone(),
        })
    );
}

#[test]
fn explicit_numeric_window_suffix_always_wins() {
    let windows = vec![window(0, "shell", "zsh"), window(1, "agent", "codex")];

    assert_eq!(
        resolve_engine_window_target("fleet:0", &windows),
        EngineWindowResolution::Explicit(ResolvedWindowTarget {
            target: "fleet:0".to_owned(),
            window: windows[0].clone(),
        })
    );
}

#[test]
fn multiple_non_shell_windows_return_typed_ambiguity() {
    let candidates = vec![window(1, "claude", "claude"), window(2, "codex", "codex")];

    assert_eq!(
        resolve_engine_window_target("fleet", &candidates),
        EngineWindowResolution::Ambiguous {
            candidates: candidates.clone(),
        }
    );
}

#[test]
fn all_shell_windows_fall_back_to_window_zero() {
    let windows = vec![window(0, "shell", "zsh"), window(1, "tools", "bash")];

    assert_eq!(
        resolve_engine_window_target("fleet", &windows),
        EngineWindowResolution::Fallback(ResolvedWindowTarget {
            target: "fleet:0".to_owned(),
            window: windows[0].clone(),
        })
    );
}
