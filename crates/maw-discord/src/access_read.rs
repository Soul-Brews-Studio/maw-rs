use super::*;

pub(super) fn access_list(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let (_, flags) = parse_flags(args);
    let access = load_access(&pre.access_json);
    let map = load_channel_map(&pre.channel_map);
    let reverse = reverse_map(&map);
    let entries = access
        .groups
        .iter()
        .map(|(id, cfg)| {
            (
                id,
                reverse
                    .get(id)
                    .cloned()
                    .unwrap_or_else(|| "(unknown)".to_owned()),
                cfg,
            )
        })
        .collect::<Vec<_>>();
    if has_flag(&flags, "json") {
        log.push(serde_json::to_string_pretty(&json!({"bot": pre.bot, "channels": entries.iter().map(|(id, name, cfg)| json!({"id": id, "name": name, "requireMention": cfg.require_mention, "allowFrom": cfg.allow_from})).collect::<Vec<_>>() })).unwrap_or_default());
        return true;
    }
    if entries.is_empty() {
        log.push("  (no channels enabled)".to_owned());
        return true;
    }
    log.push(format!("  {} channel(s):", entries.len()));
    log.push(String::new());
    log.push(
        "  channel-name                     id                    mention  allowFrom".to_owned(),
    );
    log.push(
        "  ─────────────────────────────────────────────────────────────────────────".to_owned(),
    );
    for (id, name, cfg) in entries {
        let mention = if cfg.require_mention {
            "✓ tag  "
        } else {
            "○ all  "
        };
        log.push(format!(
            "  {:<32} {:<20}  {mention}  {}",
            name,
            id,
            if cfg.allow_from.is_empty() {
                "(none)".to_owned()
            } else {
                cfg.allow_from.join(",")
            }
        ));
    }
    true
}

pub(super) fn access_show(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let Some(channel_arg) = pos.first() else {
        log.push("usage: maw discord access <bot> show <channel> [--json]".to_owned());
        return true;
    };
    let map = load_channel_map(&pre.channel_map);
    let Some(id) = resolve_channel(&map, channel_arg) else {
        log.push(format!(
            "✗ channel '{channel_arg}' not in channel-map (run 'access map --refresh')"
        ));
        return true;
    };
    let access = load_access(&pre.access_json);
    let Some(cfg) = access.groups.get(&id) else {
        log.push(format!(
            "✗ channel '{channel_arg}' ({id}) not in access.json"
        ));
        return true;
    };
    let reverse = reverse_map(&map);
    let name = reverse
        .get(&id)
        .cloned()
        .unwrap_or_else(|| "(unknown)".to_owned());
    if has_flag(&flags, "json") {
        log.push(serde_json::to_string_pretty(&json!({"bot": pre.bot, "channel": {"id": id, "name": name}, "requireMention": cfg.require_mention, "allowFrom": cfg.allow_from })).unwrap_or_default());
        return true;
    }
    log.push(format!("  #{name} ({id})"));
    log.push(format!("    requireMention: {}", cfg.require_mention));
    log.push(format!(
        "    allowFrom:      {}",
        if cfg.allow_from.is_empty() {
            "(none)".to_owned()
        } else {
            cfg.allow_from.join(", ")
        }
    ));
    true
}

pub(super) async fn access_map(
    pre: &BotResolved,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let (_, flags) = parse_flags(args);
    if has_flag(&flags, "refresh") {
        log.push("  refreshing channel-map from Discord...".to_owned());
        match decrypt_token_result(&pre.token_name) {
            Ok(token) => {
                let guilds = fetch_guilds(rest, &token).await.unwrap_or_default();
                let mut map = load_channel_map(&pre.channel_map);
                let guild_filter = flags.get("guild").and_then(|v| v.first());
                for guild in guilds
                    .iter()
                    .filter(|g| guild_filter.is_none_or(|id| id == &g.id))
                {
                    if let Ok(channels) = fetch_channels(rest, &token, &guild.id).await {
                        for channel in channels {
                            if channel.kind == 0 || channel.kind == 5 {
                                map.insert(channel.name, channel.id);
                            }
                        }
                    }
                }
                if let Err(error) = save_channel_map(&pre.channel_map, &map) {
                    log.push(format!("  ✗ failed to write channel-map: {error}"));
                } else {
                    log.push(format!("    wrote {} channel(s)", map.len()));
                }
                log.push(String::new());
            }
            Err(error) => log.push(format!("  ✗ {error}")),
        }
    }
    let map = load_channel_map(&pre.channel_map);
    if map.is_empty() {
        log.push("  (no channels mapped — run with --refresh --guild <id>)".to_owned());
        return true;
    }
    log.push(format!("  {} channel(s) in map:", map.len()));
    log.push(String::new());
    log.push("  channel-name                     id".to_owned());
    log.push("  ──────────────────────────────────────────────".to_owned());
    for (name, id) in map {
        log.push(format!("  {:<32} {id}", name));
    }
    true
}
