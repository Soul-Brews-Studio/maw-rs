use super::*;

pub(super) async fn guilds(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let Some(bot) = args.first() else {
        log.push("usage: maw discord guilds <bot> [--json]".to_owned());
        return true;
    };
    let Some(pre) = resolve_bot_for_rest(env, bot, log) else {
        return true;
    };
    let json_flag = args.iter().any(|a| a == "--json");
    let Ok(guilds) = fetch_guilds(rest, &pre.1).await else {
        log.push("✗ guilds REST failed".to_owned());
        return true;
    };
    if json_flag {
        log.push(
            serde_json::to_string_pretty(&json!({"bot": bot, "guilds": guilds}))
                .unwrap_or_default(),
        );
        return true;
    }
    log.push(format!("🌐 {bot} is in {} server(s):", guilds.len()));
    log.push(String::new());
    log.push("  id                    name".to_owned());
    log.push("  ────────────────────  ────────────────────────────────────".to_owned());
    for guild in guilds {
        log.push(format!("  {}  {}", guild.id, guild.name));
    }
    true
}

pub(super) async fn channels(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let Some(bot) = args.first() else {
        log.push("usage: maw discord channels <bot> [--guild <id>] [--all-guilds] [--json] [--with-threads]".to_owned());
        return true;
    };
    let Some((_, token, _)) = resolve_bot_for_rest(env, bot, log) else {
        return true;
    };
    let (_, flags) = parse_flags(&args[1..]);
    let guilds = fetch_guilds(rest, &token).await.unwrap_or_default();
    let targets = flags
        .get("guild")
        .and_then(|v| v.first())
        .map_or(guilds.clone(), |id| {
            guilds.into_iter().filter(|g| &g.id == id).collect()
        });
    let mut out = Vec::new();
    for guild in targets {
        match fetch_channels(rest, &token, &guild.id).await {
            Ok(chs) => out.push((guild, chs)),
            Err(e) => log.push(format!("  ⚠ {} {}: {e}", guild.id, guild.name)),
        }
    }
    if has_flag(&flags, "json") {
        log.push(serde_json::to_string_pretty(&json!({"bot": bot, "guilds": out.iter().map(|(g, c)| json!({"guild": g, "channels": c})).collect::<Vec<_>>() })).unwrap_or_default());
        return true;
    }
    log.push(format!("📺 {bot} channels across {} guild(s):", out.len()));
    log.push(String::new());
    for (guild, chs) in out {
        log.push(format!(
            "  ▼ {} ({})  ·  {} channel(s)",
            guild.name,
            guild.id,
            chs.len()
        ));
        for c in chs
            .iter()
            .filter(|c| has_flag(&flags, "with-threads") || !matches!(c.kind, 10..=12))
        {
            log.push(format!(
                "     {}  {:<6}  #{:<36} {}",
                c.id,
                channel_type_label(c.kind),
                c.name,
                c.parent_id.clone().unwrap_or_default()
            ));
        }
        log.push(String::new());
    }
    true
}

pub(super) async fn members(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let (Some(bot), Some(channel_arg)) = (args.first(), args.get(1)) else {
        log.push("usage: maw discord members <bot> <channel-name-or-id> [--json]".to_owned());
        return true;
    };
    let Some((pre, token, _)) = resolve_bot_for_rest(env, bot, log) else {
        return true;
    };
    let map = load_channel_map(&pre.channel_map);
    let Some(channel_id) = resolve_channel(&map, channel_arg) else {
        log.push(format!("✗ channel '{channel_arg}' not in channel-map. Run 'maw discord access {bot} map --guild <id> --refresh'"));
        return true;
    };
    let access = load_access(&pre.access_json);
    let Some(cfg) = access.groups.get(&channel_id) else {
        log.push(format!(
            "✗ channel {channel_id} not in access.json groups for {bot}"
        ));
        return true;
    };
    let pairs = resolve_user_list(rest, &token, &cfg.allow_from).await;
    let result = json!({"bot": bot, "channelId": channel_id, "requireMention": cfg.require_mention, "allowFrom": pairs, "effective": if cfg.allow_from.is_empty() {"mention-only"} else {"allowlist"}});
    if args.iter().any(|a| a == "--json") {
        log.push(serde_json::to_string_pretty(&result).unwrap_or_default());
        return true;
    }
    log.push(format!("👥 {bot} · #{channel_arg} ({channel_id})"));
    log.push(format!("   requireMention: {}", cfg.require_mention));
    if cfg.allow_from.is_empty() {
        log.push("   allowFrom:      (none)".to_owned());
    } else {
        log.push("   allowFrom:".to_owned());
        for pair in result["allowFrom"].as_array().into_iter().flatten() {
            log.push(format!(
                "     · {:<18} ({})",
                pair["name"].as_str().unwrap_or_default(),
                pair["id"].as_str().unwrap_or_default()
            ));
        }
    }
    log.push(format!(
        "   effective:      {}",
        result["effective"].as_str().unwrap_or_default()
    ));
    true
}

