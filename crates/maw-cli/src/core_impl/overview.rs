const DISPATCH_40: &[DispatcherEntry] = &[DispatcherEntry {
    command: "overview",
    handler: Handler::Sync(run_overview_command),
}];

const OVERVIEW_SESSION: &str = "0-overview";
const OVERVIEW_PANES_PER_PAGE: usize = 9;
const OVERVIEW_PANE_COLORS: [&str; 10] = [
    "colour204",
    "colour114",
    "colour81",
    "colour220",
    "colour177",
    "colour208",
    "colour44",
    "colour196",
    "colour83",
    "colour141",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverviewArgs {
    kill: bool,
    filters: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct OverviewTarget {
    session: String,
    window: u32,
    window_name: String,
    oracle: String,
}

trait OverviewTmux {
    fn kill_session(&mut self, name: &str) -> Result<(), String>;
    fn list_all(&mut self) -> Vec<TmuxSession>;
    fn new_session(&mut self, name: &str, window: &str) -> Result<(), String>;
    fn new_window(&mut self, session: &str, name: &str) -> Result<(), String>;
    fn set(&mut self, target: &str, option: &str, value: &str) -> Result<(), String>;
    fn select_pane(&mut self, target: &str, title: &str) -> Result<(), String>;
    fn send_command(&mut self, target: &str, command: &str) -> Result<(), String>;
    fn split_window(&mut self, target: &str) -> Result<(), String>;
    fn select_layout(&mut self, target: &str, layout: &str) -> Result<(), String>;
    fn select_window(&mut self, target: &str) -> Result<(), String>;
}

struct NativeOverviewTmux {
    client: TmuxClient<maw_tmux::CommandTmuxRunner>,
}

impl NativeOverviewTmux {
    fn local() -> Self {
        Self {
            client: TmuxClient::local(),
        }
    }
}

impl OverviewTmux for NativeOverviewTmux {
    fn kill_session(&mut self, name: &str) -> Result<(), String> {
        validate_overview_tmux_target(name)?;
        self.client.kill_session(name);
        Ok(())
    }

    fn list_all(&mut self) -> Vec<TmuxSession> {
        self.client.list_all()
    }

    fn new_session(&mut self, name: &str, window: &str) -> Result<(), String> {
        validate_overview_tmux_target(name)?;
        validate_overview_tmux_token(window)?;
        self.client
            .new_session(
                name,
                &maw_tmux::NewSessionOptions {
                    window: Some(window.to_owned()),
                    ..maw_tmux::NewSessionOptions::default()
                },
            )
            .map(|_| ())
            .map_err(|error| error.message)
    }

    fn new_window(&mut self, session: &str, name: &str) -> Result<(), String> {
        validate_overview_tmux_target(session)?;
        validate_overview_tmux_token(name)?;
        self.client
            .new_window(session, name, None)
            .map_err(|error| error.message)
    }

    fn set(&mut self, target: &str, option: &str, value: &str) -> Result<(), String> {
        validate_overview_tmux_target(target)?;
        validate_overview_tmux_token(option)?;
        self.client.set(target, option, value);
        Ok(())
    }

    fn select_pane(&mut self, target: &str, title: &str) -> Result<(), String> {
        validate_overview_tmux_target(target)?;
        self.client
            .select_pane(
                target,
                &maw_tmux::SelectPaneOptions {
                    title: Some(title.to_owned()),
                },
            )
            .map_err(|error| error.message)
    }

    fn send_command(&mut self, target: &str, command: &str) -> Result<(), String> {
        validate_overview_tmux_target(target)?;
        self.client
            .send_keys(target, &[command.to_owned(), "Enter".to_owned()])
            .map_err(|error| error.message)
    }

    fn split_window(&mut self, target: &str) -> Result<(), String> {
        validate_overview_tmux_target(target)?;
        self.client
            .split_window(Some(target), &maw_tmux::SplitWindowOptions::default())
            .map(|_| ())
            .map_err(|error| error.message)
    }

    fn select_layout(&mut self, target: &str, layout: &str) -> Result<(), String> {
        validate_overview_tmux_target(target)?;
        self.client
            .select_layout(target, layout)
            .map_err(|error| error.message)
    }

    fn select_window(&mut self, target: &str) -> Result<(), String> {
        validate_overview_tmux_target(target)?;
        self.client.select_window(target);
        Ok(())
    }
}

fn run_overview_command(argv: &[String]) -> CliOutput {
    let parsed = parse_overview_args(argv);
    let mut tmux = NativeOverviewTmux::local();
    match run_overview_with_tmux(&mut tmux, &parsed, overview_port()) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("overview: {message}\n"),
        },
    }
}

