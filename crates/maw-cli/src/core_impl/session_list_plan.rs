const LS_WATCH_DEFAULT_SECS: u64 = 2;
const LS_WATCH_ENTER: &str = "\x1b[?1049h\x1b[?25l";
const LS_WATCH_REPAINT: &str = "\x1b[H\x1b[2J";
const LS_WATCH_RESTORE: &str = "\x1b[?25h\x1b[?1049l";

fn parse_ls_pane(value: &str) -> Result<TmuxPane, String> {
    parse_discover_pane(value).map_err(|message| message.replacen("discover:", "ls:", 1))
}

fn parse_ls_watch_seconds(raw: &str) -> Option<u64> {
    let value = raw.trim().parse::<u64>().ok()?;
    (value > 0).then_some(value)
}

fn parse_ls_duration_seconds(raw: &str) -> Option<u64> {
    let trimmed = raw.trim().to_lowercase();
    let (digits, multiplier) = match trimmed.as_bytes().last().copied() {
        Some(b's') => (&trimmed[..trimmed.len() - 1], 1),
        Some(b'm') => (&trimmed[..trimmed.len() - 1], 60),
        Some(b'h') => (&trimmed[..trimmed.len() - 1], 60 * 60),
        Some(b'd') => (&trimmed[..trimmed.len() - 1], 24 * 60 * 60),
        _ => (trimmed.as_str(), 60),
    };
    let value = digits.parse::<u64>().ok()?;
    if value == 0 {
        return None;
    }
    Some(value * multiplier)
}

async fn run_ls_watch_plan(options: &LsPlanOptions) -> CliOutput {
    if !std::io::IsTerminal::is_terminal(&std::io::stdout()) {
        return ls_usage_error("maw ls: --watch requires a TTY stdout");
    }

    let live_options = ls_watch_live_options(options);
    let mut stdout = std::io::stdout();
    ls_watch_loop_with_hooks(
        &live_options,
        &mut stdout,
        render_ls_plan,
        ls_watch_current_hms,
        ls_watch_wait_next,
    )
    .await
}

fn ls_watch_live_options(options: &LsPlanOptions) -> LsPlanOptions {
    let mut live_options = options.clone();
    live_options.panes.clear();
    live_options.now = None;
    live_options.session_created.clear();
    live_options
}

async fn ls_watch_loop_with_hooks<W, Collect, Clock, Wait>(
    options: &LsPlanOptions,
    writer: &mut W,
    mut collect: Collect,
    mut clock: Clock,
    mut wait_next: Wait,
) -> CliOutput
where
    W: std::io::Write,
    Collect: FnMut(&LsPlanOptions) -> CliOutput,
    Clock: FnMut() -> String,
    Wait: FnMut(u64) -> Pin<Box<dyn Future<Output = bool> + Send>>,
{
    let interval = options.watch_interval_sec.unwrap_or(LS_WATCH_DEFAULT_SECS);
    if let Err(message) = ls_watch_enter(writer) {
        return ls_watch_error(&message);
    }

    let mut result = CliOutput {
        code: 0,
        stdout: String::new(),
        stderr: String::new(),
    };
    loop {
        let output = collect(options);
        let body = if output.code == 0 {
            output.stdout.as_str()
        } else {
            output.stderr.as_str()
        };
        if let Err(message) = ls_watch_write_frame(writer, body, interval, &clock()) {
            result = ls_watch_error(&message);
            break;
        }
        if output.code != 0 {
            result = output;
            result.stdout.clear();
            break;
        }
        if !wait_next(interval).await {
            break;
        }
    }

    if let Err(message) = ls_watch_restore(writer) {
        if result.code == 0 {
            return ls_watch_error(&message);
        }
        if !result.stderr.is_empty() {
            result.stderr.push('\n');
        }
        result.stderr.push_str(&message);
        result.stderr.push('\n');
    }
    result
}

fn ls_watch_wait_next(seconds: u64) -> Pin<Box<dyn Future<Output = bool> + Send>> {
    Box::pin(async move {
        tokio::select! {
            () = tokio::time::sleep(std::time::Duration::from_secs(seconds)) => true,
            _ = tokio::signal::ctrl_c() => false,
        }
    })
}

fn ls_watch_current_hms() -> String {
    ls_watch_current_hms_with_date(ls_watch_run_date_command)
}

fn ls_watch_current_hms_with_date(mut run_date: impl FnMut(&str) -> Option<Vec<u8>>) -> String {
    ["/bin/date", "date"]
        .into_iter()
        .find_map(|program| run_date(program).and_then(|output| ls_watch_hms_from_date_output(&output)))
        .unwrap_or_else(ls_watch_current_utc_hms)
}

fn ls_watch_run_date_command(program: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new(program)
        .arg("+%H:%M:%S")
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout)
}

fn ls_watch_hms_from_date_output(output: &[u8]) -> Option<String> {
    let hms = std::str::from_utf8(output).ok()?;
    let hms = hms.trim();
    ls_watch_is_hms(hms).then(|| hms.to_owned())
}

fn ls_watch_is_hms(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 8
        || bytes[2] != b':'
        || bytes[5] != b':'
        || !bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 2 | 5) || byte.is_ascii_digit())
    {
        return false;
    }
    let part = |start: usize| (bytes[start] - b'0') * 10 + (bytes[start + 1] - b'0');
    part(0) < 24 && part(3) < 60 && part(6) < 60
}

fn ls_watch_current_utc_hms() -> String {
    let seconds = current_epoch_seconds() % 86_400;
    ls_watch_format_hms(seconds)
}

fn ls_watch_format_hms(seconds: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        (seconds / 60) % 60,
        seconds % 60
    )
}

fn ls_watch_enter(writer: &mut impl std::io::Write) -> Result<(), String> {
    ls_watch_write_raw(writer, LS_WATCH_ENTER)
}

fn ls_watch_restore(writer: &mut impl std::io::Write) -> Result<(), String> {
    ls_watch_write_raw(writer, LS_WATCH_RESTORE)
}

