fn resolve_local_tmux_runner_target<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    query: &str,
    command: &str,
) -> Result<String, String> {
    if query.starts_with('%') {
        return Ok(query.to_owned());
    }
    let sessions = route_sessions_from_tmux_runner(runner, command)?;
    resolve_local_tmux_target_from_sessions(query, &sessions)
}

fn route_sessions_from_tmux_runner<R: maw_tmux::TmuxRunner>(
    runner: &mut R,
    command: &str,
) -> Result<Vec<RouteSession>, String> {
    let raw = runner
        .run(
            "list-windows",
            &[
                "-a".to_owned(),
                "-F".to_owned(),
                "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}".to_owned(),
            ],
        )
        .map_err(|error| format!("{command} target resolution failed: {}", error.message))?;
    Ok(tmux_sessions_to_route_sessions(maw_tmux::parse_list_all_windows(&raw)))
}

fn tmux_sessions_to_route_sessions(sessions: Vec<TmuxSession>) -> Vec<RouteSession> {
    sessions
        .into_iter()
        .map(tmux_session_to_route_session)
        .collect()
}

fn tmux_session_to_route_session(session: TmuxSession) -> RouteSession {
    RouteSession {
        name: session.name,
        source: None,
        windows: session
            .windows
            .into_iter()
            .map(|window| RouteWindow {
                index: window.index,
                name: window.name,
                active: window.active,
                kind: None,
            })
            .collect(),
    }
}

fn resolve_local_tmux_target_from_sessions(
    query: &str,
    sessions: &[RouteSession],
) -> Result<String, String> {
    match resolve_route_target(query, &RouteConfig::default(), sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => Ok(target),
        RouteResult::Peer { node, target, .. } => Err(format!(
            "cross-node target '{query}' (node '{node}', target '{target}') is not supported"
        )),
        RouteResult::Error { detail, hint, .. } => {
            if let Some(hint) = hint {
                Err(format!("{detail} — {hint}"))
            } else {
                Err(detail)
            }
        }
    }
}

fn route_result_prefer_pane_zero_for_ambiguous_agent<R: maw_tmux::TmuxRunner>(
    query: &str,
    result: RouteResult,
    runner: &mut R,
) -> RouteResult {
    match result {
        RouteResult::Local { target } => RouteResult::Local {
            target: prefer_pane_zero_for_ambiguous_agent(query, &target, runner),
        },
        RouteResult::SelfNode { target } => RouteResult::SelfNode {
            target: prefer_pane_zero_for_ambiguous_agent(query, &target, runner),
        },
        other => other,
    }
}

fn prefer_pane_zero_for_ambiguous_agent<R: maw_tmux::TmuxRunner>(
    query: &str,
    target: &str,
    runner: &mut R,
) -> String {
    let Some(agent_name) = route_agent_name_from_query(query) else {
        return target.to_owned();
    };
    let Some(window_target) = route_window_target_without_pane(target) else {
        return target.to_owned();
    };
    let Ok(raw) = runner.run(
        "list-panes",
        &["-a".to_owned(), "-F".to_owned(), maw_tmux::PANE_TARGET_FORMAT.to_owned()],
    ) else {
        return target.to_owned();
    };
    let matches = maw_tmux::pane_target_candidates_from_list_panes_output(&raw)
        .into_iter()
        .filter(|candidate| {
            candidate.source == "pane-title"
                && candidate.name.eq_ignore_ascii_case(agent_name)
                && candidate_window_target(&candidate.target).as_deref() == Some(window_target)
        })
        .collect::<Vec<_>>();
    if matches.len() <= 1 {
        return target.to_owned();
    }
    matches
        .iter()
        .find(|candidate| candidate.target.rsplit_once('.').is_some_and(|(_, pane)| pane == "0"))
        .map_or_else(|| target.to_owned(), |candidate| candidate.target.clone())
}

fn route_agent_name_from_query(query: &str) -> Option<&str> {
    let query = query.trim();
    if query.is_empty() || query.eq_ignore_ascii_case("me") || query.contains('/') {
        return None;
    }
    let name = query.split_once(':').map_or(query, |(_, name)| name);
    let (name, pane_suffix) = route_split_pane_suffix(name);
    if pane_suffix.is_some() || name.is_empty() || name.bytes().all(|byte| byte.is_ascii_digit()) {
        None
    } else {
        Some(name)
    }
}