fn parse_overview_args(argv: &[String]) -> OverviewArgs {
    OverviewArgs {
        kill: argv.iter().any(|arg| matches!(arg.as_str(), "--kill" | "-k")),
        filters: argv
            .iter()
            .filter(|arg| !arg.starts_with('-'))
            .cloned()
            .collect(),
    }
}

fn run_overview_with_tmux(
    tmux: &mut impl OverviewTmux,
    args: &OverviewArgs,
    port: u16,
) -> Result<String, String> {
    tmux.kill_session(OVERVIEW_SESSION)?;
    if args.kill {
        return Ok("overview killed\n".to_owned());
    }

    let targets = build_overview_targets(&tmux.list_all(), &args.filters);
    if targets.is_empty() {
        return Ok("no oracle sessions found\n".to_owned());
    }

    let pages = chunk_overview_targets(&targets);
    tmux.new_session(OVERVIEW_SESSION, "page-1")?;
    apply_overview_style(tmux, targets.len())?;

    for (page_index, page) in pages.iter().enumerate() {
        let win_name = format!("page-{}", page_index + 1);
        if page_index > 0 {
            tmux.new_window(OVERVIEW_SESSION, &win_name)?;
        }
        let window_target = format!("{OVERVIEW_SESSION}:{win_name}");
        for (pane_index, target) in page.iter().enumerate() {
            if pane_index > 0 {
                tmux.split_window(&window_target)?;
            }
            let global_index = page_index * OVERVIEW_PANES_PER_PAGE + pane_index;
            let pane_target = format!("{window_target}.{pane_index}");
            tmux.select_pane(&pane_target, &overview_pane_title(target, global_index))?;
            tmux.send_command(&pane_target, &overview_mirror_cmd(target, port))?;
            if pane_index > 0 {
                tmux.select_layout(&window_target, "tiled")?;
            }
        }
        tmux.select_layout(&window_target, overview_layout(page.len()))?;
    }

    tmux.select_window(&format!("{OVERVIEW_SESSION}:page-1"))?;
    Ok(render_overview_success(&pages))
}

fn build_overview_targets(sessions: &[TmuxSession], filters: &[String]) -> Vec<OverviewTarget> {
    sessions
        .iter()
        .filter(|session| is_overview_oracle_session(&session.name))
        .filter_map(|session| {
            let active = session
                .windows
                .iter()
                .find(|window| window.active)
                .or_else(|| session.windows.first())?;
            let oracle = session
                .name
                .split_once('-')
                .map_or(session.name.as_str(), |(_, oracle)| oracle)
                .to_owned();
            Some(OverviewTarget {
                session: session.name.clone(),
                window: active.index,
                window_name: active.name.clone(),
                oracle,
            })
        })
        .filter(|target| {
            filters.is_empty()
                || filters.iter().any(|filter| {
                    target.oracle.contains(filter) || target.session.contains(filter)
                })
        })
        .collect()
}

fn is_overview_oracle_session(name: &str) -> bool {
    name != OVERVIEW_SESSION
        && name
            .split_once('-')
            .is_some_and(|(slot, rest)| !rest.is_empty() && slot.chars().all(|ch| ch.is_ascii_digit()))
}

fn chunk_overview_targets(targets: &[OverviewTarget]) -> Vec<Vec<OverviewTarget>> {
    targets
        .chunks(OVERVIEW_PANES_PER_PAGE)
        .map(<[OverviewTarget]>::to_vec)
        .collect()
}