fn ls_watch_write_frame(
    writer: &mut impl std::io::Write,
    body: &str,
    interval: u64,
    hms: &str,
) -> Result<(), String> {
    ls_watch_write_raw(writer, LS_WATCH_REPAINT)?;
    std::io::Write::write_all(writer, body.as_bytes())
        .map_err(|error| format!("maw ls: --watch terminal write failed: {error}"))?;
    if !body.ends_with('\n') {
        std::io::Write::write_all(writer, b"\n")
            .map_err(|error| format!("maw ls: --watch terminal write failed: {error}"))?;
    }
    let footer = format!("\x1b[2mwatching · {interval}s · ctrl-c to exit · {hms}\x1b[0m\n");
    ls_watch_write_raw(writer, &footer)
}

fn ls_watch_write_raw(writer: &mut impl std::io::Write, value: &str) -> Result<(), String> {
    std::io::Write::write_all(writer, value.as_bytes())
        .and_then(|()| std::io::Write::flush(writer))
        .map_err(|error| format!("maw ls: --watch terminal write failed: {error}"))
}

fn ls_watch_error(message: &str) -> CliOutput {
    CliOutput {
        code: 1,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}

fn render_ls_plan(options: &LsPlanOptions) -> CliOutput {
    let mut live_options;
    let effective_options = if options.panes.is_empty() {
        let mut client = TmuxClient::local();
        let live_panes = client.list_panes();
        live_options = options.clone();
        live_options.panes = live_panes;
        if live_options.now.is_none() {
            live_options.now = Some(current_epoch_seconds());
        }
        &live_options
    } else {
        options
    };
    let panes = project_ls_panes(effective_options);
    if options.federation {
        return ls_render_federation(options, &panes);
    }
    let (verify_out, fix_out) = ls_render_verify_fix(options);
    CliOutput {
        code: 0,
        stdout: if options.json {
            render_ls_json(options, &panes)
        } else {
            let mut out = render_ls_text(options, &panes);
            out.push_str(&verify_out);
            out.push_str(&fix_out);
            out
        },
        stderr: String::new(),
    }
}

fn current_epoch_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn project_ls_panes(options: &LsPlanOptions) -> Vec<LsPanePlan> {
    let now = options.now.unwrap_or_else(|| {
        options
            .panes
            .iter()
            .filter_map(|pane| pane.last_activity)
            .max()
            .unwrap_or(0)
            .saturating_add(600)
    });
    let mut panes = options
        .panes
        .iter()
        .filter_map(|pane| {
            let session = pane
                .target
                .split_once(':')
                .map_or(&pane.target[..], |(session, _)| session);
            if !options.channels && is_ls_channel_session(session) {
                return None;
            }
            if options.fleet_only && !is_default_ls_oracle_session(session) {
                return None;
            }
            if !options.teams && is_ls_team_session(session) {
                return None;
            }
            let source = None;
            let values = [
                session,
                pane.target.as_str(),
                pane.title.as_str(),
                pane.command.as_str(),
            ];
            if let Some(filter) = &options.filter {
                let filter = filter.to_lowercase();
                if !values
                    .iter()
                    .any(|value| value.to_lowercase().contains(&filter))
                {
                    return None;
                }
            }
            let age_sec = pane.last_activity.map(|last| now.saturating_sub(last));
            if options.active
                && pane
                    .last_activity
                    .is_none_or(|_| age_sec.is_none_or(|age| age > options.active_threshold_sec.unwrap_or(30 * 60)))
            {
                return None;
            }
            Some(LsPanePlan {
                id: pane.id.clone(),
                target: pane.target.clone(),
                session: session.to_owned(),
                command: pane.command.clone(),
                title: pane.title.clone(),
                source,
                last_activity: pane.last_activity,
                session_created: options.session_created.get(session).copied(),
                status: ls_pane_status(age_sec),
                age_sec,
                agent: is_ls_agent_command(&pane.command),
            })
        })
        .collect::<Vec<_>>();

    if options.recent {
        panes.sort_by(|left, right| {
            right
                .session_created
                .unwrap_or(0)
                .cmp(&left.session_created.unwrap_or(0))
                .then_with(|| left.target.cmp(&right.target))
        });
        if let Some(limit) = options.recent_limit {
            let mut seen = BTreeSet::new();
            panes.retain(|pane| {
                seen.insert(pane.session.clone());
                seen.len() <= limit
            });
        }
    } else {
        panes.sort_by(|left, right| left.target.cmp(&right.target));
    }
    panes
}

fn is_default_ls_oracle_session(session: &str) -> bool {
    session.split_once('-').is_some_and(|(prefix, suffix)| {
        prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.starts_with('-')
    }) || session.ends_with("-oracle")
}

fn is_ls_channel_session(session: &str) -> bool {
    session.ends_with("-discord") && !session.contains("discord-admin")
}

fn is_ls_team_session(session: &str) -> bool {
    session.starts_with("team-") || session.contains(":team-") || session.contains("-team-")
}

fn is_ls_agent_command(command: &str) -> bool {
    let command = command.to_lowercase();
    command.contains("claude") || command.contains("codex") || command.contains("node")
}

fn ls_pane_status(age_sec: Option<u64>) -> &'static str {
    match age_sec {
        Some(age) if age < 30 => "active",
        Some(age) if age < 300 => "idle",
        Some(_) => "stale",
        None => "unknown",
    }
}

fn render_ls_json(options: &LsPlanOptions, panes: &[LsPanePlan]) -> String {
    let mut fields = vec![
        "\"command\":\"ls\"".to_owned(),
        format!(
            "\"mode\":\"{}\"",
            if options.mode == LsMode::Verbose {
                "verbose"
            } else {
                "compact"
            }
        ),
        "\"scope\":\"local\"".to_owned(),
        "\"json\":true".to_owned(),
    ];
    if let Some(node) = &options.node {
        fields.push(format!("\"node\":{}", json_string(node)));
    }
    if options.fleet_only {
        fields.push("\"fleetOnly\":true".to_owned());
    }
    if !options.teams {
        fields.push("\"teams\":false".to_owned());
    }
    if options.verify {
        fields.push("\"verify\":true".to_owned());
    }
    if options.fix {
        fields.push("\"fix\":true".to_owned());
    }
    if options.active {
        fields.push(format!(
            "\"activeThresholdSec\":{}",
            options.active_threshold_sec.unwrap_or(30 * 60)
        ));
    }
    if let Some(limit) = options.recent_limit {
        fields.push(format!("\"recentLimit\":{limit}"));
    }
    if options.mode == LsMode::Verbose {
        let annotations = ls_annotation_context();
        fields.push(format!(
            "\"panes\":{}",
            render_ls_panes_json(panes, &annotations)
        ));
    } else {
        fields.push(format!(
            "\"sessions\":{}",
            render_ls_sessions_json(panes, options.recent)
        ));
    }
    format!("{{{}}}\n", fields.join(","))
}

