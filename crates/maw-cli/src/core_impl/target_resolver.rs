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

// --- #790/#681/#818: local-vs-remote ambiguity for `<node>:<session>` ---
//
// `resolve_target_with_current_session` (maw-routing) always tries an
// explicit local `session:window` match before ever considering the query's
// left half as a peer/node name (see maw-routing's
// `resolve_explicit_local_session_window_target`, tried first). That means a
// local tmux session and a federation peer can share a name, and the local
// session always wins — including when the local match then fails (e.g. "no
// such window"), which reads as a local-only problem when it is actually
// preempting a cross-node send. #790 made that collision visible with a note;
// #818 stops `hey` from resolving it at all (the note lost against a delivery
// confirmation) and adds the `peer:` prefix as the way out. The helpers below
// detect the collision, build the refusal, and force the peer route, and
// separately widen peer lookup to `~/.maw/peers.json` (populated by `maw peers
// add`/pairing) so a peer that is reachable there but absent from
// `maw.config.json`'s `namedPeers` is not reported as unroutable.

/// Strip a leading `<digits>-` fleet-numbering prefix, matching how
/// maw-routing's local session matcher treats `31-black` as also answering to
/// `black`.
fn hey_strip_numeric_fleet_prefix(name: &str) -> &str {
    name.split_once('-')
        .filter(|(prefix, _)| !prefix.is_empty() && prefix.bytes().all(|byte| byte.is_ascii_digit()))
        .map_or(name, |(_, rest)| rest)
}

/// The live local tmux session `node` names, if any — i.e. the session whose
/// existence makes maw-routing's explicit-local-target step claim the query
/// before any peer lookup is even attempted. Returns the session's real name
/// so a refusal can print the pane it would actually have hit.
fn hey_local_session_matching_node<'a>(sessions: &'a [RouteSession], node: &str) -> Option<&'a str> {
    let wanted = node.to_lowercase();
    sessions
        .iter()
        .find(|session| {
            let name = session.name.to_lowercase();
            name == wanted || hey_strip_numeric_fleet_prefix(&name) == wanted
        })
        .map(|session| session.name.as_str())
}

fn hey_session_matches_node(sessions: &[RouteSession], node: &str) -> bool {
    hey_local_session_matching_node(sessions, node).is_some()
}

/// Peer URL for `node`, consulting `maw.config.json`'s `namedPeers` first
/// (what the resolver itself uses) and falling back to `~/.maw/peers.json`
/// (#681 — the store `maw peers add`/pairing actually write, and the more
/// authoritative one per the issue).
///
/// Case-insensitive, matching [`hey_local_session_matching_node`]'s own
/// lowercasing. It was exact-match before: a node typed with different
/// capitalization than the configured peer name (`White` vs a peer
/// registered as `white`) found no peer here, so the collision guard never
/// fired and the query fell through to local delivery — reopening #818
/// itself, on a capitalization mismatch, in the fix meant to close it.
fn hey_peer_url_for_node(node: &str, config: &RouteConfig) -> Option<String> {
    let wanted = node.to_lowercase();
    config
        .named_peers
        .iter()
        .find(|peer| peer.name.to_lowercase() == wanted)
        .map(|peer| peer.url.clone())
        .or_else(|| {
            peers_load_store()
                .peers
                .iter()
                .find(|(alias, _)| alias.to_lowercase() == wanted)
                .map(|(_, peer)| peer.url.clone())
        })
}

/// Like [`hey_peer_url_for_node`] but only returns a hit when it comes from
/// `peers.json` specifically — i.e. `namedPeers` does not already claim
/// `node` (the resolver would have routed it already if it did). Used to
/// decide when the peers.json fallback route (#681) applies.
///
/// Deliberately CASE-SENSITIVE, unlike `hey_peer_url_for_node` — round 2 of
/// #818 made this case-insensitive for consistency and that was itself an
/// over-fire: the real upstream resolver (the external `maw-routing` crate)
/// matches `namedPeers` with exact case, and this function's whole job is to
/// decide whether `namedPeers` "already claims" a node so the #681 fallback
/// knows whether to defer to it. A case-insensitive check here can believe
/// `namedPeers` claims a node (e.g. `"White"`) that upstream's case-sensitive
/// match does NOT recognise for a differently-cased query (`"white"`),
/// suppressing the fallback and turning a route that used to work into a
/// hard error. This function must agree with upstream's own case rule, not
/// with the local-session collision guard's rule — they are different
/// questions answered by different code.
fn hey_peer_url_for_node_stored_only(node: &str, config: &RouteConfig) -> Option<String> {
    if config.named_peers.iter().any(|peer| peer.name == node) {
        return None;
    }
    peers_load_store().peers.get(node).map(|peer| peer.url.clone())
}