fn route_window_target_without_pane(target: &str) -> Option<&str> {
    let (_, window) = target.split_once(':')?;
    let (_, pane_suffix) = route_split_pane_suffix(window);
    pane_suffix.is_none().then_some(target)
}

fn candidate_window_target(target: &str) -> Option<String> {
    target
        .rsplit_once('.')
        .and_then(|(window, pane)| {
            (!window.is_empty() && !pane.is_empty() && pane.bytes().all(|byte| byte.is_ascii_digit()))
                .then(|| window.to_owned())
        })
}

fn route_split_pane_suffix(value: &str) -> (&str, Option<&str>) {
    if let Some((window, pane)) = value.rsplit_once('.') {
        if !window.is_empty() && !pane.is_empty() && pane.bytes().all(|byte| byte.is_ascii_digit()) {
            return (window, Some(pane));
        }
    }
    (value, None)
}

// --- #790/#681: local-vs-remote ambiguity for `<node>:<session>` targets ---
//
// `resolve_target_with_current_session` (maw-routing) always tries an
// explicit local `session:window` match before ever considering the query's
// left half as a peer/node name (see maw-routing's
// `resolve_explicit_local_session_window_target`, tried first). That means a
// local tmux session and a federation peer can share a name, and the local
// session always wins — including when the local match then fails (e.g. "no
// such window"), which reads as a local-only problem when it is actually
// silently preempting a cross-node send. The helpers below detect that
// collision so callers can surface it, and separately widen peer lookup to
// `~/.maw/peers.json` (populated by `maw peers add`/pairing) so a peer that
// is reachable there but absent from `maw.config.json`'s `namedPeers` is not
// reported as unroutable.

/// Strip a leading `<digits>-` fleet-numbering prefix, matching how
/// maw-routing's local session matcher treats `31-black` as also answering to
/// `black`.
fn hey_strip_numeric_fleet_prefix(name: &str) -> &str {
    name.split_once('-')
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(name, |(_, rest)| rest)
}

/// True when `node` names a live local tmux session — i.e. the case where
/// maw-routing's explicit-local-target step will claim the query before any
/// peer lookup is even attempted.
fn hey_session_matches_node(sessions: &[RouteSession], node: &str) -> bool {
    let wanted = node.to_lowercase();
    sessions.iter().any(|session| {
        let name = session.name.to_lowercase();
        name == wanted || hey_strip_numeric_fleet_prefix(&name) == wanted
    })
}

/// Peer URL for `node`, consulting `maw.config.json`'s `namedPeers` first
/// (what the resolver itself uses) and falling back to `~/.maw/peers.json`
/// (#681 — the store `maw peers add`/pairing actually write, and the more
/// authoritative one per the issue).
fn hey_peer_url_for_node(node: &str, config: &RouteConfig) -> Option<String> {
    config
        .named_peers
        .iter()
        .find(|peer| peer.name == node)
        .map(|peer| peer.url.clone())
        .or_else(|| peers_load_store().peers.get(node).map(|peer| peer.url.clone()))
}

/// Like [`hey_peer_url_for_node`] but only returns a hit when it comes from
/// `peers.json` specifically — i.e. `namedPeers` does not already claim
/// `node` (the resolver would have routed it already if it did). Used to
/// decide when the peers.json fallback route (#681) applies.
fn hey_peer_url_for_node_stored_only(node: &str, config: &RouteConfig) -> Option<String> {
    if config.named_peers.iter().any(|peer| peer.name == node) {
        return None;
    }
    peers_load_store().peers.get(node).map(|peer| peer.url.clone())
}

/// #790: when `query` is `<node>:<rest>` and `node` names both a local
/// session and a federation peer, local resolution always wins (and can even
/// fail loudly, e.g. "no window 'x' in session 'node'") without ever
/// mentioning the same-named peer. Returns a note to surface that ambiguity
/// at the exact point someone would otherwise misread a local error as
/// evidence about the remote host. Local-first precedence is unchanged —
/// this only makes the collision visible.
fn hey_local_peer_collision_note(query: &str, sessions: &[RouteSession], config: &RouteConfig) -> Option<String> {
    let (node, rest) = query.split_once(':')?;
    if node.is_empty() || rest.is_empty() || node.eq_ignore_ascii_case("local") || node.eq_ignore_ascii_case("me") {
        return None;
    }
    if !hey_session_matches_node(sessions, node) {
        return None;
    }
    let peer_url = hey_peer_url_for_node(node, config)?;
    Some(format!(
        "note: '{node}' also matches peer {node} ({peer_url}); resolved locally — the peer was not contacted\n"
    ))
}

