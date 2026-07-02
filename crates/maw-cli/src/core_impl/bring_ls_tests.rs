fn parse_ls_pane(value: &str) -> Result<TmuxPane, String> {
    parse_discover_pane(value).map_err(|message| message.replacen("discover:", "ls:", 1))
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

fn render_ls_plan(options: &LsPlanOptions) -> CliOutput {
    if let Some(peer) = &options.peer {
        if !options.federation {
            return CliOutput {
                code: 0,
                stdout: if options.json {
                    format!(
                        "{{\"command\":\"ls\",\"scope\":\"peer\",\"peer\":{},\"sessions\":[]}}\n",
                        json_string(peer)
                    )
                } else {
                    format!("ls peer {peer}: no fake sessions\n")
                },
                stderr: String::new(),
            };
        }
    }

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
            if !options.all
                && options.mode == LsMode::Compact
                && !is_default_ls_oracle_session(session)
            {
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
            let age_sec = pane
                .last_activity
                .map_or(0, |last| now.saturating_sub(last));
            if options.active
                && pane
                    .last_activity
                    .is_none_or(|_| age_sec > options.active_threshold_sec.unwrap_or(30 * 60))
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

fn ls_pane_status(age_sec: u64) -> &'static str {
    if age_sec < 30 {
        "active"
    } else if age_sec < 300 {
        "idle"
    } else {
        "stale"
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
        fields.push(format!("\"panes\":{}", render_ls_panes_json(panes)));
    } else {
        fields.push(format!(
            "\"sessions\":{}",
            render_ls_sessions_json(panes, options.recent)
        ));
    }
    format!("{{{}}}\n", fields.join(","))
}

fn render_ls_panes_json(panes: &[LsPanePlan]) -> String {
    let rows = panes
        .iter()
        .map(|pane| {
            format!(
                "{{\"id\":{},\"target\":{},\"session\":{},\"command\":{},\"title\":{},\"status\":{},\"ageSec\":{},\"agent\":{}}}",
                json_string(&pane.id),
                json_string(&pane.target),
                json_string(&pane.session),
                json_string(&pane.command),
                json_string(&pane.title),
                json_string(pane.status),
                pane.age_sec,
                pane.agent
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
            if let Some(created) = panes.first().and_then(|pane| pane.session_created) {
                fields.push(format!("\"created\":{created}"));
            }
            let youngest_active_age = panes
                .iter()
                .filter_map(|pane| pane.last_activity.map(|_| pane.age_sec))
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
        let mut lines = vec!["TARGET CMD AGE TITLE".to_owned()];
        for pane in panes {
            lines.push(format!(
                "{} {} {} {}",
                ls_color("36", &pane.target),
                ls_color("2", &pane.command),
                ls_color("2", &format_ls_duration(pane.age_sec)),
                pane.title
            ));
        }
        lines.join("\n") + "\n"
    } else {
        let mut out = String::new();
        for (session, panes) in group_ls_sessions(panes) {
            let agents = panes.iter().filter(|pane| pane.agent).count();
            let status = ls_best_status(&panes);
            let dot = ls_status_dot(status);
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
            let _ = writeln!(out, "{dot} {session}  {pane_count}{agent_count}");
        }
        let _ = writeln!(out, "\n  {}", ls_color("2", "→ maw ls -v    full detail"));
        out
    }
}

fn ls_status_dot(status: &str) -> String {
    match status {
        "active" => ls_color("92", "●"),
        "idle" => ls_color("93", "◌"),
        "stale" => ls_color("31", "◌"),
        _ => ls_color("2", "◌"),
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

fn ls_help_ok() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: [
            "maw ls — list live sessions (local or cross-node)",
            "",
            "Usage:",
            "  maw ls                  list live local sessions (default)",
            "  maw ls <filter>         filter local sessions",
            "  maw ls <peer>           list sessions on a federation peer (legacy)",
            "  maw ls --federation     list local + peer sessions",
            "  maw ls --federation <peer>  drill into one peer",
            "  maw ls --federation --node <node>  filter the federated view",
            "  maw ls --json           emit JSON",
            "  maw ls --active [30m]   local sessions touched within a recent threshold",
            "  maw ls --fleet-only     hide orphan/ad hoc tmux sessions (legacy filter)",
            "  maw ls --no-teams       hide L2 Claude Code teams from ~/.claude/teams",
            "  maw ls --verify         include worktree-bind diagnostics",
            "  maw ls --fix            prune orphaned worktrees (local only)",
            "",
            "Peer aliases are resolved from the maw state peers store (see: maw peers list).",
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
            "{message}\nusage: maw-rs ls [<filter>] [--all] [--json|--plan-json] [--compact|-c] [--verbose|-v] [--recent|-r [N]] [--active [30m|1h]] [--federation] [--fleet-only] [--node <node>] [--verify] [--fix] [--channels] [--pane <id|command|target|title|pid|cwd|last_activity>]...\n"
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
        assert_eq!(parse_ls_duration_seconds("7w"), None);
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
            last_activity: Some(10),
            session_created: None,
            status: "mystery",
            age_sec: 5,
            agent: false,
        };
        assert_eq!(ls_best_status(&[&pane]), "unknown");
        assert!(ls_status_dot("mystery").contains('◌'));
        let rendered = render_ls_sessions_json(&[pane], true);
        assert!(rendered.contains("\"status\":\"unknown\""));
        assert!(!rendered.contains("lastActivityAgeSec"));
    }
    include!("attach_private_tests.rs");

}