fn render_ls_panes_json(panes: &[LsPanePlan], annotations: &LsAnnotationContext) -> String {
    let rows = panes
        .iter()
        .map(|pane| {
            let annotation = ls_pane_annotation(pane, annotations);
            format!(
                "{{\"id\":{},\"target\":{},\"session\":{},\"command\":{},\"title\":{},\"status\":{},\"ageSec\":{},\"agent\":{},\"annotation\":{}}}",
                json_string(&pane.id),
                json_string(&pane.target),
                json_string(&pane.session),
                json_string(&pane.command),
                json_string(&pane.title),
                json_string(pane.status),
                json_opt_u64(pane.age_sec),
                pane.agent,
                json_string(&annotation)
            )
        })
        .collect::<Vec<_>>();
    format!("[{}]", rows.join(","))
}

fn render_ls_sessions_json(panes: &[LsPanePlan], include_recent: bool) -> String {
    let mut by_session: BTreeMap<String, Vec<&LsPanePlan>> = BTreeMap::new();
    for pane in panes {
        by_session
            .entry(pane.session.clone())
            .or_default()
            .push(pane);
    }
    let mut rows = by_session.into_iter().collect::<Vec<_>>();
    if include_recent {
        rows.sort_by(|left, right| {
            right
                .1
                .first()
                .and_then(|pane| pane.session_created)
                .unwrap_or(0)
                .cmp(
                    &left
                        .1
                        .first()
                        .and_then(|pane| pane.session_created)
                        .unwrap_or(0),
                )
                .then_with(|| left.0.cmp(&right.0))
        });
    }
    let rows = rows
        .into_iter()
        .map(|(session, panes)| {
            let status = ls_best_status(&panes);
            let agents = panes.iter().filter(|pane| pane.agent).count();
            let mut fields = vec![
                format!("\"session\":{}", json_string(&session)),
                format!("\"status\":{}", json_string(status)),
                format!("\"panes\":{}", panes.len()),
                format!("\"agents\":{agents}"),
            ];
            push_json_opt(&mut fields, "oracle", ls_oracle_window_name(&panes));
            if let Some(created) = panes.first().and_then(|pane| pane.session_created) {
                fields.push(format!("\"created\":{created}"));
            }
            let youngest_active_age = panes
                .iter()
                .filter_map(|pane| pane.age_sec)
                .min();
            if let (Some(age), Some(_created)) = (
                youngest_active_age,
                panes.first().and_then(|pane| pane.session_created),
            ) {
                fields.push(format!("\"lastActivityAgeSec\":{age}"));
            }
            format!("{{{}}}", fields.join(","))
        })
        .collect::<Vec<_>>();
    format!("[{}]", rows.join(","))
}

fn ls_best_status(panes: &[&LsPanePlan]) -> &'static str {
    if panes.iter().any(|pane| pane.status == "active") {
        "active"
    } else if panes.iter().any(|pane| pane.status == "idle") {
        "idle"
    } else if panes.iter().any(|pane| pane.status == "stale") {
        "stale"
    } else {
        "unknown"
    }
}

fn render_ls_text(options: &LsPlanOptions, panes: &[LsPanePlan]) -> String {
    if panes.is_empty() {
        return if options.active {
            format!(
                "No sessions active in the last {}.\n",
                format_ls_duration(options.active_threshold_sec.unwrap_or(30 * 60))
            )
        } else {
            "No active sessions.\n  → maw bud <name>     create new oracle\n  → maw wake <name>    attach existing\n".to_owned()
        };
    }
    if options.mode == LsMode::Verbose {
        render_ls_verbose_text(panes)
    } else {
        render_ls_compact_text(panes)
    }
}

fn render_ls_verbose_text(panes: &[LsPanePlan]) -> String {
    let mut out = String::new();
    let annotations = ls_annotation_context();
    let target_width = panes
        .iter()
        .map(|pane| ls_visible_len(ls_pane_target(&pane.target)))
        .max()
        .unwrap_or(0)
        .max(28);
    let header = format!(
        "    {} {} {} {} TITLE",
        ls_pad("TARGET", target_width),
        ls_pad("CMD", 10),
        ls_pad("AGE", 6),
        ls_pad("ANNOTATION", 30)
    );
    let _ = writeln!(out, "{}", ls_color("36;1", &header));
    let groups = group_ls_sessions(panes);
    let session_width = ls_group_session_width(&groups);
    for (session, panes) in groups {
        render_ls_verbose_group(&mut out, &session, &panes, session_width, target_width, &annotations);
    }
    out
}

fn ls_group_session_width(groups: &[(String, Vec<&LsPanePlan>)]) -> usize {
    groups
        .iter()
        .map(|(session, _panes)| ls_visible_len(session))
        .max()
        .unwrap_or(0)
}

fn render_ls_verbose_group(
    out: &mut String,
    session: &str,
    panes: &[&LsPanePlan],
    session_width: usize,
    target_width: usize,
    annotations: &LsAnnotationContext,
) {
    let status = ls_best_status(panes);
    let dot = ls_status_dot(status);
    let oracle = ls_oracle_window_name(panes);
    let oracle_label = oracle
        .map(|window| format!(" · {}", ls_color("2", window)))
        .unwrap_or_default();
    let session = ls_color("36", &ls_pad(session, session_width));
    let _ = writeln!(out, "{dot} {session}{oracle_label}");

    let ordered = ls_oracle_first_panes(panes, oracle);
    for pane in ordered {
        render_ls_verbose_pane(out, pane, target_width, annotations);
    }
}