/// #681: cross-node `hey` only consulted `namedPeers`, leaving peers that are
/// registered and reachable via `~/.maw/peers.json` unroutable with an error
/// that reads as "no such peer". When the resolver comes back with an error
/// for a `<node>:<rest>` query, `node` does not name a local session (so
/// local-first precedence for #790 stays intact), and `node` is a known
/// `peers.json` alias, route through it instead of failing.
fn hey_peers_json_fallback_route(
    query: &str,
    config: &RouteConfig,
    sessions: &[RouteSession],
    result: RouteResult,
) -> RouteResult {
    let RouteResult::Error { .. } = &result else {
        return result;
    };
    let Some((node, rest)) = query.split_once(':') else {
        return result;
    };
    if node.is_empty() || rest.is_empty() || hey_session_matches_node(sessions, node) {
        return result;
    }
    let Some(peer_url) = hey_peer_url_for_node_stored_only(node, config) else {
        return result;
    };
    RouteResult::Peer { peer_url, target: rest.to_owned(), node: node.to_owned() }
}

// --- #709: verify the resolved pane still looks like an agent ---
//
// `hey <session>` (no window given) can resolve to a window/pane by
// position rather than identity. If the session was restructured — a window
// renamed, an agent's pane replaced with a shell — the same target string
// keeps resolving to *something*, and delivery still reports success. We
// cannot always tell (a codex/claude pane can be named anything), so this
// warns rather than refuses, but it turns a silent misroute into a visible
// one.

fn route_pane_looks_like_agent(command: &str, title: &str) -> bool {
    let command = command.to_ascii_lowercase();
    let title = title.to_ascii_lowercase();
    title.contains("agent")
        || title.contains("oracle")
        || title.contains("codex")
        || title.contains("claude")
        || command.contains("codex")
        || command.contains("claude")
}

fn warn_if_local_target_pane_is_not_agent<R: maw_tmux::TmuxRunner>(
    target: &str,
    runner: &mut R,
) -> Option<String> {
    let raw = runner
        .run(
            "display-message",
            &[
                "-p".to_owned(),
                "-t".to_owned(),
                target.to_owned(),
                "#{pane_current_command}|||#{pane_title}".to_owned(),
            ],
        )
        .ok()?;
    let mut parts = raw.trim_end_matches('\n').splitn(2, "|||");
    let command = parts.next().unwrap_or_default();
    let title = parts.next().unwrap_or_default();
    if command.is_empty() || route_pane_looks_like_agent(command, title) {
        return None;
    }
    Some(format!(
        "note: '{target}' resolved to a pane running '{command}', not an obvious agent pane — the session may have been restructured (#709)\n"
    ))
}

#[cfg(test)]
mod target_resolver_tests {
    use super::*;

    #[derive(Default)]
    struct FakeRunner {
        raw: String,
        calls: usize,
    }

    impl maw_tmux::TmuxRunner for FakeRunner {
        fn run(&mut self, subcommand: &str, _args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            if subcommand == "list-panes" {
                self.calls += 1;
                Ok(self.raw.clone())
            } else {
                Err(maw_tmux::TmuxError::new(format!("unexpected {subcommand}")))
            }
        }
    }

    #[test]
    fn ambiguous_agent_name_in_one_window_prefers_pane_zero() {
        let mut runner = FakeRunner {
            raw: [
                "%1|||81-kru32:0.2|||kru32-oracle||||||/tmp",
                "%2|||81-kru32:0.0|||kru32-oracle||||||/tmp",
                "%3|||81-kru32:0.1|||kru32-oracle||||||/tmp",
            ]
            .join("\n"),
            ..FakeRunner::default()
        };

        let result = route_result_prefer_pane_zero_for_ambiguous_agent(
            "81-kru32:kru32-oracle",
            RouteResult::Local { target: "81-kru32:0".to_owned() },
            &mut runner,
        );

        assert_eq!(result, RouteResult::Local { target: "81-kru32:0.0".to_owned() });
        assert_eq!(runner.calls, 1);
    }

