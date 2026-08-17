// Cross-node `maw peek` (#820).
//
// `/api/capture` has served cross-node capture since serve landed; the CLI just
// had no verb for it, so the only way to read a remote pane was to curl the peer
// by hand. `maw peek <node>:<target>` closes that, using `maw hey <node>:<oracle>`
// syntax rather than a `--peer` flag so the two federated verbs address peers the
// same way.
//
// The colon form is genuinely ambiguous: tmux targets are `session:window`, so
// `a:b` can name a local window OR a remote target. Resolving that by preferring
// one silently is #818 — a local session shadowing a peer of the same name, with
// the caller never told. So when BOTH readings exist this refuses and names them,
// and the caller disambiguates. Only when exactly one reading exists do we act.

// core_impl files are include!-flattened into one module, so there is no `use
// super::*` here — siblings are already in scope.

/// A peer we can reach, resolved from the peers store.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PeekPeer {
    alias: String,
    url: String,
}

/// What `<prefix>:<rest>` turned out to mean.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PeekRoute {
    /// No colon, or the prefix names no known peer: an ordinary tmux target.
    Local,
    /// The prefix names a peer and no local session, so capture over federation.
    Remote { peer: PeekPeer, target: String },
    /// The prefix names BOTH a peer and a live local session. Refuse rather than
    /// guess — see #818 for what guessing costs.
    Ambiguous { alias: String, target: String },
}

/// Decide what `target` addresses, given what peers and local sessions exist.
///
/// Pure: both worlds are passed in, so this is testable without a peers file, a
/// tmux server, or any process-global state.
///
/// `local_sessions` is a CLOSURE, not a slice, and the ordering of the checks
/// here is load-bearing. Every early return above it must happen without it
/// being called, because listing sessions costs a tmux round-trip on every
/// single `maw peek` — and in tests it consumes a queued mock response, which
/// silently shifts what the next call receives. An unconditional list here broke
/// six existing peek tests before this was made lazy.
fn peek_route_for(
    target: &str,
    peer_for: &dyn Fn(&str) -> Option<PeekPeer>,
    local_sessions: &mut dyn FnMut() -> Vec<String>,
) -> PeekRoute {
    let Some((prefix, rest)) = target.split_once(':') else {
        return PeekRoute::Local;
    };
    if prefix.is_empty() || rest.is_empty() {
        return PeekRoute::Local;
    }
    // Peer lookup first: it is a file read, and it fails fast for the overwhelmingly
    // common `session:window` case, so tmux is never touched for a local target.
    let Some(peer) = peer_for(prefix) else {
        return PeekRoute::Local;
    };
    if local_sessions().iter().any(|session| session == prefix) {
        return PeekRoute::Ambiguous { alias: prefix.to_owned(), target: rest.to_owned() };
    }
    PeekRoute::Remote { peer, target: rest.to_owned() }
}

/// The message shown when a name is both a peer and a local session.
///
/// Names both readings and gives the exact command for each. An error that only
/// says "ambiguous" leaves the caller to guess the escape hatch, which is the
/// complaint #810 was filed about.
fn peek_ambiguous_message(alias: &str, target: &str) -> String {
    format!(
        "peek: '{alias}' is both a peer and a live local session — refusing to guess.\n  \
         remote pane:  maw peek {alias}:{target} --peer {alias}\n  \
         local window: maw peek ./{alias}:{target}\n  \
         (a local session silently shadowing a peer is #818; this refuses instead)"
    )
}

/// Strip the `./` that forces a target to be read as local.
fn peek_strip_local_marker(target: &str) -> &str {
    target.strip_prefix("./").unwrap_or(target)
}

/// The capture URL for a peer and target.
fn peek_capture_url(peer_url: &str, target: &str) -> String {
    format!(
        "{}/api/capture?target={}",
        peer_url.trim_end_matches('/'),
        peek_percent_encode(target)
    )
}