fn render_ls_verbose_pane(
    out: &mut String,
    pane: &LsPanePlan,
    target_width: usize,
    annotations: &LsAnnotationContext,
) {
    let dot = ls_status_dot(pane.status);
    let target = ls_pad(ls_pane_target(&pane.target), target_width);
    let command = ls_pad(&pane.command, 10);
    let age = ls_pad(&format_ls_age(pane.age_sec), 6);
    let annotation = ls_pane_annotation(pane, annotations);
    let annotation = ls_render_annotation(&annotation);
    let title = if pane.title.is_empty() {
        String::new()
    } else {
        ls_color("2", &ls_truncate(&pane.title, 50))
    };
    let _ = writeln!(
        out,
        "  {dot} {} {} {} {} {}",
        ls_color("36", &target),
        ls_color("2", &command),
        ls_color("2", &age),
        annotation,
        title
    );
}

fn render_ls_compact_text(panes: &[LsPanePlan]) -> String {
    let mut out = String::new();
    for (session, panes) in group_ls_sessions(panes) {
        let agents = panes.iter().filter(|pane| pane.agent).count();
        let status = ls_best_status(&panes);
        let dot = ls_status_dot(status);
        let oracle = ls_oracle_window_name(&panes)
            .map(|window| format!(" · {}", ls_color("2", window)))
            .unwrap_or_default();
        let session = ls_color("36", &session);
        let pane_count = ls_color(
            "2",
            &format!(
                "{} pane{}",
                panes.len(),
                if panes.len() == 1 { "" } else { "s" }
            ),
        );
        let agent_count = if agents > 0 {
            format!(
                "  {}",
                ls_color(
                    "94",
                    &format!("{agents} agent{}", if agents == 1 { "" } else { "s" })
                )
            )
        } else {
            String::new()
        };
        let _ = writeln!(out, "  {dot} {session}{oracle}  {pane_count}{agent_count}");
    }
    let _ = writeln!(out, "\n  {}", ls_color("2", "→ maw ls -v    full detail"));
    out
}

fn ls_oracle_window_name<'a>(panes: &[&'a LsPanePlan]) -> Option<&'a str> {
    panes
        .iter()
        .map(|pane| ls_window_name(&pane.target))
        .find(|window| window.ends_with("-oracle"))
}

#[derive(Debug, Default)]
struct LsAnnotationContext {
    fleet_sessions: BTreeSet<String>,
    team_by_pane: BTreeMap<String, String>,
}

fn ls_annotation_context() -> LsAnnotationContext {
    LsAnnotationContext {
        fleet_sessions: ls_fleet_sessions_for_annotation(),
        team_by_pane: ls_team_by_pane_for_annotation(),
    }
}

fn ls_fleet_sessions_for_annotation() -> BTreeSet<String> {
    let mut sessions = BTreeSet::new();
    for entry in fleet_load_entries() {
        let stem = entry.file.strip_suffix(".json").unwrap_or(&entry.file);
        if !stem.is_empty() {
            sessions.insert(stem.to_owned());
        }
        if !entry.session.name.is_empty() {
            sessions.insert(entry.session.name);
        }
    }
    sessions
}

fn ls_team_by_pane_for_annotation() -> BTreeMap<String, String> {
    let mut teams = BTreeMap::new();
    let root = team_home_dir().join(".claude").join("teams");
    for (team, config) in team_read_tool_teams(&root) {
        for member in config.members {
            let Some(pane_id) = member
                .tmux_pane_id
                .filter(|pane| !pane.is_empty() && pane != "in-process")
            else {
                continue;
            };
            teams.insert(pane_id, format!("{} @ {team}", member.name));
        }
    }
    teams
}

fn ls_pane_annotation(pane: &LsPanePlan, annotations: &LsAnnotationContext) -> String {
    let pane_ref = maw_tmux::TmuxLsPaneRef {
        id: pane.id.clone(),
        target: pane.target.clone(),
        command: Some(pane.command.clone()),
    };
    let annotation = maw_tmux::annotate_pane(
        &pane_ref,
        &annotations.fleet_sessions,
        &annotations.team_by_pane,
    );
    if annotation.is_empty() && ls_is_orphan_list_session(&pane.session, &annotations.fleet_sessions)
    {
        "orphan".to_owned()
    } else {
        annotation
    }
}

fn ls_is_fleet_list_session(session: &str, fleet_sessions: &BTreeSet<String>) -> bool {
    if session.split_once('-').is_some_and(|(prefix, suffix)| {
        prefix.chars().all(|ch| ch.is_ascii_digit()) && !suffix.starts_with('-')
    }) {
        return true;
    }
    fleet_sessions.contains(session)
}

fn ls_is_orphan_list_session(session: &str, fleet_sessions: &BTreeSet<String>) -> bool {
    if ls_is_fleet_list_session(session, fleet_sessions) {
        return false;
    }
    !(session == "maw-view" || session.ends_with("-view"))
}

fn ls_annotation_display(annotation: &str) -> &str {
    if annotation == "orphan" {
        "[orphan]"
    } else {
        annotation
    }
}

fn ls_render_annotation(annotation: &str) -> String {
    let display = ls_annotation_display(annotation);
    if display.is_empty() {
        return " ".repeat(30);
    }
    let padding = " ".repeat(30usize.saturating_sub(ls_visible_len(display)));
    let colored = if annotation.starts_with("team:") {
        ls_color("36", display)
    } else if annotation.starts_with("fleet:") {
        ls_color("32", display)
    } else if annotation.starts_with("view:") {
        ls_color("2", display)
    } else if annotation == "orphan" {
        ls_color("33", display)
    } else {
        display.to_owned()
    };
    format!("{colored}{padding}")
}

fn ls_oracle_first_panes<'a>(
    panes: &[&'a LsPanePlan],
    oracle: Option<&str>,
) -> Vec<&'a LsPanePlan> {
    let Some(oracle) = oracle else {
        return panes.to_vec();
    };
    panes
        .iter()
        .copied()
        .filter(|pane| ls_window_name(&pane.target) == oracle)
        .chain(
            panes
                .iter()
                .copied()
                .filter(|pane| ls_window_name(&pane.target) != oracle),
        )
        .collect()
}