    #[test]
    fn explicit_pane_or_single_match_keeps_resolved_target() {
        let mut explicit = FakeRunner::default();
        assert_eq!(
            prefer_pane_zero_for_ambiguous_agent("81-kru32:kru32-oracle.2", "81-kru32:0.2", &mut explicit),
            "81-kru32:0.2"
        );
        assert_eq!(explicit.calls, 0);

        let mut single = FakeRunner {
            raw: "%1|||81-kru32:0.1|||kru32-oracle||||||/tmp\n".to_owned(),
            ..FakeRunner::default()
        };
        assert_eq!(
            prefer_pane_zero_for_ambiguous_agent("81-kru32:kru32-oracle", "81-kru32:0", &mut single),
            "81-kru32:0"
        );
    }

    // --- #790/#681 ---

    fn hey_named_peer(name: &str, url: &str) -> RouteNamedPeer {
        RouteNamedPeer { name: name.to_owned(), url: url.to_owned() }
    }

    fn hey_config_with_named_peers(peers: Vec<RouteNamedPeer>) -> RouteConfig {
        RouteConfig { named_peers: peers, ..RouteConfig::default() }
    }

    fn hey_sessions(names: &[&str]) -> Vec<RouteSession> {
        names
            .iter()
            .map(|name| RouteSession {
                name: (*name).to_owned(),
                windows: vec![RouteWindow { index: 0, name: "work".to_owned(), active: true, kind: None }],
                source: None,
            })
            .collect()
    }

    struct PeersFileGuard {
        _lock: std::sync::MutexGuard<'static, ()>,
        _restore: EnvVarRestore,
        path: std::path::PathBuf,
    }

    impl PeersFileGuard {
        fn empty(label: &str) -> Self {
            let lock = env_test_lock();
            let restore = EnvVarRestore::capture("PEERS_FILE");
            let path = std::env::temp_dir().join(format!(
                "maw-rs-target-resolver-peers-{label}-{}-{}",
                std::process::id(),
                std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos())
            ));
            std::env::set_var("PEERS_FILE", &path);
            Self { _lock: lock, _restore: restore, path }
        }

        fn with_peer(label: &str, alias: &str, url: &str) -> Self {
            let guard = Self::empty(label);
            std::fs::write(
                &guard.path,
                format!(r#"{{"version":1,"peers":{{"{alias}":{{"url":"{url}","addedAt":"2026-08-13T00:00:00.000Z"}}}}}}"#),
            )
            .expect("write peers.json");
            guard
        }
    }

    #[test]
    fn collision_note_fires_when_node_matches_both_local_session_and_peer() {
        let sessions = hey_sessions(&["31-black", "other"]);
        let config = hey_config_with_named_peers(vec![hey_named_peer("black", "http://10.10.0.6:3456")]);

        let note = hey_local_peer_collision_note("black:oracle", &sessions, &config)
            .expect("collision note");
        assert!(note.contains("black"), "note should name the colliding peer: {note}");
        assert!(note.contains("http://10.10.0.6:3456"), "note should include the peer url: {note}");
        assert!(note.contains("resolved locally"), "note should say local won: {note}");

        // The numeric-fleet-prefix alias ("31-black" answering to "black")
        // must also be recognised, matching maw-routing's own local matcher.
        assert!(hey_local_peer_collision_note("black:oracle", &sessions, &config).is_some());
    }

    #[test]
    fn collision_note_is_silent_without_a_real_collision() {
        let sessions = hey_sessions(&["31-black"]);
        let config = hey_config_with_named_peers(vec![hey_named_peer("black", "http://10.10.0.6:3456")]);

        // No peer named "other" — no collision to report.
        assert!(hey_local_peer_collision_note("other:oracle", &sessions, &config).is_none());
        // "black" is a peer, but no local session named "phaith" exists.
        assert!(hey_local_peer_collision_note("phaith:oracle", &hey_sessions(&[]), &config).is_none());
        // The explicit local escape hatch is never a "collision".
        assert!(hey_local_peer_collision_note("local:oracle", &sessions, &config).is_none());
        // Bare (no ':') targets are handled by the picker, not this helper.
        assert!(hey_local_peer_collision_note("black", &sessions, &config).is_none());
    }

    #[test]
    fn collision_note_checks_peers_json_when_named_peers_has_no_match() {
        let _guard = PeersFileGuard::with_peer("collision", "black", "http://10.10.0.6:3456");
        let sessions = hey_sessions(&["31-black"]);
        let config = RouteConfig::default(); // namedPeers empty — peers.json is the only source

        let note = hey_local_peer_collision_note("black:oracle", &sessions, &config)
            .expect("collision note from peers.json");
        assert!(note.contains("http://10.10.0.6:3456"));
    }

