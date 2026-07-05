use super::*;

#[derive(Debug, Clone)]
pub(super) struct BotRow {
    pub(super) bot: String,
    pub(super) in_pass: bool,
    pub(super) in_registry: bool,
    pub(super) legacy_path: Option<PathBuf>,
    pub(super) hybrid_path: Option<PathBuf>,
    pub(super) tmux_line: Option<String>,
    pub(super) online: bool,
    pub(super) online_session: Option<String>,
    pub(super) online_bun_pid: Option<u32>,
    pub(super) anchor: Option<String>,
    pub(super) discord_ok: Option<bool>,
    pub(super) discord_status: Option<u16>,
    pub(super) discord_username: Option<String>,
}

pub(super) async fn status(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let check = args.iter().any(|a| a == "--check");
    let redact = args.iter().any(|a| a == "--redact");
    let json_flag = args.iter().any(|a| a == "--json");
    let filter = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(String::as_str);
    let mut rows = gather_rows(env);
    if let Some(filter) = filter {
        rows.retain(|r| r.bot == filter || r.bot == format!("{filter}-oracle"));
        if rows.is_empty() {
            log.push(format!(
                "✗ no bot matching '{filter}' in pass or state-dirs.ts"
            ));
            return true;
        }
    }
    if check {
        let tokens = list_pass_tokens(env);
        for row in &mut rows {
            if let Some(entry) = tokens.iter().find(|t| t.bot == row.bot) {
                if let Some(token) = decrypt_token(&entry.name) {
                    let (ok, status, username) = ping(rest, &token).await;
                    row.discord_ok = Some(ok);
                    row.discord_status = Some(status);
                    row.discord_username = username;
                } else {
                    row.discord_ok = Some(false);
                    row.discord_status = Some(0);
                }
            }
        }
    }
    if json_flag {
        let rows_json = rows.iter().map(row_json).collect::<Vec<_>>();
        log.push(
            serde_json::to_string_pretty(
                &json!({"host": short_host(env), "redacted": redact, "rows": rows_json}),
            )
            .unwrap_or_default(),
        );
    } else if filter.is_some() && rows.len() == 1 {
        emit_status_detail(env, &rows[0], redact, log);
    } else {
        emit_status_table(env, &rows, check, redact, log);
    }
    true
}

pub(super) fn gather_rows(env: &DiscordEnv) -> Vec<BotRow> {
    let tokens = list_pass_tokens(env);
    let registry = load_state_dirs_registry(env);
    let anchors = load_anchors(env);
    let all = tokens
        .iter()
        .map(|t| t.bot.clone())
        .chain(registry.iter().cloned())
        .collect::<BTreeSet<_>>();
    all.into_iter()
        .map(|bot| {
            let online = find_online_bun_for_bot(&bot);
            BotRow {
                in_pass: tokens.iter().any(|t| t.bot == bot),
                in_registry: registry.contains(&bot),
                legacy_path: find_legacy_state_dir(env, &bot),
                hybrid_path: find_hybrid_discord(env, &bot),
                tmux_line: find_tmux_session(&bot),
                online: online.is_some(),
                online_session: online.as_ref().and_then(|o| o.1.clone()),
                online_bun_pid: online.as_ref().map(|o| o.0),
                anchor: anchors.get(&bot).cloned(),
                discord_ok: None,
                discord_status: None,
                discord_username: None,
                bot,
            }
        })
        .collect()
}

pub(super) fn row_json(row: &BotRow) -> Value {
    json!({
        "bot": row.bot,
        "inPass": row.in_pass,
        "inRegistry": row.in_registry,
        "legacyPath": row.legacy_path.as_ref().map(|p| p.display().to_string()),
        "hybridPath": row.hybrid_path.as_ref().map(|p| p.display().to_string()),
        "tmuxLine": row.tmux_line,
        "online": row.online,
        "onlineSession": row.online_session,
        "onlineBunPid": row.online_bun_pid,
        "anchor": row.anchor,
        "discordOK": row.discord_ok,
        "discordStatus": row.discord_status,
        "discordUsername": row.discord_username,
    })
}

pub(super) fn short_host(env: &DiscordEnv) -> String {
    env.hostname
        .split('.')
        .next()
        .unwrap_or("unknown")
        .to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Severity {
    Ok,
    Warn,
    Info,
    Error,
}

pub(super) fn classify(row: &BotRow) -> (Severity, String) {
    if row.in_registry && !row.in_pass {
        return (
            Severity::Error,
            "registered but no token in pass".to_owned(),
        );
    }
    if row.in_pass && !row.in_registry {
        return (
            Severity::Error,
            "token in pass but not in state-dirs.ts".to_owned(),
        );
    }
    if row.discord_ok == Some(false) {
        return (
            Severity::Error,
            format!("Discord REST returned {}", row.discord_status.unwrap_or(0)),
        );
    }
    if row.tmux_line.is_some() && !row.online {
        return (
            Severity::Error,
            "tmux session exists but no Gateway bun — orphan (bind incomplete)".to_owned(),
        );
    }
    if row.in_pass && row.in_registry && row.hybrid_path.is_none() && row.online {
        return (
            Severity::Info,
            "online but using legacy state-dir — hybrid pattern not applied".to_owned(),
        );
    }
    if row.in_registry && !row.online {
        return (Severity::Warn, "offline on this host".to_owned());
    }
    (Severity::Ok, String::new())
}

pub(super) fn sev_name(sev: Severity) -> &'static str {
    match sev {
        Severity::Ok => "ok",
        Severity::Warn => "warn",
        Severity::Info => "info",
        Severity::Error => "error",
    }
}

pub(super) fn sev_icon(sev: Severity) -> &'static str {
    match sev {
        Severity::Ok => "✓",
        Severity::Warn => "○",
        Severity::Info => "·",
        Severity::Error => "✗",
    }
}
