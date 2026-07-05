const DISPATCH_78: &[DispatcherEntry] = &[DispatcherEntry {
    command: "kill",
    handler: Handler::Sync(kill_run_command),
}];

const KILL_USAGE: &str = "usage: maw kill <target>[:window] [--pane N] [--index N|--all] [--peer <alias>]  (see: maw sleep for graceful stop, maw done for worktrees)";
const KILL_WINDOW_FORMAT: &str =
    "#{session_name}|||#{window_index}|||#{window_name}|||#{window_active}|||#{pane_current_path}";
const KILL_PEER_API_PATH: &str = "/api/kill";
const KILL_PEER_CURL_TIMEOUT_SECONDS: &str = "5";
const KILL_PEER_HTTP_STATUS_MARKER: &str = "__MAW_HTTP_STATUS__:";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct KillOptions {
    target: String,
    pane: Option<u32>,
    index: Option<u32>,
    all: bool,
    peer: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillPeer {
    alias: String,
    url: String,
    node: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillPeerRequest {
    peer: KillPeer,
    target: String,
    pane: Option<u32>,
    index: Option<u32>,
    all: bool,
    from: String,
    peer_key: String,
    timestamp: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillPeerResponse {
    output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillSession {
    name: String,
    windows: Vec<KillWindow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct KillWindow {
    index: u32,
    name: String,
}

trait KillTmux {
    fn kill_list_sessions(&mut self) -> Result<Vec<KillSession>, String>;
    fn kill_list_panes_all(&mut self) -> Result<String, String>;
    fn kill_list_pane_indexes(&mut self, target: &str) -> Result<Vec<u32>, String>;
    fn kill_kill_session(&mut self, session: &str) -> Result<(), String>;
    fn kill_kill_window(&mut self, target: &str) -> Result<(), String>;
    fn kill_kill_pane(&mut self, target: &str) -> Result<(), String>;
}

trait KillPeerTransport {
    fn kill_peer(&mut self, request: &KillPeerRequest) -> Result<KillPeerResponse, String>;
}

struct KillSystemTmux {
    runner: maw_tmux::CommandTmuxRunner,
}

struct KillCurlPeerTransport;

impl KillSystemTmux {
    fn kill_new() -> Self {
        Self {
            runner: maw_tmux::CommandTmuxRunner::new(),
        }
    }
}

impl KillPeerTransport for KillCurlPeerTransport {
    fn kill_peer(&mut self, request: &KillPeerRequest) -> Result<KillPeerResponse, String> {
        kill_validate_peer_request(request)?;
        let body = kill_peer_body(request)?;
        let headers = sign_headers_v3_at(
            &request.peer_key,
            &request.from,
            "POST",
            KILL_PEER_API_PATH,
            Some(body.as_bytes()),
            request.timestamp,
        )?;
        let argv = kill_peer_curl_argv(&request.peer.url, &headers, &body)?;
        let output = kill_spawn_curl(&argv)?;
        let (status, body) = kill_split_peer_http_output(&output)?;
        kill_parse_peer_response(&request.peer.alias, &request.peer.url, status, &body)
    }
}

impl KillTmux for KillSystemTmux {
    fn kill_list_sessions(&mut self) -> Result<Vec<KillSession>, String> {
        kill_tmux_run(
            &mut self.runner,
            "list-windows",
            &["-a", "-F", KILL_WINDOW_FORMAT],
        )
        .map(|raw| kill_parse_sessions(&raw))
    }

    fn kill_list_panes_all(&mut self) -> Result<String, String> {
        kill_tmux_run(
            &mut self.runner,
            "list-panes",
            &["-a", "-F", maw_tmux::PANE_TARGET_FORMAT],
        )
    }

    fn kill_list_pane_indexes(&mut self, target: &str) -> Result<Vec<u32>, String> {
        kill_validate_tmux_target(target)?;
        kill_tmux_run(
            &mut self.runner,
            "list-panes",
            &["-t", target, "-F", "#{pane_index}"],
        )
        .map(|raw| kill_parse_numbers(&raw))
    }

    fn kill_kill_session(&mut self, session: &str) -> Result<(), String> {
        kill_validate_tmux_target(session)?;
        kill_tmux_run(&mut self.runner, "kill-session", &["-t", session]).map(|_| ())
    }

    fn kill_kill_window(&mut self, target: &str) -> Result<(), String> {
        kill_validate_tmux_target(target)?;
        kill_tmux_run(&mut self.runner, "kill-window", &["-t", target]).map(|_| ())
    }

    fn kill_kill_pane(&mut self, target: &str) -> Result<(), String> {
        kill_validate_tmux_target(target)?;
        kill_tmux_run(&mut self.runner, "kill-pane", &["-t", target]).map(|_| ())
    }
}

fn kill_run_command(argv: &[String]) -> CliOutput {
    kill_run_command_with(
        argv,
        &mut KillSystemTmux::kill_new(),
        &mut KillCurlPeerTransport,
        &load_hey_config(),
        load_peer_key,
        kill_now_seconds,
    )
}

fn kill_run_command_with(
    argv: &[String],
    tmux: &mut impl KillTmux,
    peer: &mut impl KillPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> CliOutput {
    match kill_run(argv, tmux, peer, config, peer_key, now) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

fn kill_run(
    argv: &[String],
    tmux: &mut impl KillTmux,
    peer: &mut impl KillPeerTransport,
    config: &HeyConfig,
    peer_key: fn() -> Result<String, String>,
    now: fn() -> i64,
) -> Result<String, String> {
    let options = kill_parse_args(argv)?;
    if options.peer.is_some() {
        return kill_peer_forward(&options, peer, config, peer_key, now);
    }
    kill_validate_user_target(&options.target)?;
    let (raw_session, raw_window) = kill_split_target(&options.target);
    kill_validate_user_target(&raw_session)?;
    let sessions = tmux.kill_list_sessions()?;
    kill_resolve_and_apply(tmux, &sessions, &raw_session, &raw_window, &options)
}

fn kill_parse_args(argv: &[String]) -> Result<KillOptions, String> {
    let mut options = KillOptions::default();
    let mut index = 0;
    while index < argv.len() {
        index += kill_parse_arg(argv, index, &mut options)?;
    }
    if options.target.is_empty() || options.target == "--help" || options.target == "-h" {
        return Err(KILL_USAGE.to_owned());
    }
    Ok(options)
}