/// The `<node>` half of a `<node>:<rest>` query that names BOTH a live local
/// session and a known federation peer: `(local session name, peer url)`.
/// `local:`/`me` are explicit same-node forms, never a collision, and the
/// `peer:` escape hatch below is resolved before this is ever consulted.
fn hey_local_peer_collision(
    query: &str,
    sessions: &[RouteSession],
    config: &RouteConfig,
) -> Option<(String, String)> {
    let (node, rest) = query.split_once(':')?;
    if node.is_empty() || rest.is_empty() || node.eq_ignore_ascii_case("local") || node.eq_ignore_ascii_case("me") {
        return None;
    }
    let session = hey_local_session_matching_node(sessions, node)?;
    let peer_url = hey_peer_url_for_node(node, config)?;
    Some((session.to_owned(), peer_url))
}

/// #818: local-first precedence used to *win* this collision — the message
/// landed in a local pane, `hey` exited 0 with a delivery confirmation, and
/// the only contrary signal was the #790 prose note. Every other signal
/// (exit code, "delivered →" line, real text in a real pane) confirmed the
/// operator's "I paired this peer, I sent to it" model, so the misroute only
/// surfaced when the *remote* side was checked and found empty — during a
/// bring-up, which is exactly when peer names are newest and most likely to
/// collide. Guessing is the defect, so refuse and name both candidates plus
/// both unambiguous forms. Peer routes are exempt: nothing was shadowed.
fn hey_local_peer_collision_refusal(
    command: &str,
    query: &str,
    sessions: &[RouteSession],
    config: &RouteConfig,
    result: &RouteResult,
) -> Option<CliOutput> {
    if command != "hey" || matches!(result, RouteResult::Peer { .. }) {
        return None;
    }
    let (node, rest) = query.split_once(':')?;
    let (session, peer_url) = hey_local_peer_collision(query, sessions, config)?;
    Some(CliOutput {
        code: send_error_code(command),
        stdout: String::new(),
        stderr: format!(
            "{command}: refusing to guess — '{node}' names both the local tmux session '{session}' and federation peer '{node}' ({peer_url}); delivering locally would NOT contact the peer (#818)\n  peer:   maw {command} peer:{node}:{rest} <message>\n  local:  maw {command} local:{node}:{rest} <message>\n"
        ),
    })
}