fn apply_overview_style(tmux: &mut impl OverviewTmux, target_count: usize) -> Result<(), String> {
    let settings = [
        ("pane-border-status", "top".to_owned()),
        ("pane-border-format", " #{pane_title} ".to_owned()),
        ("pane-border-style", "fg=colour238".to_owned()),
        ("pane-active-border-style", "fg=colour45".to_owned()),
        ("status-style", "bg=colour235,fg=colour248".to_owned()),
        ("status-left-length", "40".to_owned()),
        ("status-right-length", "60".to_owned()),
        (
            "status-left",
            format!(
                "#[fg=colour16,bg=colour204,bold] █ MAW #[fg=colour204,bg=colour238] #[fg=colour255,bg=colour238] {target_count} oracles #[fg=colour238,bg=colour235] "
            ),
        ),
        (
            "status-right",
            "#[fg=colour238,bg=colour235]#[fg=colour114,bg=colour238] ● live #[fg=colour81,bg=colour238] %H:%M #[fg=colour16,bg=colour81,bold] %d-%b ".to_owned(),
        ),
        ("status-justify", "centre".to_owned()),
        (
            "window-status-format",
            "#[fg=colour248,bg=colour235] #I:#W ".to_owned(),
        ),
        (
            "window-status-current-format",
            "#[fg=colour16,bg=colour45,bold] #I:#W ".to_owned(),
        ),
    ];
    for (option, value) in settings {
        tmux.set(OVERVIEW_SESSION, option, &value)?;
    }
    Ok(())
}

fn overview_pane_color(index: usize) -> &'static str {
    OVERVIEW_PANE_COLORS[index % OVERVIEW_PANE_COLORS.len()]
}

fn overview_pane_title(target: &OverviewTarget, index: usize) -> String {
    format!(
        "#[fg={},bold]{} ({}:{})#[default]",
        overview_pane_color(index),
        target.oracle,
        target.session,
        target.window
    )
}

fn overview_mirror_cmd(target: &OverviewTarget, port: u16) -> String {
    format!(
        "watch --color -t -n0.5 'curl -s \"http://localhost:{port}/api/mirror?target={}&lines=$(tput lines)\"'",
        overview_encode_uri_component(&format!("{}:{}", target.session, target.window))
    )
}

fn overview_layout(count: usize) -> &'static str {
    if count <= 2 {
        "even-horizontal"
    } else {
        "tiled"
    }
}