fn ls_window_name(target: &str) -> &str {
    let window = target
        .split_once(':')
        .map_or(target, |(_, window_and_pane)| window_and_pane);
    window
        .rsplit_once('.')
        .map_or(window, |(window, _pane)| window)
}

fn ls_pane_target(target: &str) -> &str {
    target
        .split_once(':')
        .map_or(target, |(_, window_and_pane)| window_and_pane)
}

fn ls_visible_len(value: &str) -> usize {
    value.chars().count()
}

fn ls_pad(value: &str, width: usize) -> String {
    let mut out = ls_truncate(value, width);
    let len = ls_visible_len(&out);
    if len < width {
        out.push_str(&" ".repeat(width - len));
    }
    out
}

fn ls_status_dot(status: &str) -> String {
    let (code, glyph) = ls_status_dot_parts(status);
    ls_color(code, glyph)
}

fn ls_status_dot_parts(status: &str) -> (&'static str, &'static str) {
    match status {
        "frozen" => ("33", "⚠"),
        "active" => ("32", "●"),
        "idle" => ("33", "◐"),
        "stale" => ("31", "◌"),
        _ => ("90", "·"),
    }
}

fn ls_color(code: &str, value: &str) -> String {
    if std::env::var_os("NO_COLOR").is_some() {
        value.to_owned()
    } else {
        format!("\x1b[{code}m{value}\x1b[0m")
    }
}

fn group_ls_sessions(panes: &[LsPanePlan]) -> Vec<(String, Vec<&LsPanePlan>)> {
    let mut by_session: BTreeMap<String, Vec<&LsPanePlan>> = BTreeMap::new();
    for pane in panes {
        by_session
            .entry(pane.session.clone())
            .or_default()
            .push(pane);
    }
    by_session.into_iter().collect()
}

fn format_ls_duration(sec: u64) -> String {
    if sec < 60 {
        format!("{sec}s")
    } else if sec < 3600 {
        format!("{}m", sec / 60)
    } else if sec < 86_400 {
        format!("{}h", sec / 3600)
    } else {
        format!("{}d", sec / 86_400)
    }
}

fn format_ls_age(age_sec: Option<u64>) -> String {
    let Some(sec) = age_sec else {
        return String::new();
    };
    if sec == 0 {
        String::new()
    } else if sec < 60 {
        format!("{sec}s")
    } else if sec < 3600 {
        format!("{}m", sec / 60)
    } else if sec < 86_400 {
        format!("{}h{}m", sec / 3600, (sec % 3600) / 60)
    } else {
        let days = sec / 86_400;
        let hours = (sec % 86_400) / 3600;
        if hours == 0 {
            format!("{days}d")
        } else {
            format!("{days}d{hours}h")
        }
    }
}

fn ls_truncate(value: &str, width: usize) -> String {
    value.chars().take(width).collect::<String>()
}

fn ls_help_ok() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: [
            "maw ls — list live sessions (local or cross-node)",
            "",
            "Usage:",
            "  maw ls                  list live local sessions (default)",
            "  maw ls <filter>         filter local sessions",
            "  maw ls --federation     list local + peer sessions",
            "  maw ls --federation <peer>  drill into one peer",
            "  maw ls --federation --node <node>  filter the federated view",
            "  maw ls --json           emit JSON",
            "  maw ls --watch[=secs]   repaint live local sessions in a TTY (default 2s; not for pipes)",
            "  maw ls --active [30m]   local sessions touched within a recent threshold",
            "  maw ls --fleet-only     hide orphan/ad hoc tmux sessions (legacy filter)",
            "  maw ls --no-teams       hide L2 Claude Code teams from ~/.claude/teams",
            "  maw ls --verify         include worktree-bind diagnostics",
            "  maw ls --fix            prune orphaned worktrees (local only)",
            "",
            "Federation peer aliases are resolved from the maw state peers store (see: maw peers list).",
            "For registered fleet config, use maw fleet ls.",
        ]
        .join("\n")
            + "\n",
        stderr: String::new(),
    }
}

fn ls_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs ls [<filter>] [--all] [--json|--plan-json] [--compact|-c] [--verbose|-v] [--watch[=secs]] [--recent|-r [N]] [--active [30m|1h]] [--federation] [--fleet-only] [--node <node>] [--verify] [--fix] [--channels] [--pane <id|command|target|title|pid|cwd|last_activity>]...\n"
        ),
    }
}

fn run_bring_plan(argv: &[String]) -> CliOutput {
    let plan_json = argv.iter().any(|arg| arg == "--plan-json");
    let filtered: Vec<String> = argv
        .iter()
        .filter(|arg| arg.as_str() != "--plan-json")
        .cloned()
        .collect();
    match parse_bring_args(&filtered) {
        Ok(parsed) => CliOutput {
            code: 0,
            stdout: if plan_json {
                render_bring_plan_json(&parsed)
            } else {
                render_bring_plan_text(&parsed)
            },
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("{}\n{}\n", error.message, error.usage.join("\n")),
        },
    }
}

fn render_bring_plan_text(parsed: &ParsedBringArgs) -> String {
    let mut lines = vec![format!("wake {} --split", parsed.oracle)];
    if let Some(engine) = &parsed.opts.engine {
        lines.push(format!("engine: {engine}"));
    }
    if let Some(session) = &parsed.opts.session {
        lines.push(format!("session: {session}"));
    }
    if let Some(split_target) = &parsed.opts.split_target {
        lines.push(format!("split-target: {split_target}"));
    }
    if parsed.opts.pick {
        lines.push("pick: true".to_owned());
    }
    lines.join("\n") + "\n"
}

