use super::*;

pub(super) fn bind(env: &DiscordEnv, args: &[String], log: &mut Vec<String>) -> bool {
    let Some(bot) = args.first() else {
        log.push(
            "usage: maw discord bind <bot> [--apply] [--restart] [--session <name>] [--force]"
                .to_owned(),
        );
        log.push(String::new());
        log.push("  --apply      execute the plan (default: dry-run)".to_owned());
        log.push(
            "  --restart    if already online, telegraph + kill the existing session first"
                .to_owned(),
        );
        log.push("  --session    custom tmux session name (default: <bot>-discord)".to_owned());
        log.push(
            "  --force      override 'attached clients' check on --restart (yanks panes)"
                .to_owned(),
        );
        return true;
    };
    if rejects_option_arg(bot) {
        log.push("✗ invalid bot name: leading dash/-- separator rejected".to_owned());
        return true;
    }
    let apply = args.iter().any(|a| a == "--apply");
    let session = flag_value(args, "--session").unwrap_or_else(|| format!("{bot}-discord"));
    log.push(format!(
        "🪣 maw discord bind {bot}{}",
        if apply {
            " --apply"
        } else {
            " (dry-run — pass --apply to execute)"
        }
    ));
    log.push(String::new());
    let token = list_pass_tokens(env).into_iter().find(|t| t.bot == *bot);
    let state_dir = find_hybrid_discord(env, bot).or_else(|| find_legacy_state_dir(env, bot));
    let online = find_online_bun_for_bot(bot);
    log.push("  pre-flight:".to_owned());
    log.push(format!(
        "    {} pass token            {}",
        if token.is_some() { "✓" } else { "✗" },
        token.as_ref().map_or_else(
            || format!("missing discord/{bot}-token"),
            |t| format!("discord/{}", t.name)
        )
    ));
    log.push(format!(
        "    {} state-dir             {}",
        if state_dir.is_some() { "✓" } else { "✗" },
        state_dir.as_ref().map_or_else(
            || "missing hybrid .discord or legacy ~/.claude/channels".to_owned(),
            |p| p.display().to_string()
        )
    ));
    log.push(format!(
        "    {} not already online    {}",
        if online.is_none() { "✓" } else { "✗" },
        online.as_ref().map_or_else(
            || "ok".to_owned(),
            |o| format!(
                "already online pid {} tmux {}",
                o.0,
                o.1.clone().unwrap_or_else(|| "?".to_owned())
            )
        )
    ));
    log.push(String::new());
    if token.is_none() || state_dir.is_none() || online.is_some() {
        log.push("  ✗ pre-flight failed. fix the failing checks above and re-run.".to_owned());
        if online.is_some() {
            log.push("     to restart anyway, re-run with --restart (telegraphs + kills the existing session)".to_owned());
        }
        return true;
    }
    let cwd = find_ghq_path(env, bot).unwrap_or_else(|| env.ghq_root.join(bot));
    log.push("  plan:".to_owned());
    log.push(format!("    session: {session}"));
    log.push(format!("    cwd:     {}", cwd.display()));
    log.push(format!(
        "    state:   {}",
        state_dir.expect("checked").display()
    ));
    log.push("    command: claude --channels plugin:discord@claude-plugins-official".to_owned());
    log.push(String::new());
    if !apply {
        log.push("  ⓘ dry-run only — re-run with --apply to execute".to_owned());
        return true;
    }
    log.push(
        "  ✗ native maw-rs bind apply is intentionally not implemented in REST-only piece 1"
            .to_owned(),
    );
    log.push(
        "    use dry-run output for review; no gateway/websocket process is launched here"
            .to_owned(),
    );
    true
}