fn render_overview_success(pages: &[Vec<OverviewTarget>]) -> String {
    let count: usize = pages.iter().map(Vec::len).sum();
    let plural = if pages.len() > 1 { "s" } else { "" };
    let mut out = format!(
        "\x1b[32m✅\x1b[0m overview: {count} oracles across {} page{plural}\n",
        pages.len()
    );
    for (index, page) in pages.iter().enumerate() {
        let _ = writeln!(
            out,
            "  page-{}: {}",
            index + 1,
            page.iter()
                .map(|target| target.oracle.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    out.push_str("\n  attach: tmux attach -t 0-overview\n");
    if pages.len() > 1 {
        out.push_str("  navigate: Ctrl-b n/p (next/prev page)\n");
    }
    out
}

fn overview_port() -> u16 {
    load_hey_config_port()
        .or_else(|| std::env::var("MAW_PORT").ok()?.parse::<u16>().ok())
        .unwrap_or(31_745)
}

fn validate_overview_tmux_target(target: &str) -> Result<(), String> {
    if target.is_empty() || target.trim() != target || target.starts_with('-') {
        return Err("tmux target/session must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if !target.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ':' | '.' | '%' )
    }) {
        return Err("tmux target/session contains unsupported characters".to_owned());
    }
    Ok(())
}

fn validate_overview_tmux_token(token: &str) -> Result<(), String> {
    if token.is_empty() || token.trim() != token || token.starts_with('-') {
        return Err("tmux token must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if !token
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-'))
    {
        return Err("tmux token contains unsupported characters".to_owned());
    }
    Ok(())
}

fn overview_encode_uri_component(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')');
        if keep {
            encoded.push(char::from(byte));
        } else {
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[cfg(test)]
mod overview_tests {
    use super::*;

    #[derive(Default)]
    struct FakeOverviewTmux {
        sessions: Vec<TmuxSession>,
        calls: Vec<String>,
    }

    impl OverviewTmux for FakeOverviewTmux {
        fn kill_session(&mut self, name: &str) -> Result<(), String> {
            validate_overview_tmux_target(name)?;
            self.calls.push(format!("kill-session {name}"));
            Ok(())
        }

        fn list_all(&mut self) -> Vec<TmuxSession> {
            self.calls.push("list-all".to_owned());
            self.sessions.clone()
        }

        fn new_session(&mut self, name: &str, window: &str) -> Result<(), String> {
            self.calls.push(format!("new-session {name} {window}"));
            Ok(())
        }

        fn new_window(&mut self, session: &str, name: &str) -> Result<(), String> {
            self.calls.push(format!("new-window {session} {name}"));
            Ok(())
        }

        fn set(&mut self, target: &str, option: &str, value: &str) -> Result<(), String> {
            self.calls.push(format!("set {target} {option} {value}"));
            Ok(())
        }

        fn select_pane(&mut self, target: &str, title: &str) -> Result<(), String> {
            self.calls.push(format!("select-pane {target} {title}"));
            Ok(())
        }

        fn send_command(&mut self, target: &str, command: &str) -> Result<(), String> {
            self.calls.push(format!("send {target} {command}"));
            Ok(())
        }

        fn split_window(&mut self, target: &str) -> Result<(), String> {
            self.calls.push(format!("split-window {target}"));
            Ok(())
        }

        fn select_layout(&mut self, target: &str, layout: &str) -> Result<(), String> {
            self.calls.push(format!("select-layout {target} {layout}"));
            Ok(())
        }

        fn select_window(&mut self, target: &str) -> Result<(), String> {
            self.calls.push(format!("select-window {target}"));
            Ok(())
        }
    }

    fn session(name: &str, active: u32) -> TmuxSession {
        TmuxSession {
            name: name.to_owned(),
            windows: vec![
                maw_tmux::TmuxWindow {
                    index: 1,
                    name: "main".to_owned(),
                    active: active == 1,
                    cwd: None,
                },
                maw_tmux::TmuxWindow {
                    index: 2,
                    name: "work".to_owned(),
                    active: active == 2,
                    cwd: None,
                },
            ],
        }
    }

    #[test]
    fn overview_builds_targets_like_maw_js() {
        let sessions = vec![
            session("01-wish", 2),
            session("10-bigboy", 1),
            session("0-overview", 1),
            session("scratch", 1),
        ];
        let targets = build_overview_targets(&sessions, &["wish".to_owned()]);
        assert_eq!(
            targets,
            vec![OverviewTarget {
                session: "01-wish".to_owned(),
                window: 2,
                window_name: "work".to_owned(),
                oracle: "wish".to_owned(),
            }]
        );
    }

    #[test]
    fn overview_kill_matches_maw_js_output_and_skips_discovery() {
        let mut tmux = FakeOverviewTmux::default();
        let out = run_overview_with_tmux(
            &mut tmux,
            &OverviewArgs {
                kill: true,
                filters: vec![],
            },
            31_745,
        )
        .expect("kill succeeds");
        assert_eq!(out, "overview killed\n");
        assert_eq!(tmux.calls, vec!["kill-session 0-overview"]);
    }

    #[test]
    fn overview_success_matches_committed_golden_and_records_safe_tmux_plan() {
        let mut tmux = FakeOverviewTmux {
            sessions: vec![session("01-wish", 2), session("02-bigboy", 1)],
            calls: vec![],
        };
        let out = run_overview_with_tmux(
            &mut tmux,
            &OverviewArgs {
                kill: false,
                filters: vec![],
            },
            31_745,
        )
        .expect("overview succeeds");
        assert_eq!(
            out,
            include_str!("../../tests/fixtures/native-orchestration/overview.stdout")
        );
        assert!(tmux.calls.iter().any(|call| call == "new-session 0-overview page-1"));
        assert!(tmux.calls.iter().any(|call| call == "select-layout 0-overview:page-1 even-horizontal"));
        assert!(tmux.calls.iter().any(|call| call.contains("target=01-wish%3A2")));
        assert!(tmux.calls.iter().any(|call| call.contains("target=02-bigboy%3A1")));
    }

    #[test]
    fn overview_guard_rejects_option_injected_targets() {
        assert!(validate_overview_tmux_target("-bad").is_err());
        assert!(validate_overview_tmux_target(" good").is_err());
        assert!(validate_overview_tmux_target("good;bad").is_err());
        assert!(validate_overview_tmux_target("01-wish:2.0").is_ok());
    }

    #[test]
    fn overview_parser_ignores_color_and_dash_flags_like_maw_js() {
        let args = parse_overview_args(&[
            "--color".to_owned(),
            "wish".to_owned(),
            "--unknown".to_owned(),
            "-k".to_owned(),
        ]);
        assert_eq!(
            args,
            OverviewArgs {
                kill: true,
                filters: vec!["wish".to_owned()],
            }
        );
    }
}
