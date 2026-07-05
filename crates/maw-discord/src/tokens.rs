use super::*;

pub(super) fn list_pass_tokens(env: &DiscordEnv) -> Vec<TokenEntry> {
    let dir = env.pass_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let name = path.file_name()?.to_str()?.strip_suffix(".gpg")?.to_owned();
            let meta = entry.metadata().ok()?;
            let bot = name.strip_suffix("-token").unwrap_or(&name).to_owned();
            Some(TokenEntry {
                name,
                bot,
                file: path,
                size_bytes: meta.len(),
                modified: meta.modified().ok(),
            })
        })
        .collect::<Vec<_>>();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub(super) fn decrypt_token(name: &str) -> Option<String> {
    if let Ok(token) = env::var("DISCORD_BOT_TOKEN") {
        let trimmed = token.trim().to_owned();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if rejects_option_arg(name) || name.contains('/') || name.contains("..") {
        return None;
    }
    let out = Command::new("pass")
        .args(["show", &format!("discord/{name}")])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let token = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!token.is_empty()).then_some(token)
}

pub(super) fn rejects_option_arg(value: &str) -> bool {
    value == "--" || value.starts_with('-')
}

pub(super) async fn ping(rest: &dyn DiscordRest, token: &str) -> (bool, u16, Option<String>) {
    match rest.get_json("/users/@me", token).await {
        Ok(res) if (200..300).contains(&res.status) => (
            true,
            res.status,
            res.body
                .get("username")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        ),
        Ok(res) => (false, res.status, None),
        Err(_) => (false, 0, None),
    }
}

pub(super) async fn tokens(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let action = args.first().map_or("ls", String::as_str).to_lowercase();
    match action.as_str() {
        "ls" => tokens_ls(env, log),
        "check" => tokens_check(env, rest, args.get(1).map(String::as_str), log).await,
        _ => {
            log.push(format!("unknown subcommand: tokens {action}"));
            log.push("usage: maw discord tokens <ls|check> [bot]".to_owned());
            false
        }
    }
}

pub(super) fn tokens_ls(env: &DiscordEnv, log: &mut Vec<String>) -> bool {
    let tokens = list_pass_tokens(env);
    if tokens.is_empty() {
        log.push("✗ no tokens in pass (~/.password-store/discord/)".to_owned());
        log.push("hint: pass insert discord/<bot>-token".to_owned());
        return true;
    }
    log.push(format!(
        "📦 {} token(s) in pass (~/.password-store/discord/)",
        tokens.len()
    ));
    log.push(String::new());
    log.push("  name                                  size    last-modified".to_owned());
    log.push("  ──────────────────────────────────────────────────────────────".to_owned());
    for token in tokens {
        log.push(format!(
            "  {:<38}{:<7} {}",
            token.name,
            format!("{}B", token.size_bytes),
            token.modified.map_or_else(|| "—".to_owned(), ymd_utc)
        ));
    }
    log.push(String::new());
    log.push("use 'maw discord tokens check' to verify each one decrypts + Discord 200".to_owned());
    true
}

pub(super) async fn tokens_check(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    only: Option<&str>,
    log: &mut Vec<String>,
) -> bool {
    let tokens = list_pass_tokens(env);
    if tokens.is_empty() {
        log.push("✗ no tokens to check".to_owned());
        return true;
    }
    let filtered = tokens
        .into_iter()
        .filter(|t| {
            only.is_none_or(|needle| {
                t.name == needle || t.name == format!("{needle}-token") || t.bot == needle
            })
        })
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        let needle = only.unwrap_or_default();
        log.push(format!(
            "✗ no token matching '{needle}' (tried '{needle}', '{needle}-token', bot=='{needle}')"
        ));
        return true;
    }
    log.push(format!("🔐 checking {} token(s)...", filtered.len()));
    log.push(String::new());
    log.push("  name                                  decrypt  discord  bot".to_owned());
    log.push("  ──────────────────────────────────────────────────────────────────".to_owned());
    let mut ok_count = 0;
    let mut fail_count = 0;
    for entry in &filtered {
        let name = format!("{:<38}", entry.name);
        let Some(token) = decrypt_token(&entry.name) else {
            log.push(format!("  {name}✗ fail   —        —"));
            fail_count += 1;
            continue;
        };
        let (ok, status, username) = ping(rest, &token).await;
        let status_text = if ok {
            format!("✓ {status}    ")
        } else if status == 0 {
            "✗ ERR   ".to_owned()
        } else {
            format!("✗ {status}   ")
        };
        log.push(format!(
            "  {name}✓ OK    {status_text} {}",
            username.unwrap_or_else(|| "—".to_owned())
        ));
        if ok {
            ok_count += 1;
        } else {
            fail_count += 1;
        }
    }
    log.push(String::new());
    log.push(format!(
        "summary: {ok_count}/{} green{}",
        filtered.len(),
        if fail_count > 0 {
            format!(", {fail_count} fail")
        } else {
            String::new()
        }
    ));
    true
}