/// Percent-encode a query value.
///
/// Hand-rolled because the tree has no URL crate in this dependency layer, and a
/// tmux target is a narrow alphabet. Anything outside unreserved characters is
/// encoded, so a target can never smuggle `&` or `#` into the query string.
fn peek_percent_encode(value: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

/// Pull the pane text out of a `/api/capture` response.
fn peek_parse_capture_response(alias: &str, status: u16, body: &str) -> Result<String, String> {
    if status == 401 || status == 403 {
        return Err(format!(
            "peek: {alias} rejected our credentials (HTTP {status}) — check `maw peers list` and the federation token"
        ));
    }
    if !(200..300).contains(&status) {
        let detail = serde_json::from_str::<serde_json::Value>(body)
            .ok()
            .and_then(|value| value.get("error").and_then(|e| e.as_str()).map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(format!("peek: {alias} could not capture that target: {detail}"));
    }
    let value = serde_json::from_str::<serde_json::Value>(body)
        .map_err(|error| format!("peek: {alias} returned invalid JSON: {error}"))?;
    if let Some(error) = value.get("error").and_then(|e| e.as_str()) {
        if !error.is_empty() {
            return Err(format!("peek: {alias} could not capture that target: {error}"));
        }
    }
    value
        .get("content")
        .and_then(|c| c.as_str())
        .map(str::to_owned)
        .ok_or_else(|| format!("peek: {alias} returned no content field"))
}

/// Resolve a peer alias from the peers store, reusing kill's reader so both
/// federated verbs see the same file, the same legacy fallback, and the same
/// `PEERS_FILE`/`MAW_HOME` overrides.
fn peek_lookup_peer(alias: &str) -> Option<PeekPeer> {
    let raw = kill_read_peers_json().ok().flatten()?;
    let store = serde_json::from_str::<KillPeersStore>(&raw).ok()?;
    let url = store.peers.get(alias)?.url.as_deref()?;
    kill_validate_peer_url(url).ok()?;
    Some(PeekPeer { alias: alias.to_owned(), url: url.to_owned() })
}

/// Build the curl argv for a signed `/api/capture` GET.
///
/// Pure and separated from `peek_fetch_remote` specifically so it can be
/// checked against `kill_validate_curl_argv` without a live peer. The first
/// version inlined a raw `"\n%{http_code}"` as the `-w` value — a real newline,
/// not a marker string — and `kill_validate_curl_argv` (reused below via
/// `kill_spawn_curl`) rejects EVERY argv entry containing a control character;
/// Rust's `is_control()` counts `\n` as one. That passed review and clippy and
/// failed on the first live call to a real peer. `kill`'s own `-w` value was
/// never a raw newline for exactly this reason
/// (`KILL_PEER_HTTP_STATUS_MARKER`); this should have reused that constant
/// instead of re-deriving the same idea worse.
fn peek_capture_curl_argv(peer_url: &str, target: &str, headers: &Headers) -> Vec<String> {
    let mut argv = vec![
        "-sS".to_owned(),
        "--max-time".to_owned(),
        "10".to_owned(),
        "-w".to_owned(),
        format!("{KILL_PEER_HTTP_STATUS_MARKER}%{{http_code}}"),
    ];
    for (name, value) in headers.to_btree_map() {
        argv.push("-H".to_owned());
        argv.push(format!("{name}: {value}"));
    }
    // `--` before the URL, like kill: without it a peer URL beginning with '-'
    // would be parsed by curl as a flag. kill_validate_curl_argv enforces it.
    argv.push("--".to_owned());
    argv.push(peek_capture_url(peer_url, target));
    argv
}

/// GET the peer's `/api/capture`, signed the way every other federated call is.
fn peek_fetch_remote(peer: &PeekPeer, target: &str, from: &str) -> Result<String, String> {
    let path = format!("/api/capture?target={}", peek_percent_encode(target));
    let federation_token = load_federation_token()?;
    let peer_key = load_peer_key()?;
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("peek: clock before epoch: {error}"))?
        .as_secs()
        .try_into()
        .map_err(|_| "peek: timestamp out of range".to_owned())?;
    let headers = sign_headers_v3_at(&federation_token, &peer_key, from, "GET", &path, None, timestamp)?;
    let argv = peek_capture_curl_argv(&peer.url, target, &headers);
    let output = kill_spawn_curl(&argv)?;
    let (status, body) = kill_split_peer_http_output(&output)?;
    peek_parse_capture_response(&peer.alias, status, &body)
}

/// Our wire identity for a federated call, resolved exactly as `kill --peer` and
/// `hey` resolve it — one sender identity for every federated verb, so a peer's
/// ACL sees the same `from` no matter which command reached it.
fn peek_sender_address() -> String {
    let config = load_hey_config();
    let sender_oracle = resolve_hey_sender_oracle(&config);
    resolve_hey_wire_from(None, &config, &sender_oracle).unwrap_or_default()
}

/// Live local tmux session names, used only to detect the peer/session collision.
///
/// A tmux failure yields an empty list rather than an error: if we cannot prove a
/// local session shadows the peer, the peer reading is the one the user asked for
/// with `<peer>:<target>`, and failing the whole peek because tmux is unreachable
/// would be worse than proceeding.
fn peek_local_session_names<R: maw_tmux::TmuxRunner>(runner: &mut R) -> Vec<String> {
    runner
        .run("list-sessions", &["-F".to_owned(), "#{session_name}".to_owned()])
        .map(|raw| raw.lines().map(str::trim).filter(|line| !line.is_empty()).map(str::to_owned).collect())
        .unwrap_or_default()
}

    // Pins the live-call failure directly: the first version of peek_capture_curl_argv
    // built the -w value as "\n%{http_code}" (a raw newline), which passed clippy and
    // review but failed kill_validate_curl_argv's own control-character check on the
    // first real request. This asserts the produced argv passes that exact validator,
    // and separately proves the OLD shape would have failed it.
    #[test]
    fn peek_capture_curl_argv_passes_kill_validate_curl_argv() {
        let headers = Headers::new([("x-maw-from", "white:oracle"), ("x-maw-sig", "deadbeef")]);
        let argv = peek_capture_curl_argv("http://black.test:3467", "16-thclaws:0", &headers);
        assert!(kill_validate_curl_argv(&argv).is_ok(), "{argv:?}");
        assert!(argv.iter().any(|arg| arg.starts_with(KILL_PEER_HTTP_STATUS_MARKER)), "{argv:?}");

        // The exact regression: a raw newline in the -w value is what shipped and broke.
        let broken = vec!["-w".to_owned(), "\n%{http_code}".to_owned()];
        assert!(kill_validate_curl_argv(&broken).is_err(), "the bug this test exists to catch no longer reproduces");
    }

#[cfg(test)]
mod peek_federation_tests {
    use super::*;

    fn peer(alias: &str) -> PeekPeer {
        PeekPeer { alias: alias.to_owned(), url: format!("http://{alias}:3456") }
    }

    fn peers<'a>(known: &'a [&'a str]) -> impl Fn(&str) -> Option<PeekPeer> + 'a {
        move |name: &str| known.contains(&name).then(|| peer(name))
    }

    #[test]
    fn peek_route_plain_target_stays_local() {
        let lookup = peers(&["mba"]);
        assert_eq!(peek_route_for("reviewer", &lookup, &mut Vec::new), PeekRoute::Local);
        // session:window where the session is not a peer is an ordinary tmux target
        assert_eq!(peek_route_for("16-thclaws:0", &lookup, &mut Vec::new), PeekRoute::Local);
    }

    #[test]
    fn peek_route_known_peer_goes_remote() {
        let lookup = peers(&["mba"]);
        assert_eq!(
            peek_route_for("mba:reviewer", &lookup, &mut Vec::new),
            PeekRoute::Remote { peer: peer("mba"), target: "reviewer".to_owned() }
        );
    }

    // #818 is exactly this collision resolved silently in the local direction. A
    // caller who meant the remote pane would read the wrong machine's screen and
    // have no way to tell — so refusing is the whole point of the feature.
    #[test]
    fn peek_route_refuses_when_a_name_is_both_peer_and_local_session() {
        let lookup = peers(&["mba"]);
        assert_eq!(
            peek_route_for("mba:reviewer", &lookup, &mut || vec!["mba".to_owned()]),
            PeekRoute::Ambiguous { alias: "mba".to_owned(), target: "reviewer".to_owned() }
        );
        let message = peek_ambiguous_message("mba", "reviewer");
        assert!(message.contains("--peer mba"), "{message}");
        assert!(message.contains("./mba:reviewer"), "{message}");
    }

    #[test]
    fn peek_route_ignores_degenerate_colon_forms() {
        let lookup = peers(&["mba"]);
        assert_eq!(peek_route_for(":reviewer", &lookup, &mut Vec::new), PeekRoute::Local);
        assert_eq!(peek_route_for("mba:", &lookup, &mut Vec::new), PeekRoute::Local);
    }

    #[test]
    fn peek_capture_url_encodes_the_target() {
        assert_eq!(
            peek_capture_url("http://mba:3456/", "16-thclaws:0"),
            "http://mba:3456/api/capture?target=16-thclaws%3A0"
        );
        // A target can never smuggle a second query parameter.
        assert_eq!(
            peek_capture_url("http://mba:3456", "a&b=c"),
            "http://mba:3456/api/capture?target=a%26b%3Dc"
        );
    }

    #[test]
    fn peek_parse_capture_response_reads_content_and_reports_failures() {
        assert_eq!(
            peek_parse_capture_response("mba", 200, r#"{"content":"hello","target":"x"}"#).unwrap(),
            "hello"
        );
        let unauthorized = peek_parse_capture_response("mba", 401, "{}").unwrap_err();
        assert!(unauthorized.contains("rejected our credentials"), "{unauthorized}");
        let bad = peek_parse_capture_response("mba", 400, r#"{"error":"no such pane"}"#).unwrap_err();
        assert!(bad.contains("no such pane"), "{bad}");
        // A 200 carrying an error field is still a failure, not empty content.
        let soft = peek_parse_capture_response("mba", 200, r#"{"content":"","error":"boom"}"#).unwrap_err();
        assert!(soft.contains("boom"), "{soft}");
    }

    #[test]
    fn peek_strip_local_marker_forces_a_local_reading() {
        assert_eq!(peek_strip_local_marker("./mba:reviewer"), "mba:reviewer");
        assert_eq!(peek_strip_local_marker("mba:reviewer"), "mba:reviewer");
    }
}
