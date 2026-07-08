use super::*;

pub(super) async fn access(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    if args.is_empty() {
        access_usage(log);
        return true;
    }
    let bot = &args[0];
    let sub = args.get(1).map_or("", String::as_str).to_lowercase();
    if sub.is_empty() || matches!(sub.as_str(), "help" | "-h" | "--help") {
        access_usage(log);
        return true;
    }
    let Some(pre) = resolve_bot(env, bot, log) else {
        return true;
    };
    log.push(format!(
        "🪪 maw discord access {bot} {sub}{}",
        if args.len() > 2 {
            format!(" {}", args[2..].join(" "))
        } else {
            String::new()
        }
    ));
    log.push(format!(
        "  state-dir: {}{}",
        pre.state_dir.display(),
        if pre.is_hybrid {
            " (hybrid)"
        } else {
            " (legacy)"
        }
    ));
    log.push(String::new());
    match sub.as_str() {
        "list" => access_list(&pre, &args[2..], log),
        "show" => access_show(&pre, &args[2..], log),
        "map" => access_map(&pre, rest, &args[2..], log).await,
        "add" => access_add(&pre, &args[2..], log),
        "rm" => access_rm(&pre, &args[2..], log),
        "set" => access_set(&pre, &args[2..], log),
        "allow" => access_allow(&pre, &args[2..], log),
        "lockdown" => access_lockdown(&pre, &args[2..], log),
        _ => {
            log.push(format!("✗ unknown subcommand: {sub}"));
            access_usage(log);
            true
        }
    }
}

pub(super) fn access_usage(log: &mut Vec<String>) {
    log.extend([
        "usage: maw discord access <bot> <subcommand> [args]".to_owned(),
        String::new(),
        "subcommands:".to_owned(),
        "  list [--json]                       enabled channels for <bot>".to_owned(),
        "  show <channel> [--json]             inspect one channel's config".to_owned(),
        "  map [--guild <id>] [--refresh]      channel-map (name → id), --refresh from Discord"
            .to_owned(),
        "  add <channel> [--no-mention] [--allow <id>...]".to_owned(),
        "                                      enable channel access".to_owned(),
        "  rm <channel> [--dry-run]            remove channel access".to_owned(),
        "  set <channel> [--no-mention|--mention] [--allow <id>...]".to_owned(),
        "                                      toggle existing channel without rm+add".to_owned(),
        "  allow <add|rm|ls> [<user-id>]       global DM allowlist management".to_owned(),
        "  lockdown [--off] [--dry-run]        dmPolicy=allowlist (or revert with --off)"
            .to_owned(),
    ]);
}

#[derive(Debug, Clone)]
pub(super) struct BotResolved {
    pub(super) bot: String,
    pub(super) state_dir: PathBuf,
    pub(super) token_name: String,
    pub(super) is_hybrid: bool,
    pub(super) access_json: PathBuf,
    pub(super) channel_map: PathBuf,
}

pub(super) fn resolve_bot(
    env: &DiscordEnv,
    bot: &str,
    log: &mut Vec<String>,
) -> Option<BotResolved> {
    if rejects_option_arg(bot) {
        log.push("✗ invalid bot name: leading dash/-- separator rejected".to_owned());
        return None;
    }
    let registry = load_state_dirs_registry(env);
    let hybrid = find_hybrid_discord(env, bot);
    let legacy = find_legacy_state_dir(env, bot);
    let state_dir = match hybrid.clone().or(legacy) {
        Some(path) => path,
        None => {
            log.push(format!("✗ no state-dir found for '{bot}' (checked hybrid <repo>/.discord/ and {}/.claude/channels/{bot}/)", env.home.display()));
            return None;
        }
    };
    let Some(tok) = list_pass_tokens(env).into_iter().find(|t| t.bot == bot) else {
        log.push(format!(
            "✗ no pass entry for '{bot}' (looked for discord/{bot}-token.gpg)"
        ));
        return None;
    };
    if !registry.contains(bot) {
        log.push(format!(
            "⚠ '{bot}' not in discord-oracle/src/state-dirs.ts — dashboard won't see it"
        ));
    }
    Some(BotResolved {
        bot: bot.to_owned(),
        token_name: tok.name,
        is_hybrid: hybrid.is_some(),
        access_json: state_dir.join("access.json"),
        channel_map: state_dir.join("channel-map.json"),
        state_dir,
    })
}

pub(super) fn load_access(path: &Path) -> AccessFile {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(super) fn save_access(path: &Path, access: &AccessFile) -> Result<(), String> {
    let body = serde_json::to_string_pretty(access).map_err(|e| e.to_string())? + "\n";
    fs::write(path, body).map_err(|e| e.to_string())
}

pub(super) fn load_channel_map(path: &Path) -> BTreeMap<String, String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(super) fn save_channel_map(path: &Path, map: &BTreeMap<String, String>) -> Result<(), String> {
    let body = serde_json::to_string_pretty(map).map_err(|e| e.to_string())? + "\n";
    fs::write(path, body).map_err(|e| e.to_string())
}

pub(super) fn resolve_channel(map: &BTreeMap<String, String>, name: &str) -> Option<String> {
    if name.chars().all(|c| c.is_ascii_digit()) {
        Some(name.to_owned())
    } else {
        map.get(name).cloned()
    }
}

pub(super) fn parse_flags(args: &[String]) -> (Vec<String>, HashMap<String, Vec<String>>) {
    let mut pos = Vec::new();
    let mut flags: HashMap<String, Vec<String>> = HashMap::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--no-mention" | "--mention" | "--dry-run" | "--json" | "--refresh" | "--off"
            | "--all-guilds" | "--with-threads" => {
                flags
                    .entry(args[i].trim_start_matches("--").to_owned())
                    .or_default()
                    .push("true".to_owned());
            }
            "--guild" | "--allow" => {
                if let Some(v) = args.get(i + 1) {
                    flags
                        .entry(args[i].trim_start_matches("--").to_owned())
                        .or_default()
                        .push(v.clone());
                    i += 1;
                }
            }
            a => pos.push(a.to_owned()),
        }
        i += 1;
    }
    (pos, flags)
}

pub(super) fn has_flag(flags: &HashMap<String, Vec<String>>, name: &str) -> bool {
    flags.contains_key(name)
}
