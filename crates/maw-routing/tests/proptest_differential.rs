use maw_routing::{resolve_target, MawConfig, NamedPeer, ResolveResult, Session, Window};
use proptest::prelude::*;
use std::collections::HashMap;

fn names() -> impl Strategy<Value = String> {
    prop_oneof![
        Just(String::new()),
        Just(" ".to_owned()),
        Just("33-maw-rs".to_owned()),
        Just("codex".to_owned()),
        Just("codex2".to_owned()),
        Just("บ้าน-codex".to_owned()),
        "[a-z0-9][a-z0-9-]{0,18}".prop_map(|s| s),
    ]
}

fn win(index: u32, name: String, active: bool) -> Window {
    Window {
        index,
        name,
        active,
        kind: None,
    }
}

fn window_strategy() -> impl Strategy<Value = Window> {
    (0u32..10, names(), any::<bool>()).prop_map(|(index, name, active)| win(index, name, active))
}

fn session_strategy() -> impl Strategy<Value = Session> {
    (
        names(),
        prop::collection::vec(window_strategy(), 0..10),
        prop_oneof![Just(None), Just(Some("local".to_owned()))],
    )
        .prop_map(|(name, windows, source)| Session {
            name,
            windows,
            source,
        })
}

fn sessions_strategy() -> impl Strategy<Value = Vec<Session>> {
    prop::collection::vec(session_strategy(), 0..8)
}

fn peer_config() -> MawConfig {
    MawConfig {
        node: Some("selfnode".to_owned()),
        named_peers: vec![NamedPeer {
            name: "peerbox".to_owned(),
            url: "http://peerbox.local:3457".to_owned(),
        }],
        peers: vec!["http://farbox.wg:3458".to_owned()],
        agents: HashMap::from([
            ("remoteagent".to_owned(), "peerbox".to_owned()),
            ("selfagent".to_owned(), "selfnode".to_owned()),
        ]),
    }
}

fn config_strategy() -> impl Strategy<Value = MawConfig> {
    prop_oneof![Just(MawConfig::default()), Just(peer_config())]
}

fn target_strategy() -> impl Strategy<Value = String> {
    prop_oneof![
        names(),
        (names(), names()).prop_map(|(s, w)| format!("{s}:{w}")),
        (names(), 0u32..10, 0u32..4).prop_map(|(s, w, p)| format!("{s}:{w}.{p}")),
        ("[a-z]{1,8}", names()).prop_map(|(n, t)| format!("{n}:{t}")),
        (0u32..9999).prop_map(|p| format!("%{p}")),
    ]
}

fn target_exists(target: &str, sessions: &[Session]) -> bool {
    let (session_name, window_part) = target.split_once(':').unwrap_or((target, ""));
    if window_part.is_empty() {
        return sessions.iter().any(|session| session.name == session_name);
    }
    let window = window_part.split('.').next().unwrap_or(window_part);
    sessions
        .iter()
        .filter(|session| session.name == session_name)
        .flat_map(|session| &session.windows)
        .any(|w| w.index.to_string() == window || w.name.eq_ignore_ascii_case(window))
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, max_shrink_iters: 256, .. ProptestConfig::default() })]

    #[test]
    fn generated_routing_inputs_never_panic_and_are_deterministic(query in target_strategy(), config in config_strategy(), sessions in sessions_strategy()) {
        let first = resolve_target(&query, &config, &sessions);
        let second = resolve_target(&query, &config, &sessions);
        prop_assert_eq!(&first, &second);
        if let ResolveResult::Local { target } | ResolveResult::SelfNode { target } = first {
            prop_assert!(target_exists(&target, &sessions), "resolved {target:?} outside generated sessions {sessions:?}");
        }
    }

    #[test]
    fn exact_session_window_priority(session in "[a-z][a-z0-9-]{1,12}", window in "[a-z][a-z0-9-]{1,12}", index in 0u32..10) {
        let sessions = vec![
            Session { name: session.clone(), windows: vec![win(index, window.clone(), true)], source: None },
            Session { name: format!("99-{session}-shadow"), windows: vec![win(9, format!("{window}-shadow"), false)], source: None },
        ];
        prop_assert_eq!(resolve_target(&format!("{session}:{window}"), &MawConfig::default(), &sessions), ResolveResult::Local { target: format!("{session}:{index}") });
    }

    #[test]
    fn ambiguous_exact_window_names_are_not_silently_chosen(name in "[a-z][a-z0-9-]{1,12}") {
        let sessions = vec![
            Session { name: "01-left".to_owned(), windows: vec![win(1, name.clone(), true)], source: None },
            Session { name: "02-right".to_owned(), windows: vec![win(1, name.clone(), true)], source: None },
        ];
        let ambiguous = matches!(resolve_target(&name, &MawConfig::default(), &sessions), ResolveResult::Error { .. });
        prop_assert!(ambiguous);
    }
}
