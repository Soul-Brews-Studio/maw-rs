use super::*;

/// Run the native Discord REST plugin command.
///
/// # Errors
///
/// Returns an error output if the reqwest client cannot be constructed.
pub async fn run_discord_command(args: Vec<String>) -> DiscordOutput {
    let Ok(rest) = ReqwestDiscordRest::new() else {
        return DiscordOutput {
            code: 1,
            stdout: String::new(),
            stderr: "failed to initialize Discord REST client\n".to_owned(),
        };
    };
    run_discord_command_with(&args, &DiscordEnv::from_process(), &rest).await
}

pub async fn run_discord_command_with(
    args: &[String],
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
) -> DiscordOutput {
    let mut logs = Vec::new();
    let ok = match args.first().map(|s| s.to_lowercase()) {
        None => {
            usage(&mut logs);
            true
        }
        Some(sub) if matches!(sub.as_str(), "help" | "-h" | "--help") => {
            usage(&mut logs);
            true
        }
        Some(sub) if matches!(sub.as_str(), "version" | "-v" | "--version") => {
            version(&mut logs);
            true
        }
        Some(sub) if sub == "tokens" => tokens(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "status" => status(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "bind" => bind(env, &args[1..], &mut logs),
        Some(sub) if sub == "access" => access(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "guilds" => guilds(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "channels" => channels(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "members" => members(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "inventory" => inventory(env, rest, &args[1..], &mut logs).await,
        Some(sub) if sub == "pair" => pair(env, &args[1..], &mut logs),
        Some(sub) if sub == "route" => route(env, &args[1..], &mut logs),
        Some(sub) if sub == "serve" => serve(env, rest, &args[1..], &mut logs).await,
        Some(sub) => {
            logs.push(format!("unknown subcommand: {sub}"));
            usage(&mut logs);
            false
        }
    };

    DiscordOutput {
        code: if ok { 0 } else { 1 },
        stdout: with_final_newline(&logs.join("\n")),
        stderr: String::new(),
    }
}

pub(super) fn with_final_newline(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else if s.ends_with('\n') {
        s.to_owned()
    } else {
        format!("{s}\n")
    }
}

pub(super) fn usage(log: &mut Vec<String>) {
    log.extend([
        "usage: maw discord <subcommand> [args]".to_owned(),
        String::new(),
        "subcommands:".to_owned(),
        "  version                            show plugin version + subcommand status".to_owned(),
        "  tokens ls                          list all Discord bot tokens in pass (no reveal)".to_owned(),
        "  tokens check [bot]                 verify each token decrypts + Discord REST 200".to_owned(),
        "  status [bot] [--check] [--redact] [--json]".to_owned(),
        "                                     fleet inspection from this host — pass × legacy × hybrid × tmux × registry".to_owned(),
        "  bind <bot> [--apply] [--restart] [--session <name>] [--force]".to_owned(),
        "                                     end-to-end Discord-online for a bot on this host".to_owned(),
        "  access <bot> <list|show|map|add|rm|set|allow|lockdown> [...]".to_owned(),
        "                                     channel + allowlist management per bot (NEW v0.4)".to_owned(),
        String::new(),
        "subcommands (v0.5 native):".to_owned(),
        "  pair <oracle> <channel>            access.json + channel-map.json bootstrap".to_owned(),
        "  route [oracle] <from> <to>          channel-map.json entry".to_owned(),
        "  serve [oracle] [--dry-run]          loopback-only after_send relay".to_owned(),
        String::new(),
        "token strategy: HYBRID — tokens in pass (central), .discord/ config in bot repo.".to_owned(),
        "see: ψ/outbox/ideas/2026-05-17_self-contained-bot-repo-gpg-pattern.md".to_owned(),
    ]);
}

pub(super) fn version(log: &mut Vec<String>) {
    log.extend([
        format!("maw discord v{VERSION}"),
        String::new(),
        "subcommand status:".to_owned(),
        "  ✓ tokens ls / check        v0.1".to_owned(),
        "  ✓ status [bot] [flags]     v0.3.1 (real online/where via bun ancestry)".to_owned(),
        "  ✓ bind <bot>               v0.3 (rewrite to use 'maw wake' pending)".to_owned(),
        "  ✓ access <bot> ...         v0.4 (list/show/map/add/rm/set/allow/lockdown)".to_owned(),
        "  ✓ guilds/channels/members/inventory <bot>  v0.4.2 (Discord-state visibility)".to_owned(),
        "  ✓ pair <oracle> <chan>     v0.5 (seed access + channel-map)".to_owned(),
        "  ✓ route <from> <to>        v0.5 (channel-map entry)".to_owned(),
        "  ✓ serve (after_send hook)  v0.5 (loopback-only relay)".to_owned(),
    ]);
}