fn render_bring_plan_json(parsed: &ParsedBringArgs) -> String {
    let opts = &parsed.opts;
    let mut fields = vec![
        format!("\"oracle\":{}", json_string(&parsed.oracle)),
        format!("\"split\":{}", opts.split),
    ];
    push_json_opt(&mut fields, "engine", opts.engine.as_deref());
    if opts.pick {
        fields.push("\"pick\":true".to_owned());
    }
    push_json_opt(&mut fields, "session", opts.session.as_deref());
    push_json_opt(&mut fields, "splitTarget", opts.split_target.as_deref());
    format!(
        "{{\"command\":\"bring\",\"opts\":{{{}}}}}\n",
        fields.join(",")
    )
}

fn push_json_opt(fields: &mut Vec<String>, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        fields.push(format!("{}:{}", json_string(key), json_string(value)));
    }
}

fn json_opt_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "null".to_owned(), |value| value.to_string())
}

fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
    out
}



#[cfg(test)]
mod remaining_cli_private_coverage_tests {
    use super::*;

    fn ls_test_options() -> LsPlanOptions {
        LsPlanOptions {
            json: false,
            mode: LsMode::Compact,
            all: true,
            channels: false,
            active: false,
            active_threshold_sec: None,
            recent: false,
            recent_limit: None,
            filter: None,
            peer: None,
            federation: false,
            node: None,
            fleet_only: false,
            teams: true,
            verify: false,
            fix: false,
            watch_interval_sec: None,
            now: None,
            panes: Vec::new(),
            session_created: std::collections::BTreeMap::new(),
        }
    }

    fn ls_test_pane(
        id: &str,
        target: &str,
        session: &str,
        command: &str,
        agent: bool,
    ) -> LsPanePlan {
        LsPanePlan {
            id: id.to_owned(),
            target: target.to_owned(),
            session: session.to_owned(),
            command: command.to_owned(),
            title: String::new(),
            source: None,
            last_activity: None,
            session_created: None,
            status: "active",
            age_sec: Some(0),
            agent,
        }
    }

    #[test]
    fn private_pair_code_store_consumed_state_is_renderable() {
        let result = PairCodeStorePlanResult::Lookup(LookupResult::Consumed);
        assert_eq!(pair_code_store_result_state(&result), "consumed");
        assert_eq!(pair_code_store_result_entry(&result), "null");
    }

    #[test]
    fn private_route_error_without_hint_is_renderable() {
        let result = RouteResult::Error {
            reason: "missing".to_owned(),
            detail: "no route".to_owned(),
            hint: None,
        };
        assert_eq!(
            render_route_plan_text("neo", &result),
            "route neo: error missing no route\n"
        );
    }

    #[test]
    fn private_calver_and_ls_duration_error_edges_are_reachable() {
        assert_eq!(
            parse_i32_part(None, "hour"),
            Err("calver: missing hour in --now".to_owned())
        );
        assert_eq!(parse_ls_duration_seconds("2m"), Some(120));
        assert_eq!(parse_ls_watch_seconds("5"), Some(5));
        assert_eq!(parse_ls_watch_seconds("0"), None);
        assert_eq!(parse_ls_duration_seconds("7w"), None);
    }

    #[test]
    fn private_ls_watch_flags_parse_and_reject_json() {
        let default_watch = parse_ls_plan_options(&["--watch".to_owned()]).expect("default watch");
        assert_eq!(
            default_watch.watch_interval_sec,
            Some(LS_WATCH_DEFAULT_SECS)
        );

        let explicit_watch =
            parse_ls_plan_options(&["--watch=5".to_owned()]).expect("explicit watch");
        assert_eq!(explicit_watch.watch_interval_sec, Some(5));

        let json_watch =
            parse_ls_plan_options(&["--watch".to_owned(), "--json".to_owned()]).expect_err("json watch");
        assert_eq!(json_watch.code, 2);
        assert!(json_watch
            .stderr
            .contains("maw ls: --watch cannot be combined with --json"));
    }

    #[tokio::test]
    async fn private_ls_watch_loop_repaints_and_restores_with_injected_hooks() {
        let mut options = ls_test_options();
        options.watch_interval_sec = Some(5);
        let mut buffer = Vec::<u8>::new();
        let mut renders = 0usize;
        let mut sleeps = Vec::<u64>::new();

        let output = ls_watch_loop_with_hooks(
            &options,
            &mut buffer,
            |_| {
                renders += 1;
                CliOutput {
                    code: 0,
                    stdout: format!("frame {renders}\n"),
                    stderr: String::new(),
                }
            },
            || "01:02:03".to_owned(),
            |seconds| {
                sleeps.push(seconds);
                Box::pin(std::future::ready(sleeps.len() < 3))
            },
        )
        .await;

        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(renders, 3);
        assert_eq!(sleeps, vec![5, 5, 5]);
        let text = String::from_utf8(buffer).expect("utf8 watch output");
        assert!(text.starts_with(LS_WATCH_ENTER), "{text:?}");
        assert!(text.ends_with(LS_WATCH_RESTORE), "{text:?}");
        assert_eq!(text.matches(LS_WATCH_REPAINT).count(), 3);
        assert!(text.contains("frame 1\n"));
        assert!(text.contains("frame 3\n"));
        assert!(text.contains("watching · 5s · ctrl-c to exit · 01:02:03"));
    }

    #[test]
    fn private_ls_watch_footer_clock_uses_local_date_output() {
        let mut programs = Vec::<String>::new();
        let hms = ls_watch_current_hms_with_date(|program| {
            programs.push(program.to_owned());
            Some(b"11:30:17\n".to_vec())
        });

        assert_eq!(hms, "11:30:17");
        assert_eq!(programs, vec!["/bin/date"]);
        assert_eq!(
            ls_watch_hms_from_date_output(b"04:30:17\n"),
            Some("04:30:17".to_owned())
        );
        assert_eq!(ls_watch_hms_from_date_output(b"4:30:17\n"), None);
        assert_eq!(ls_watch_hms_from_date_output(b"24:00:00\n"), None);
    }

