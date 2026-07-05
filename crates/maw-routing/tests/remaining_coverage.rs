use maw_routing::{resolve_target, MawConfig, ResolveResult, Session, Window};

fn window(index: u32, name: &str) -> Window {
    Window {
        index,
        name: name.to_owned(),
        active: index == 0,
        kind: None,
    }
}

fn session(name: &str, windows: Vec<Window>) -> Session {
    Session {
        name: name.to_owned(),
        windows,
        source: None,
    }
}

#[test]
fn colon_query_without_window_uses_first_session_window() {
    let sessions = vec![session("dev", vec![window(5, "main")])];

    assert_eq!(
        resolve_target("dev:", &MawConfig::default(), &sessions),
        ResolveResult::Local {
            target: "dev:5".to_owned(),
        }
    );
}

#[test]
fn colon_numeric_window_resolves_existing_index_exactly() {
    let sessions = vec![session("dev", vec![window(4, "work"), window(5, "main")])];

    assert_eq!(
        resolve_target("dev:4", &MawConfig::default(), &sessions),
        ResolveResult::Local {
            target: "dev:4".to_owned(),
        }
    );
}

#[test]
fn colon_numeric_window_miss_is_loud_without_direct_fallback() {
    let sessions = vec![session("dev", vec![window(5, "main")])];

    assert_eq!(
        resolve_target("dev:4", &MawConfig::default(), &sessions),
        ResolveResult::Error {
            reason: "session_window_not_found".to_owned(),
            detail: "no window '4' in session 'dev'".to_owned(),
            hint: Some("windows: dev:5 (main)".to_owned()),
        }
    );
}

#[test]
fn numeric_oracle_session_aliases_resolve_to_first_window() {
    let sessions = vec![session("101-mawjs-oracle", vec![window(2, "agent")])];

    assert_eq!(
        resolve_target("mawjs", &MawConfig::default(), &sessions),
        ResolveResult::Local {
            target: "101-mawjs-oracle:2".to_owned(),
        }
    );
}

#[test]
fn substring_window_match_requires_a_single_candidate() {
    let sessions = vec![
        session("alpha", vec![window(1, "homekeeper")]),
        session("beta", vec![window(2, "homekeeper")]),
    ];

    assert!(matches!(
        resolve_target("home", &MawConfig::default(), &sessions),
        ResolveResult::Error { .. }
    ));

    assert_eq!(
        resolve_target(
            "keeper",
            &MawConfig::default(),
            &[session("solo", vec![window(3, "homekeeper")])]
        ),
        ResolveResult::Local {
            target: "solo:3".to_owned(),
        }
    );
}

#[test]
fn remaining_alias_and_substring_edges_are_stable() {
    assert_eq!(
        resolve_target(
            "mawjs",
            &MawConfig::default(),
            &[session("mawjs-oracle", vec![window(4, "main")])],
        ),
        ResolveResult::Local {
            target: "mawjs-oracle:4".to_owned(),
        }
    );

    assert_eq!(
        resolve_target(
            "mawjs",
            &MawConfig::default(),
            &[
                session("101-mawjs", vec![window(1, "main")]),
                session("mawjs-oracle", vec![window(2, "main")]),
            ],
        ),
        ResolveResult::Local {
            target: "101-mawjs:1".to_owned(),
        }
    );

    assert!(matches!(
        resolve_target(
            "home",
            &MawConfig::default(),
            &[
                session("alpha-home", vec![window(1, "main")]),
                session("beta", vec![window(2, "homebase")]),
            ],
        ),
        ResolveResult::Error { .. }
    ));
}
