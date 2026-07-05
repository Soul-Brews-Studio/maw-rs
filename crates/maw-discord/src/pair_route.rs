use super::*;

pub(super) fn pair(env: &DiscordEnv, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let (Some(bot), Some(channel_arg)) = (pos.first(), pos.get(1)) else {
        log.push("usage: maw discord pair <oracle> <channel> [--allow <user-id>...] [--no-mention] [--dry-run]".to_owned());
        return true;
    };
    if !discord_validate_name(bot, "oracle", log) || !discord_validate_channel_arg(channel_arg, log)
    {
        return false;
    }
    let Some(pre) = resolve_bot(env, bot, log) else {
        return false;
    };
    let mut map = load_channel_map(&pre.channel_map);
    let Some(channel_id) = discord_resolve_or_seed_channel(&mut map, channel_arg) else {
        log.push(format!("✗ channel '{channel_arg}' not in channel-map; use a numeric channel id or run access map --refresh"));
        return false;
    };
    let allow = flags.get("allow").cloned().unwrap_or_default();
    if !allow
        .iter()
        .all(|id| discord_validate_snowflake_for_log(id, "allow", log))
    {
        return false;
    }
    let mut access = load_access(&pre.access_json);
    access.groups.insert(
        channel_id.clone(),
        AccessGroup {
            require_mention: !has_flag(&flags, "no-mention"),
            allow_from: allow.clone(),
        },
    );
    log.push(format!("🔗 maw discord pair {bot} {channel_arg}"));
    log.push(format!("  state-dir: {}", pre.state_dir.display()));
    if has_flag(&flags, "dry-run") {
        log.push(format!("  [dry-run] would enable channel {channel_id}"));
        return true;
    }
    if let Err(error) = save_channel_map(&pre.channel_map, &map) {
        log.push(format!(
            "✗ failed to save channel-map.json: {}",
            discord_redact(&error)
        ));
        return false;
    }
    if let Err(error) = save_access(&pre.access_json, &access) {
        log.push(format!(
            "✗ failed to save access.json: {}",
            discord_redact(&error)
        ));
        return false;
    }
    log.push(format!("  ✓ paired {bot} → {channel_id}"));
    log.push(format!(
        "  ✓ requireMention={}",
        !has_flag(&flags, "no-mention")
    ));
    if !allow.is_empty() {
        log.push(format!("  ✓ allowFrom=[{}]", allow.join(",")));
    }
    true
}

pub(super) fn route(env: &DiscordEnv, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let parsed = match pos.as_slice() {
        [bot, from, to, ..] => DiscordRouteTarget::Bot { bot, from, to },
        [from, to] => DiscordRouteTarget::Env { from, to },
        _ => {
            log.push("usage: maw discord route [<oracle>] <from> <to> [--dry-run]".to_owned());
            return true;
        }
    };
    let Some((label, path)) = discord_route_path(env, parsed, log) else {
        return false;
    };
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let mut map = load_channel_map(&path);
    let (from, to) = parsed.route_pair();
    if !discord_validate_channel_arg(from, log) || !discord_validate_channel_arg(to, log) {
        return false;
    }
    if !is_numeric_snowflake(to) {
        log.push(format!(
            "✗ route target '{to}' must be a numeric Discord channel id"
        ));
        return false;
    }
    log.push(format!("🧭 maw discord route {label} {from} {to}"));
    if has_flag(&flags, "dry-run") {
        log.push(format!("  [dry-run] would map {from} → {to}"));
        return true;
    }
    map.insert(from.to_owned(), to.to_owned());
    if let Err(error) = save_channel_map(&path, &map) {
        log.push(format!(
            "✗ failed to save channel-map.json: {}",
            discord_redact(&error)
        ));
        return false;
    }
    log.push(format!("  ✓ route {from} → {to}"));
    true
}

#[derive(Clone, Copy)]
pub(super) enum DiscordRouteTarget<'a> {
    Bot {
        bot: &'a String,
        from: &'a String,
        to: &'a String,
    },
    Env {
        from: &'a String,
        to: &'a String,
    },
}

impl<'a> DiscordRouteTarget<'a> {
    fn route_pair(self) -> (&'a str, &'a str) {
        match self {
            Self::Bot { from, to, .. } | Self::Env { from, to } => (from, to),
        }
    }
}

pub(super) fn discord_route_path(
    env: &DiscordEnv,
    target: DiscordRouteTarget<'_>,
    log: &mut Vec<String>,
) -> Option<(String, PathBuf)> {
    match target {
        DiscordRouteTarget::Bot { bot, .. } => {
            if !discord_validate_name(bot, "oracle", log) {
                return None;
            }
            let pre = resolve_bot(env, bot, log)?;
            Some((bot.clone(), pre.channel_map))
        }
        DiscordRouteTarget::Env { .. } => {
            let raw = env::var_os("DISCORD_STATE_DIR").map(PathBuf::from)?;
            if !raw.is_absolute() {
                log.push("✗ DISCORD_STATE_DIR must be absolute".to_owned());
                return None;
            }
            Some((
                "$DISCORD_STATE_DIR".to_owned(),
                raw.join("channel-map.json"),
            ))
        }
    }
}

pub(super) fn discord_resolve_or_seed_channel(
    map: &mut BTreeMap<String, String>,
    channel: &str,
) -> Option<String> {
    if is_numeric_snowflake(channel) {
        map.entry(channel.to_owned())
            .or_insert_with(|| channel.to_owned());
        return Some(channel.to_owned());
    }
    map.get(channel).cloned()
}