    #[test]
    fn private_ls_unknown_status_and_json_age_without_created_are_reachable() {
        let pane = LsPanePlan {
            id: "%1".to_owned(),
            target: "alpha:1.0".to_owned(),
            session: "alpha".to_owned(),
            command: "zsh".to_owned(),
            title: String::new(),
            source: None,
            last_activity: None,
            session_created: None,
            status: "mystery",
            age_sec: None,
            agent: false,
        };
        assert_eq!(ls_best_status(&[&pane]), "unknown");
        assert_eq!(ls_status_dot_parts("mystery"), ("90", "·"));
        let rendered = render_ls_sessions_json(&[pane], true);
        assert!(rendered.contains("\"status\":\"unknown\""));
        assert!(!rendered.contains("lastActivityAgeSec"));
    }

    #[test]
    fn private_ls_oracle_window_name_renders_compact_text_and_json() {
        assert_eq!(
            ls_window_name("188-maw-rs:maw-rs-oracle.0"),
            "maw-rs-oracle"
        );
        assert_eq!(ls_window_name("188-maw-rs:maw-rs-oracle"), "maw-rs-oracle");
        assert_eq!(ls_window_name("maw-rs-oracle.0"), "maw-rs-oracle");
        assert_eq!(ls_window_name("maw-rs-oracle"), "maw-rs-oracle");

        let options = ls_test_options();
        let panes = vec![
            ls_test_pane(
                "%1",
                "188-maw-rs:maw-rs-oracle.0",
                "188-maw-rs",
                "zsh",
                false,
            ),
            ls_test_pane(
                "%2",
                "188-maw-rs:maw-rs-codex-1.0",
                "188-maw-rs",
                "codex",
                true,
            ),
        ];

        let text = render_ls_text(&options, &panes);
        assert!(text.starts_with("  "));
        assert_eq!(text.matches("maw-rs-oracle").count(), 1);
        assert!(text.contains(" · "));
        assert!(text.contains("2 panes"));
        assert!(text.contains("1 agent"));

        let json = render_ls_sessions_json(&panes, false);
        assert_eq!(json.matches("maw-rs-oracle").count(), 1);
        assert!(json.contains("\"oracle\":\"maw-rs-oracle\""));
    }

    #[test]
    fn private_ls_compact_without_oracle_window_keeps_original_row_shape() {
        let options = ls_test_options();
        let pane = ls_test_pane("%3", "199-scratch:main.0", "199-scratch", "zsh", false);

        let text = render_ls_text(&options, std::slice::from_ref(&pane));
        let expected = format!(
            "  {} {}  {}\n\n  {}\n",
            ls_status_dot("active"),
            ls_color("36", "199-scratch"),
            ls_color("2", "1 pane"),
            ls_color("2", "→ maw ls -v    full detail")
        );
        assert_eq!(text, expected);

        let json = render_ls_sessions_json(&[pane], false);
        assert!(!json.contains("\"oracle\""));
    }

    #[test]
    fn private_ls_verbose_groups_sessions_with_oracle_pane_first_and_json_unchanged() {
        let mut options = ls_test_options();
        options.mode = LsMode::Verbose;

        let mut worker = ls_test_pane(
            "%2",
            "188-maw-rs:maw-rs-codex-1.0",
            "188-maw-rs",
            "codex",
            true,
        );
        worker.title = "worker".to_owned();
        worker.age_sec = Some(120);
        worker.status = "idle";
        let mut oracle = ls_test_pane(
            "%1",
            "188-maw-rs:maw-rs-oracle.0",
            "188-maw-rs",
            "2.1.198",
            false,
        );
        oracle.title = "main".to_owned();
        let mut other = ls_test_pane("%3", "200-other:main.0", "200-other", "zsh", false);
        other.title = "shell".to_owned();
        other.age_sec = Some(5);
        let panes = vec![worker, oracle, other];

        let text = render_ls_text(&options, &panes);
        assert_eq!(text.matches("TARGET").count(), 1);
        assert!(text.contains("ANNOTATION"));
        assert!(text.contains("188-maw-rs"));
        assert!(text.contains(" · "));
        assert!(text.contains("maw-rs-oracle"));
        assert!(text.contains("200-other"));
        assert!(text.lines().any(|line| {
            line.starts_with("  ") && line.contains("maw-rs-oracle.0") && line.contains("main")
        }));
        assert!(text.lines().any(|line| {
            line.contains("◐") && line.contains("maw-rs-codex-1.0") && line.contains("2m")
        }));
        assert!(
            text.find("maw-rs-oracle.0").expect("oracle member")
                < text.find("maw-rs-codex-1.0").expect("worker member")
        );

        assert_eq!(
            render_ls_sessions_json(&panes, false),
            "[{\"session\":\"188-maw-rs\",\"status\":\"active\",\"panes\":2,\"agents\":1,\"oracle\":\"maw-rs-oracle\"},{\"session\":\"200-other\",\"status\":\"active\",\"panes\":1,\"agents\":0}]"
        );
    }