    #[test]
    fn peers_json_fallback_routes_when_named_peers_has_no_match_and_no_local_session_collides() {
        let _guard = PeersFileGuard::with_peer("fallback", "vps", "http://vps.example:3456");
        let sessions = hey_sessions(&["unrelated"]);
        let config = RouteConfig::default();
        let error = RouteResult::Error {
            reason: "unknown_node".to_owned(),
            detail: "node 'vps' not in namedPeers or peers".to_owned(),
            hint: None,
        };

        let result = hey_peers_json_fallback_route("vps:33-maw-rs", &config, &sessions, error);

        assert_eq!(
            result,
            RouteResult::Peer {
                peer_url: "http://vps.example:3456".to_owned(),
                target: "33-maw-rs".to_owned(),
                node: "vps".to_owned(),
            }
        );
    }

    #[test]
    fn peers_json_fallback_preserves_local_first_precedence_and_non_error_results() {
        let _guard = PeersFileGuard::with_peer("precedence", "black", "http://10.10.0.6:3456");
        let config = RouteConfig::default();

        // #790: if "black" is ALSO a local session, local-first precedence
        // must hold — the peers.json fallback must not steal the route.
        let sessions_with_local = hey_sessions(&["31-black"]);
        let error = RouteResult::Error { reason: "session_window_not_found".to_owned(), detail: "no window 'oracle' in session '31-black'".to_owned(), hint: None };
        assert_eq!(
            hey_peers_json_fallback_route("black:oracle", &config, &sessions_with_local, error.clone()),
            error,
            "fallback must not override a real local-session collision"
        );

        // Non-Error results pass through untouched.
        let local = RouteResult::Local { target: "31-black:0".to_owned() };
        assert_eq!(
            hey_peers_json_fallback_route("black:oracle", &config, &hey_sessions(&[]), local.clone()),
            local
        );
    }

    #[test]
    fn peers_json_fallback_leaves_non_node_shaped_errors_alone() {
        let _guard = PeersFileGuard::empty("shapeless");
        let config = RouteConfig::default();
        let error = RouteResult::Error { reason: "not_found".to_owned(), detail: "'bare' not in local sessions or agents map".to_owned(), hint: None };
        assert_eq!(
            hey_peers_json_fallback_route("bare", &config, &hey_sessions(&[]), error.clone()),
            error
        );
    }

    // --- #709 ---

    #[derive(Default)]
    struct PaneInfoRunner {
        info: Option<String>,
        calls: Vec<Vec<String>>,
    }

    impl maw_tmux::TmuxRunner for PaneInfoRunner {
        fn run(&mut self, subcommand: &str, args: &[String]) -> Result<String, maw_tmux::TmuxError> {
            if subcommand == "display-message" {
                self.calls.push(args.to_vec());
                self.info.clone().map_or_else(|| Err(maw_tmux::TmuxError::new("no such pane")), Ok)
            } else {
                Err(maw_tmux::TmuxError::new(format!("unexpected {subcommand}")))
            }
        }
    }

    #[test]
    fn warns_when_resolved_pane_is_a_bare_shell() {
        let mut runner = PaneInfoRunner { info: Some("bash|||bash".to_owned()), ..PaneInfoRunner::default() };

        let note = warn_if_local_target_pane_is_not_agent("33-maw-rs:0", &mut runner)
            .expect("note about non-agent pane");
        assert!(note.contains("33-maw-rs:0"));
        assert!(note.contains("bash"));
        assert_eq!(runner.calls.len(), 1);
        assert!(runner.calls[0].iter().any(|arg| arg == "33-maw-rs:0"), "should query the resolved target: {:?}", runner.calls[0]);
    }

    #[test]
    fn stays_silent_when_resolved_pane_looks_like_an_agent() {
        let mut claude_command = PaneInfoRunner { info: Some("claude|||some title".to_owned()), ..PaneInfoRunner::default() };
        assert!(warn_if_local_target_pane_is_not_agent("33-maw-rs:0", &mut claude_command).is_none());

        let mut oracle_title = PaneInfoRunner { info: Some("node|||⠐ browser-oracle".to_owned()), ..PaneInfoRunner::default() };
        assert!(warn_if_local_target_pane_is_not_agent("33-maw-rs:0", &mut oracle_title).is_none());
    }

    #[test]
    fn fails_open_when_pane_command_is_unavailable() {
        let mut runner = PaneInfoRunner::default();
        assert!(warn_if_local_target_pane_is_not_agent("33-maw-rs:0", &mut runner).is_none());
    }
}
