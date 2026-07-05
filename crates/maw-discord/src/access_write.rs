use super::*;

pub(super) fn access_add(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let Some(channel_arg) = pos.first() else {
        log.push(
            "usage: maw discord access <bot> add <channel> [--no-mention] [--allow <id>...]"
                .to_owned(),
        );
        return true;
    };
    let map = load_channel_map(&pre.channel_map);
    let Some(id) = resolve_channel(&map, channel_arg) else {
        log.push(format!("✗ channel '{channel_arg}' not in channel-map"));
        log.push(format!(
            "  run 'maw discord access {} map --refresh' first",
            pre.bot
        ));
        return true;
    };
    let mut access = load_access(&pre.access_json);
    let allow = flags.get("allow").cloned().unwrap_or_default();
    access.groups.insert(
        id.clone(),
        AccessGroup {
            require_mention: !has_flag(&flags, "no-mention"),
            allow_from: allow.clone(),
        },
    );
    if let Err(error) = save_access(&pre.access_json, &access) {
        log.push(format!("✗ failed to save access.json: {error}"));
        return true;
    }
    log.push(format!("  ✓ enabled #{channel_arg} ({id})"));
    if has_flag(&flags, "no-mention") || !allow.is_empty() {
        log.push(format!(
            "  ✓ flags applied: {}{}",
            if has_flag(&flags, "no-mention") {
                "mention=false "
            } else {
                ""
            },
            if allow.is_empty() {
                String::new()
            } else {
                format!("allow=[{}]", allow.join(","))
            }
        ));
    } else {
        log.push(
            "  (defaults applied: requireMention=true, allowFrom=[$DISCORD_USER_ID])".to_owned(),
        );
    }
    true
}

pub(super) fn access_rm(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let Some(channel_arg) = pos.first() else {
        log.push("usage: maw discord access <bot> rm <channel> [--dry-run]".to_owned());
        return true;
    };
    let map = load_channel_map(&pre.channel_map);
    let Some(id) = resolve_channel(&map, channel_arg) else {
        log.push(format!("✗ channel '{channel_arg}' not in channel-map"));
        return true;
    };
    let mut access = load_access(&pre.access_json);
    if !access.groups.contains_key(&id) {
        log.push(format!("✗ channel '{channel_arg}' not currently enabled"));
        return true;
    }
    if has_flag(&flags, "dry-run") {
        log.push(format!(
            "  [dry-run] would remove #{channel_arg} ({id}) from access"
        ));
        log.push(format!(
            "            current config: {}",
            serde_json::to_string(&access.groups[&id]).unwrap_or_default()
        ));
        return true;
    }
    access.groups.remove(&id);
    if let Err(error) = save_access(&pre.access_json, &access) {
        log.push(format!("✗ failed to save access.json: {error}"));
    } else {
        log.push(format!("  ✓ removed #{channel_arg} ({id})"));
    }
    true
}

pub(super) fn access_set(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let (pos, flags) = parse_flags(args);
    let Some(channel_arg) = pos.first() else {
        log.push("usage: maw discord access <bot> set <channel> [--no-mention|--mention] [--allow <id>...]".to_owned());
        return true;
    };
    let map = load_channel_map(&pre.channel_map);
    let Some(id) = resolve_channel(&map, channel_arg) else {
        log.push(format!("✗ channel '{channel_arg}' not in channel-map"));
        return true;
    };
    let mut access = load_access(&pre.access_json);
    let Some(cfg) = access.groups.get_mut(&id) else {
        log.push("✗ channel not currently enabled — use 'add' instead".to_owned());
        return true;
    };
    if has_flag(&flags, "no-mention") {
        cfg.require_mention = false;
    }
    if has_flag(&flags, "mention") {
        cfg.require_mention = true;
    }
    if let Some(allow) = flags.get("allow") {
        cfg.allow_from = allow.clone();
    }
    if let Err(error) = save_access(&pre.access_json, &access) {
        log.push(format!("✗ failed to save access.json: {error}"));
    } else if let Some(cfg) = access.groups.get(&id) {
        log.push(format!(
            "  ✓ updated: {}allow=[{}]",
            if cfg.require_mention {
                "mention=true "
            } else {
                "mention=false "
            },
            cfg.allow_from.join(",")
        ));
    }
    true
}

pub(super) fn access_allow(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let action = args.first().map_or("", String::as_str);
    if !matches!(action, "add" | "rm" | "ls") {
        log.push("usage: maw discord access <bot> allow <add|rm|ls> [<user-id>]".to_owned());
        return true;
    }
    let mut access = load_access(&pre.access_json);
    if action == "ls" {
        log.push(format!("  global allowlist ({}):", access.allow_from.len()));
        for id in access.allow_from {
            log.push(format!("    {id}"));
        }
        return true;
    }
    let Some(user_id) = args.get(1) else {
        log.push(format!(
            "usage: maw discord access <bot> allow {action} <user-id>"
        ));
        return true;
    };
    if action == "add" {
        if access.allow_from.contains(user_id) {
            log.push(format!("  ○ {user_id} already in allowlist"));
        } else {
            access.allow_from.push(user_id.clone());
            let _ = save_access(&pre.access_json, &access);
            log.push(format!("  ✓ added {user_id} to global allowlist"));
        }
    } else if let Some(index) = access.allow_from.iter().position(|id| id == user_id) {
        access.allow_from.remove(index);
        let _ = save_access(&pre.access_json, &access);
        log.push(format!("  ✓ removed {user_id} from global allowlist"));
    } else {
        log.push(format!("  ○ {user_id} not in allowlist"));
    }
    true
}

pub(super) fn access_lockdown(pre: &BotResolved, args: &[String], log: &mut Vec<String>) -> bool {
    let (_, flags) = parse_flags(args);
    let mut access = load_access(&pre.access_json);
    let target = if has_flag(&flags, "off") {
        "open"
    } else {
        "allowlist"
    };
    let current = access.dm_policy.clone();
    if current == target {
        log.push(format!("  ○ dmPolicy already '{target}' — no change"));
        return true;
    }
    if has_flag(&flags, "dry-run") {
        log.push(format!(
            "  [dry-run] would set dmPolicy: '{current}' → '{target}'"
        ));
        return true;
    }
    access.dm_policy = target.to_owned();
    let _ = save_access(&pre.access_json, &access);
    log.push(format!("  ✓ dmPolicy: '{current}' → '{target}'"));
    true
}