    fn strip_ansi(input: &str) -> String {
        let mut out = String::new();
        let mut chars = input.chars().peekable();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' && chars.peek() == Some(&'[') {
                let _ = chars.next();
                for inner in chars.by_ref() {
                    if inner == 'm' {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn char_find(haystack: &str, needle: &str) -> Option<usize> {
        let byte_index = haystack.find(needle)?;
        Some(haystack[..byte_index].chars().count())
    }

    #[test]
    fn private_ls_verbose_uses_one_global_target_width_and_single_header() {
        let mut options = ls_test_options();
        options.mode = LsMode::Verbose;

        let first = ls_test_pane("%1", "880-short:main.0", "880-short", "zsh", false);
        let second = ls_test_pane(
            "%2",
            "881-long:a-very-very-long-window-name.0",
            "881-long",
            "bash",
            false,
        );
        let text = strip_ansi(&render_ls_text(&options, &[first, second]));

        assert_eq!(text.matches("TARGET").count(), 1);
        assert_eq!(text.matches("ANNOTATION").count(), 1);
        let header = text
            .lines()
            .find(|line| line.contains("TARGET"))
            .expect("header");
        let short = text
            .lines()
            .find(|line| line.contains("main.0"))
            .expect("short row");
        let long = text
            .lines()
            .find(|line| line.contains("a-very-very-long-window-name.0"))
            .expect("long row");
        assert_eq!(char_find(header, "TARGET"), char_find(short, "main.0"));
        assert_eq!(short.find("zsh"), long.find("bash"));
    }

    #[test]
    fn private_ls_status_dots_match_maw_js_glyphs_and_colors() {
        assert_eq!(ls_status_dot_parts("frozen"), ("33", "⚠"));
        assert_eq!(ls_status_dot_parts("active"), ("32", "●"));
        assert_eq!(ls_status_dot_parts("idle"), ("33", "◐"));
        assert_eq!(ls_status_dot_parts("stale"), ("31", "◌"));
        assert_eq!(ls_status_dot_parts("unknown"), ("90", "·"));
    }

    #[test]
    fn private_ls_verbose_member_rows_have_per_pane_dots_and_aligned_header() {
        let mut options = ls_test_options();
        options.mode = LsMode::Verbose;

        let mut active = ls_test_pane("%1", "900-mixed:main.0", "900-mixed", "zsh", false);
        active.status = "active";
        active.age_sec = Some(5);
        let mut idle = ls_test_pane("%2", "900-mixed:worker.0", "900-mixed", "bash", false);
        idle.status = "idle";
        idle.age_sec = Some(120);

        let text = strip_ansi(&render_ls_text(&options, &[active, idle]));
        let header = text
            .lines()
            .find(|line| line.contains("TARGET"))
            .expect("header");
        let active_row = text
            .lines()
            .find(|line| line.contains("main.0"))
            .expect("active row");
        let idle_row = text
            .lines()
            .find(|line| line.contains("worker.0"))
            .expect("idle row");

        assert!(active_row.starts_with("  ● "), "{active_row:?}");
        assert!(idle_row.starts_with("  ◐ "), "{idle_row:?}");
        assert_eq!(char_find(header, "TARGET"), char_find(active_row, "main.0"));
        assert_eq!(char_find(header, "TARGET"), char_find(idle_row, "worker.0"));
    }

    #[test]
    fn private_ls_verbose_group_headers_use_global_session_column() {
        let mut options = ls_test_options();
        options.mode = LsMode::Verbose;

        let crew = ls_test_pane(
            "%1",
            "183-crew-master:crew-master-oracle.0",
            "183-crew-master",
            "zsh",
            false,
        );
        let hermes = ls_test_pane(
            "%2",
            "168-hermes:hermes-oracle.0",
            "168-hermes",
            "zsh",
            false,
        );
        let world = ls_test_pane(
            "%3",
            "58-world-guardian:main.0",
            "58-world-guardian",
            "zsh",
            false,
        );

        let text = strip_ansi(&render_ls_text(&options, &[crew, hermes, world]));
        let crew_header = text
            .lines()
            .find(|line| line.contains("183-crew-master"))
            .expect("crew header");
        let hermes_header = text
            .lines()
            .find(|line| line.contains("168-hermes"))
            .expect("hermes header");
        let world_header = text
            .lines()
            .find(|line| line.contains("58-world-guardian"))
            .expect("world header");

        assert_eq!(char_find(crew_header, " · "), char_find(hermes_header, " · "));
        assert!(world_header.ends_with("58-world-guardian"));
    }

    #[test]
    fn private_ls_age_blank_unknown_combined_hours_and_title_cap() {
        let mut options = ls_test_options();
        options.mode = LsMode::Verbose;

        let mut unknown = ls_test_pane("%1", "901-age:unknown.0", "901-age", "zsh", false);
        unknown.status = "unknown";
        unknown.age_sec = None;
        let mut old = ls_test_pane("%2", "901-age:old.0", "901-age", "bash", false);
        old.status = "stale";
        old.age_sec = Some(3900);
        old.title = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789".to_owned();

        assert_eq!(format_ls_age(None), "");
        assert_eq!(format_ls_age(Some(0)), "");
        assert_eq!(format_ls_age(Some(3900)), "1h5m");

        let rendered_json = render_ls_panes_json(
            std::slice::from_ref(&unknown),
            &LsAnnotationContext::default(),
        );
        assert!(rendered_json.contains("\"ageSec\":null"));

        let text = strip_ansi(&render_ls_text(&options, &[unknown, old]));
        let unknown_row = text
            .lines()
            .find(|line| line.contains("unknown.0"))
            .expect("unknown row");
        let command_start = unknown_row.find("zsh").expect("command");
        assert_eq!(&unknown_row[command_start + 10..command_start + 18], "        ");
        assert!(unknown_row.starts_with("  · "), "{unknown_row:?}");

        let old_row = text
            .lines()
            .find(|line| line.contains("old.0"))
            .expect("old row");
        assert!(old_row.contains("1h5m"), "{old_row:?}");
        assert!(old_row.ends_with("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWX"));
        assert!(!old_row.contains("YZ0123456789"));
    }

    #[test]
    fn private_ls_annotation_port_handles_team_fleet_view_and_orphan() {
        let context = LsAnnotationContext {
            fleet_sessions: BTreeSet::from(["50-mawjs".to_owned()]),
            team_by_pane: BTreeMap::from([("%team".to_owned(), "builder @ alpha".to_owned())]),
        };

        let team = ls_test_pane("%team", "scratch:worker.0", "scratch", "codex", true);
        let fleet = ls_test_pane("%fleet", "50-mawjs:1.0", "50-mawjs", "zsh", false);
        let view = ls_test_pane("%view", "maw-view:main.0", "maw-view", "zsh", false);
        let orphan = ls_test_pane("%orphan", "scratch:main.0", "scratch", "zsh", false);
        let numeric_fleet_shape = ls_test_pane(
            "%numeric",
            "51-unregistered:main.0",
            "51-unregistered",
            "zsh",
            false,
        );

        assert_eq!(ls_pane_annotation(&team, &context), "team: builder @ alpha");
        assert_eq!(ls_pane_annotation(&fleet, &context), "fleet: mawjs");
        assert_eq!(ls_pane_annotation(&view, &context), "view: maw-view");
        assert_eq!(ls_pane_annotation(&orphan, &context), "orphan");
        assert_eq!(ls_pane_annotation(&numeric_fleet_shape, &context), "");
        assert!(ls_render_annotation("orphan").contains("[orphan]"));
    }
    include!("attach_private_tests.rs");

}