/// #818 escape hatch: `peer:<node>:<target>` addresses a federation peer
/// unambiguously, so a peer may keep the natural name of the machine it runs
/// on instead of being renamed to something that cannot collide with a local
/// session. Mirrors the `local:` prefix maw-routing already honours for the
/// other side of the same ambiguity, and resolves before local matching so
/// no local session can shadow it.
///
/// Only claims a query SHAPED like `peer:<node>:<target>` (two colons) —
/// never unconditionally on the literal string `"peer:"`. The first version
/// claimed every `hey` target starting with that string, so an ORDINARY
/// session literally named "peer" (`maw hey peer:oracle` — one colon, an
/// entirely normal `<session>:<window>` form with zero peers configured
/// anywhere) became permanently unreachable via `hey`, erroring with advice
/// that could not fix the caller's actual problem. Real tmux targets never
/// have a second colon (panes use `.`, not `:`), so a one-colon `rest` falls
/// through silently — it was never this escape hatch's business.
///
/// A GENUINE two-colon shape is still ambiguous, though, and round 2 of this
/// fix missed it: `peer:01-hojo:3` is EXACTLY what a real peer literally
/// named `peer` (nothing currently forbids that name) types to reach its own
/// canonical `<node>:<session>:<window>` address (#410) — same three
/// colon-separated parts as the escape hatch `peer:white:white`, no syntactic
/// difference at all. So before claiming the two-colon shape, check whether
/// "peer" ITSELF resolves to a real local session or registered peer. If it
/// does, this is the identical ambiguity #818 already refuses for every
/// other name — refuse here too rather than silently picking the keyword
/// reading over that peer's own address. Only when no real "peer" exists
/// (the overwhelmingly common case) does the escape-hatch reading proceed.
fn hey_forced_peer_route(
    command: &str,
    query: &str,
    sessions: &[RouteSession],
    config: &RouteConfig,
) -> Option<RouteResult> {
    if command != "hey" {
        return None;
    }
    let rest = query.strip_prefix("peer:")?;
    let (node, target) = rest.split_once(':').filter(|(node, target)| !node.is_empty() && !target.is_empty())?;
    let real_peer_named_peer = hey_local_session_matching_node(sessions, "peer").is_some()
        || hey_peer_url_for_node("peer", config).is_some();
    if real_peer_named_peer {
        return Some(RouteResult::Error {
            reason: "ambiguous_peer_keyword".to_owned(),
            detail: format!(
                "'peer' is both the peer:<node>:<target> keyword and the name of a real peer or session — refusing to guess whether '{query}' means the keyword or that peer's own address"
            ),
            // NOT `local:peer:{rest}` (round 3's own regression, caught on adversarial
            // review of round 3 itself): `rest` is "<node>:<target>", so that hint would
            // read "local:peer:<node>:<target>" -- and maw-routing's local resolver
            // (`resolve_target_with_current_session` -> `find_window`) only splits the
            // post-"local:" remainder on the FIRST colon, so it goes looking for a
            // window literally named "<node>:<target>", which never exists. `target`
            // alone (what's actually being addressed once "peer" is understood as the
            // session) is the one that resolves.
            hint: Some(format!("rename the peer, or use local:peer:{target} for the local session named 'peer'")),
        });
    }
    Some(hey_peer_url_for_node(node, config).map_or_else(
        || RouteResult::Error {
            reason: "unknown_node".to_owned(),
            detail: format!("no peer named '{node}' in namedPeers or the peer store"),
            hint: Some("check `maw peers list`".to_owned()),
        },
        |peer_url| RouteResult::Peer { peer_url, target: target.to_owned(), node: node.to_owned() },
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

// #813: this was the reported divergence. The copy here checked the title's
// four keywords plus `codex`/`claude` in the command -- but a live Claude Code
// pane reports its VERSION (`2.1.233`) as the command and its current task
// line as the title, so on npm-launched hosts this warned at 7 of 7 real agent
// panes. Now delegated to the one shared predicate.
fn route_pane_looks_like_agent(command: &str, title: &str) -> bool {
    maw_tmux::is_agent_pane(Some(command), Some(title))
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
        _restore: EnvVarRestore,
        path: std::path::PathBuf,
        _lock: crate::test_env::EnvLockGuard,
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
            Self { _restore: restore, path, _lock: lock }
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

    /// #818's field report verbatim: local session `11-white` (window
    /// `white`) and a peer *also* named `white`.
    fn hey_818_sessions() -> Vec<RouteSession> {
        vec![RouteSession {
            name: "11-white".to_owned(),
            windows: vec![RouteWindow { index: 3, name: "white".to_owned(), active: true, kind: None }],
            source: None,
        }]
    }

    #[test]
    fn colliding_node_name_refuses_instead_of_delivering_locally() {
        let sessions = hey_818_sessions();
        let config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);

        // The misroute itself, unchanged: maw-routing hands back a LOCAL pane
        // for a query that names a peer, so nothing downstream ever contacts
        // it. This is the state `hey` used to deliver into with exit 0.
        let result = resolve_route_target("white:white", &config, &sessions);
        assert_eq!(result, RouteResult::Local { target: "11-white:3".to_owned() });

        let refusal = hey_local_peer_collision_refusal("hey", "white:white", &sessions, &config, &result)
            .expect("#818: an ambiguous peer/session name must refuse, not deliver locally");
        assert_ne!(refusal.code, 0, "a refusal must not exit 0");
        assert!(refusal.stdout.is_empty(), "nothing was delivered: {}", refusal.stdout);
        // Both candidates named, and both unambiguous forms offered.
        assert!(refusal.stderr.contains("11-white"), "{}", refusal.stderr);
        assert!(refusal.stderr.contains("http://192.168.1.164:3456"), "{}", refusal.stderr);
        assert!(refusal.stderr.contains("peer:white:white"), "{}", refusal.stderr);
        assert!(refusal.stderr.contains("local:white:white"), "{}", refusal.stderr);
    }

    #[test]
    fn collision_refusal_is_silent_without_a_real_collision() {
        let sessions = hey_sessions(&["31-black"]);
        let config = hey_config_with_named_peers(vec![hey_named_peer("black", "http://10.10.0.6:3456")]);
        let local = RouteResult::Local { target: "31-black:0".to_owned() };
        let refuse = |query: &str, sessions: &[RouteSession]| {
            hey_local_peer_collision_refusal("hey", query, sessions, &config, &local)
        };

        // No peer named "other" — no collision to report.
        assert!(refuse("other:oracle", &sessions).is_none());
        // "black" is a peer, but no local session named "phaith" exists.
        assert!(refuse("phaith:oracle", &hey_sessions(&[])).is_none());
        // The explicit local escape hatch is never a "collision".
        assert!(refuse("local:oracle", &sessions).is_none());
        // Bare (no ':') targets are handled by the picker, not this helper.
        assert!(refuse("black", &sessions).is_none());
        // `send` keeps its historical routing; only `hey` gained the refusal.
        assert!(hey_local_peer_collision_refusal("send", "black:oracle", &sessions, &config, &local).is_none());
        // A route that already reached the peer shadowed nothing.
        let peer = RouteResult::Peer {
            peer_url: "http://10.10.0.6:3456".to_owned(),
            target: "oracle".to_owned(),
            node: "black".to_owned(),
        };
        assert!(hey_local_peer_collision_refusal("hey", "black:oracle", &sessions, &config, &peer).is_none());
    }

    #[test]
    fn collision_refusal_checks_peers_json_when_named_peers_has_no_match() {
        let _guard = PeersFileGuard::with_peer("collision", "black", "http://10.10.0.6:3456");
        let sessions = hey_sessions(&["31-black"]);
        let config = RouteConfig::default(); // namedPeers empty — peers.json is the only source
        let local = RouteResult::Local { target: "31-black:0".to_owned() };

        let refusal = hey_local_peer_collision_refusal("hey", "black:oracle", &sessions, &config, &local)
            .expect("collision refusal from peers.json");
        assert!(refusal.stderr.contains("http://10.10.0.6:3456"), "{}", refusal.stderr);
    }

    #[test]
    fn forced_peer_prefix_routes_past_a_same_named_local_session() {
        let sessions = hey_818_sessions();
        let config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);

        // The escape hatch resolves before local matching, so the local
        // session that shadowed "white:white" cannot claim this form...
        let forced = hey_forced_peer_route("hey", "peer:white:white", &sessions, &config).expect("forced peer route");
        assert_eq!(
            forced,
            RouteResult::Peer {
                peer_url: "http://192.168.1.164:3456".to_owned(),
                target: "white".to_owned(),
                node: "white".to_owned(),
            }
        );
        // ...and being a Peer route, it is exempt from the collision refusal.
        assert!(hey_local_peer_collision_refusal("hey", "peer:white:white", &sessions, &config, &forced).is_none());

        // A multi-colon target keeps its window suffix (#410 form).
        assert_eq!(
            hey_forced_peer_route("hey", "peer:white:01-hojo:3", &sessions, &config),
            Some(RouteResult::Peer {
                peer_url: "http://192.168.1.164:3456".to_owned(),
                target: "01-hojo:3".to_owned(),
                node: "white".to_owned(),
            })
        );
    }

    #[test]
    fn forced_peer_prefix_errors_clearly_and_leaves_other_targets_alone() {
        let _guard = PeersFileGuard::empty("forced");
        let config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);

        let unknown = hey_forced_peer_route("hey", "peer:ghost:oracle", &[], &config).expect("unknown peer error");
        let RouteResult::Error { reason, detail, .. } = &unknown else { panic!("expected an error: {unknown:?}") };
        assert_eq!(reason, "unknown_node");
        assert!(detail.contains("ghost"), "{detail}");

        // Untouched: ordinary targets, and every non-`hey` verb.
        assert!(hey_forced_peer_route("hey", "white:white", &[], &config).is_none());
        assert!(hey_forced_peer_route("send", "peer:white:white", &[], &config).is_none());
    }

    // #818 rejection, round 2: `hey_forced_peer_route` used to claim ANY query
    // starting with the literal string "peer:", so an ordinary local session
    // named "peer" (one colon, e.g. `peer:oracle` -- a ✔ordinary
    // <session>:<window> form) became permanently unreachable via `hey`, with
    // zero peers configured anywhere. Real tmux targets never have a SECOND
    // colon (panes use `.`), so the fix only claims the two-colon shape.
    #[test]
    fn forced_peer_prefix_falls_through_on_an_ordinary_one_colon_session_named_peer() {
        let config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);

        // The exact reported over-fire: a session literally named "peer",
        // zero relevant peers configured. Must fall through, not error.
        assert!(hey_forced_peer_route("hey", "peer:oracle", &[], &config).is_none());

        // Same shape with a dot-form pane suffix -- still one colon.
        assert!(hey_forced_peer_route("hey", "peer:reviewer.0", &[], &config).is_none());

        // A genuine two-colon shape with an unknown node is still a likely
        // TYPO of the escape hatch, not an ordinary local target (tmux
        // targets don't have two colons) -- keeps the helpful error rather
        // than silently falling through to a worse, unrelated failure.
        let _guard = PeersFileGuard::empty("forced-typo");
        let typo = hey_forced_peer_route("hey", "peer:withe:oracle", &[], &config).expect("unknown peer error");
        let RouteResult::Error { reason, .. } = &typo else { panic!("expected an error: {typo:?}") };
        assert_eq!(reason, "unknown_node");
    }

    // #818 rejection, round 3: adversarial verification found round 2's own
    // two-colon heuristic still over-fires. `peer:01-hojo:3` is EXACTLY what a
    // real peer literally named "peer" (nothing forbids that name) types to
    // reach its own canonical #410 address `<node>:<session>:<window>` -- same
    // three colon-separated parts as the escape hatch `peer:white:white`, no
    // syntactic difference. Refuse rather than guess, the same philosophy
    // #818 already applies to every OTHER name collision.
    #[test]
    fn forced_peer_prefix_refuses_when_a_real_peer_is_literally_named_peer() {
        let config = hey_config_with_named_peers(vec![hey_named_peer("peer", "http://peer-node.test:3456")]);

        let refusal = hey_forced_peer_route("hey", "peer:01-hojo:3", &[], &config).expect("ambiguous keyword error");
        let RouteResult::Error { reason, detail, hint } = &refusal else { panic!("expected an error: {refusal:?}") };
        assert_eq!(reason, "ambiguous_peer_keyword");
        assert!(detail.contains("'peer'"), "{detail}");
        assert!(hint.as_deref().is_some_and(|h| h.contains("local:peer:3")), "{hint:?}");

        // Same ambiguity when "peer" is a live LOCAL session rather than a
        // registered peer -- both sources of a real "peer" must trigger it.
        let sessions = vec![RouteSession { name: "peer".to_owned(), windows: vec![], source: None }];
        let no_named_peers = RouteConfig::default();
        let refusal2 = hey_forced_peer_route("hey", "peer:oracle:0", &sessions, &no_named_peers).expect("ambiguous keyword error");
        assert!(matches!(refusal2, RouteResult::Error { reason, .. } if reason == "ambiguous_peer_keyword"));

        // Unaffected: no real "peer" anywhere -- the ordinary escape hatch
        // still works exactly as round 2 fixed it.
        let clean_config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);
        assert!(hey_forced_peer_route("hey", "peer:white:white", &[], &clean_config).is_some());
    }

    // Adversarial review of round 3 itself (the fix above) found that its hint
    // text told the operator to type an address that does not resolve:
    // `local:peer:01-hojo:3` looks fine to a human, but maw-routing's local
    // resolver only splits the post-"local:" remainder on the FIRST colon, so
    // it searches for a window literally named "01-hojo:3" (never exists).
    // This test drives the ACTUAL local resolver with the hint text this
    // function emits, proving the recovery advice is followable, not just
    // that it contains the right substring.
    #[test]
    fn ambiguous_peer_keyword_hint_actually_resolves_locally() {
        let sessions = vec![RouteSession {
            name: "peer".to_owned(),
            windows: vec![RouteWindow { index: 3, name: "01-hojo".to_owned(), active: true, kind: None }],
            source: None,
        }];
        let config = RouteConfig::default();

        let refusal = hey_forced_peer_route("hey", "peer:01-hojo:3", &sessions, &config).expect("ambiguous keyword error");
        let RouteResult::Error { hint, .. } = &refusal else { panic!("expected an error: {refusal:?}") };
        let hint = hint.as_deref().expect("hint present");
        let suggested = hint
            .split_whitespace()
            .find(|word| word.starts_with("local:peer:"))
            .unwrap_or_else(|| panic!("hint has no local:peer: suggestion: {hint}"));

        let resolved = resolve_route_target(suggested, &config, &sessions);
        assert_eq!(
            resolved,
            RouteResult::SelfNode { target: "peer:3".to_owned() },
            "the hint's own suggested address ('{suggested}') must resolve to the local session it names"
        );
    }

    // Case-insensitivity must match on both sides of the collision guard, or a
    // capitalization mismatch on `hey`'s node reopens #818 itself: the local
    // side already lowercases (hey_local_session_matching_node), so the peer
    // side must too.
    #[test]
    fn hey_peer_url_for_node_matches_case_insensitively() {
        let config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);
        assert_eq!(hey_peer_url_for_node("White", &config).as_deref(), Some("http://192.168.1.164:3456"));
        assert_eq!(hey_peer_url_for_node("WHITE", &config).as_deref(), Some("http://192.168.1.164:3456"));
        assert_eq!(hey_peer_url_for_node("white", &config).as_deref(), Some("http://192.168.1.164:3456"));
    }

    // End-to-end: before this fix, `White:white` (peer configured lowercase
    // "white") found no peer at the collision guard -- case-sensitive lookup
    // -- so the guard never fired and the query delivered locally, exit 0,
    // silently. That is #818 itself, reopened by a capitalization mismatch in
    // the very fix meant to close it.
    #[test]
    fn colliding_node_name_refuses_even_with_a_capitalization_mismatch() {
        let sessions = hey_818_sessions();
        let config = hey_config_with_named_peers(vec![hey_named_peer("white", "http://192.168.1.164:3456")]);
        let result = resolve_route_target("White:white", &config, &sessions);
        let refusal = hey_local_peer_collision_refusal("hey", "White:white", &sessions, &config, &result)
            .expect("case mismatch must still refuse, not silently deliver locally");
        assert!(refusal.stderr.contains("11-white"), "{}", refusal.stderr);
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

    // #818 rejection, round 3, over-fire 2: round 2 made this fallback's
    // namedPeers check case-INsensitive "for consistency" with the collision
    // guard. But this function decides whether namedPeers already claims a
    // node -- and the real upstream resolver (maw-routing) matches namedPeers
    // with exact case. A namedPeers entry "White" and a peers.json entry
    // "white" (plausible: one from committed config, one from `maw peers
    // add`) used to route via #681's fallback for a query "white"; a
    // case-insensitive check here wrongly believes namedPeers "already
    // claims" it and suppresses the fallback, turning a working route into
    // an error. Must stay case-sensitive, matching upstream's own rule.
    #[test]
    fn peers_json_fallback_stays_case_sensitive_against_named_peers() {
        let _guard = PeersFileGuard::with_peer("fallback-case", "white", "http://legacy-white.example:3456");
        let sessions = hey_sessions(&["unrelated"]);
        let config = hey_config_with_named_peers(vec![hey_named_peer("White", "http://different-white.example:3456")]);
        let error = RouteResult::Error {
            reason: "unknown_node".to_owned(),
            detail: "node 'white' not in namedPeers or peers".to_owned(),
            hint: None,
        };

        let result = hey_peers_json_fallback_route("white:33-maw-rs", &config, &sessions, error);

        assert_eq!(
            result,
            RouteResult::Peer {
                peer_url: "http://legacy-white.example:3456".to_owned(),
                target: "33-maw-rs".to_owned(),
                node: "white".to_owned(),
            },
            "a case-different namedPeers entry must not suppress the peers.json fallback"
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
