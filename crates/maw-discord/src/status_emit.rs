use super::*;

pub(super) fn emit_status_table(
    env: &DiscordEnv,
    rows: &[BotRow],
    check: bool,
    redact: bool,
    log: &mut Vec<String>,
) {
    let host = short_host(env);
    log.push(format!(
        "🔍 maw discord status @ {host} — {} bot(s) | {}{}",
        rows.len(),
        if redact { "REDACTED · " } else { "" },
        if check {
            "with Discord REST"
        } else {
            "online/where via bun ancestry — use --check for REST"
        }
    ));
    log.push(String::new());
    let head = "  bot                          online  anchor              drift  where (tmux session)              severity";
    log.push(head.to_owned());
    log.push(format!("  {}", "─".repeat(head.len() - 2)));
    let mut counts = BTreeMap::from([
        (Severity::Ok, 0usize),
        (Severity::Warn, 0),
        (Severity::Info, 0),
        (Severity::Error, 0),
    ]);
    let mut drift_count = 0;
    for row in rows {
        let (sev, _) = classify(row);
        *counts.entry(sev).or_default() += 1;
        let is_here = row.anchor.as_ref().is_some_and(|a| {
            a == &host
                || a == &format!("nat@{host}")
                || a.ends_with(&format!("@{host}"))
                || a.ends_with(&format!("@{host}.wg"))
        });
        let drift = if row.online && row.anchor.is_some() && !is_here {
            drift_count += 1;
            "⚠ here"
        } else if row.online && row.anchor.is_some() {
            " ✓ ok "
        } else if !row.online && row.anchor.is_some() && is_here {
            "⚠ down"
        } else {
            "  ─  "
        };
        let where_text = row
            .online_session
            .clone()
            .or_else(|| row.tmux_line.as_ref().map(|_| "(orphan tmux)".to_owned()))
            .unwrap_or_else(|| "—".to_owned());
        log.push(format!(
            "  {:<28}{}  {:<18}  {drift}  {:<33} {} {}",
            row.bot,
            if row.online { "✓ ON  " } else { "✗ off " },
            row.anchor.clone().unwrap_or_else(|| "—".to_owned()),
            where_text,
            sev_icon(sev),
            sev_name(sev)
        ));
    }
    log.push(String::new());
    log.push(format!(
        "summary @ {host}: {} ok · {} warn · {} info · {} error",
        counts[&Severity::Ok],
        counts[&Severity::Warn],
        counts[&Severity::Info],
        counts[&Severity::Error]
    ));
    log.push(format!(
        "  online: {}/{}  ·  anchors: {}/{}  ·  drift: {drift_count}",
        rows.iter().filter(|r| r.online).count(),
        rows.len(),
        rows.iter().filter(|r| r.anchor.is_some()).count(),
        rows.len()
    ));
    log.push("  legend: ✓ ON = Gateway bun verified · anchor = canonical host (state-dirs.ts ANCHORS) · drift = bot online but not on anchor host".to_owned());
    if counts[&Severity::Error] > 0 || drift_count > 0 {
        log.push("run 'maw discord status <bot>' for details on any error/drift row".to_owned());
    }
}

pub(super) fn emit_status_detail(
    env: &DiscordEnv,
    row: &BotRow,
    redact: bool,
    log: &mut Vec<String>,
) {
    let (sev, reason) = classify(row);
    let host = short_host(env);
    log.push(format!(
        "🔍 {}  @ {host}    {} {}{}",
        row.bot,
        sev_icon(sev),
        sev_name(sev),
        if reason.is_empty() {
            String::new()
        } else {
            format!(" — {reason}")
        }
    ));
    log.push(String::new());
    if row.online {
        log.push(format!("  Gateway:           ✓ ONLINE on {host}"));
        log.push(format!(
            "                       bun pid:      {}",
            row.online_bun_pid.unwrap_or(0)
        ));
        log.push(format!(
            "                       tmux session: {}",
            row.online_session
                .clone()
                .unwrap_or_else(|| "(detached)".to_owned())
        ));
    } else if let Some(tmux) = &row.tmux_line {
        log.push(
            "  Gateway:           ✗ OFFLINE — tmux session present but no Gateway bun (orphan)"
                .to_owned(),
        );
        log.push(format!("                       orphan tmux:  {tmux}"));
    } else {
        log.push(format!("  Gateway:           ✗ OFFLINE on {host}"));
    }
    if row.in_pass {
        if let Some(t) = list_pass_tokens(env).into_iter().find(|t| t.bot == row.bot) {
            let when = if redact {
                "—".to_owned()
            } else {
                t.modified.map_or_else(|| "—".to_owned(), ymd_utc)
            };
            log.push(format!(
                "  Pass token:        ✓ discord/{} ({}, {when})",
                t.name,
                fmt_size(t.size_bytes)
            ));
        }
    } else {
        log.push(format!(
            "  Pass token:        ✗ missing — no discord/{}-token in pass",
            row.bot
        ));
    }
    log.push(format!(
        "  Legacy state-dir:  {}",
        row.legacy_path.as_ref().map_or_else(
            || format!("✗ not found at ~/.claude/channels/{}/", row.bot),
            |p| format!("✓ {}/", p.display())
        )
    ));
    log.push(format!(
        "  Hybrid .discord/:  {}",
        row.hybrid_path.as_ref().map_or_else(
            || "✗ not found".to_owned(),
            |p| format!("✓ {}/", p.display())
        )
    ));
    log.push(format!(
        "  Registry:          {}",
        if row.in_registry {
            "✓ in state-dirs.ts"
        } else {
            "✗ missing from state-dirs.ts"
        }
    ));
    log.push(format!(
        "  Anchor:            {}",
        row.anchor.clone().unwrap_or_else(|| "—".to_owned())
    ));
    if let Some(ok) = row.discord_ok {
        let username = row
            .discord_username
            .clone()
            .unwrap_or_else(|| "—".to_owned());
        log.push(format!(
            "  Discord REST:      {} {}  {username}",
            if ok { "✓" } else { "✗" },
            row.discord_status.unwrap_or(0)
        ));
    } else {
        log.push("  Discord REST:      (not checked — add --check)".to_owned());
    }
}