pub(super) async fn inventory(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let Some(bot) = args.first() else {
        log.push("usage: maw discord inventory <bot> [--json]".to_owned());
        return true;
    };
    let Some((pre, token, _)) = resolve_bot_for_rest(env, bot, log) else {
        return true;
    };
    let guilds = fetch_guilds(rest, &token).await.unwrap_or_default();
    let access = load_access(&pre.access_json);
    let mut rows = Vec::new();
    let mut total_channels = 0usize;
    let mut total_enabled = 0usize;
    for guild in guilds {
        match fetch_channels(rest, &token, &guild.id).await {
            Ok(chs) => {
                total_channels += chs.len();
                total_enabled += chs
                    .iter()
                    .filter(|c| access.groups.contains_key(&c.id))
                    .count();
                rows.push((guild, chs));
            }
            Err(e) => log.push(format!("  ⚠ {} {}: {e}", guild.id, guild.name)),
        }
    }
    if args.iter().any(|a| a == "--json") {
        log.push(serde_json::to_string_pretty(&json!({"bot": bot, "inventory": rows.iter().map(|(g, c)| json!({"guild": g, "channels": c})).collect::<Vec<_>>() })).unwrap_or_default());
        return true;
    }
    let all_ids = access
        .groups
        .values()
        .flat_map(|g| g.allow_from.clone())
        .collect::<BTreeSet<_>>();
    let names = resolve_user_list(rest, &token, &all_ids.into_iter().collect::<Vec<_>>()).await;
    let name_by_id = names
        .into_iter()
        .filter_map(|v| {
            Some((
                v.get("id")?.as_str()?.to_owned(),
                v.get("name")?.as_str()?.to_owned(),
            ))
        })
        .collect::<HashMap<_, _>>();
    log.push(format!("📋 {bot} — full inventory"));
    log.push(String::new());
    for (guild, channels) in rows {
        let enabled = channels
            .iter()
            .filter(|c| access.groups.contains_key(&c.id))
            .count();
        log.push(format!(
            "  ▼ {}  ({})  ·  {enabled}/{} enabled",
            guild.name,
            guild.id,
            channels.len()
        ));
        for channel in channels {
            if let Some(cfg) = access.groups.get(&channel.id) {
                let mention = if cfg.require_mention {
                    "mention"
                } else {
                    "all-msg"
                };
                let allow = if cfg.allow_from.is_empty() {
                    "(none)".to_owned()
                } else {
                    cfg.allow_from
                        .iter()
                        .map(|id| name_by_id.get(id).cloned().unwrap_or_else(|| id.clone()))
                        .collect::<Vec<_>>()
                        .join(",")
                };
                log.push(format!("     ✓ #{:<36} {mention} {allow}", channel.name));
            } else {
                log.push(format!(
                    "     · #{:<36} (in guild, no access)",
                    channel.name
                ));
            }
        }
        log.push(String::new());
    }
    log.push(format!("summary: {} server(s) · {total_channels} channels visible · {total_enabled} enabled · {} unique allow-users resolved", fetch_guilds(rest, &token).await.unwrap_or_default().len(), name_by_id.len()));
    true
}
