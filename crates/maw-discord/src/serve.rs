use super::*;

pub(super) async fn serve(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    args: &[String],
    log: &mut Vec<String>,
) -> bool {
    let (_, flags) = parse_flags(args);
    let pos = discord_serve_positionals(args);
    let bot = pos.first().cloned();
    if bot
        .as_ref()
        .is_some_and(|value| !discord_validate_name(value, "oracle", log))
    {
        return false;
    }
    let host = flag_value(args, "--host").unwrap_or_else(|| "127.0.0.1".to_owned());
    if host != "127.0.0.1" {
        log.push("✗ discord serve refuses non-loopback bind; use 127.0.0.1".to_owned());
        return false;
    }
    let port = flag_value(args, "--port").unwrap_or_else(|| "3457".to_owned());
    if !port.chars().all(|c| c.is_ascii_digit()) {
        log.push("✗ discord serve port must be numeric".to_owned());
        return false;
    }
    if let (Some(bot_name), Some(channel), Some(message)) = (
        bot.as_ref(),
        flag_value(args, "--channel"),
        flag_value(args, "--message"),
    ) {
        return discord_serve_post_once(env, rest, bot_name, &channel, &message, log).await;
    }
    log.push(format!("🛰 maw discord serve listening on {host}:{port}"));
    log.push("  bind: 127.0.0.1 only (fleet-safe)".to_owned());
    if has_flag(&flags, "dry-run") {
        log.push("  [dry-run] listener not started".to_owned());
        return true;
    }
    discord_serve_loopback_once(&host, &port, log)
}

pub(super) async fn discord_serve_post_once(
    env: &DiscordEnv,
    rest: &dyn DiscordRest,
    bot: &str,
    channel: &str,
    message: &str,
    log: &mut Vec<String>,
) -> bool {
    if !discord_validate_channel_arg(channel, log) {
        return false;
    }
    let Some((pre, token, _)) = resolve_bot_for_rest(env, bot, log) else {
        return false;
    };
    let map = load_channel_map(&pre.channel_map);
    let Some(channel_id) = resolve_channel(&map, channel) else {
        log.push(format!("✗ channel '{channel}' not in channel-map"));
        return false;
    };
    if message.chars().any(|ch| ch == '\0') {
        log.push("✗ discord serve message must not contain NUL".to_owned());
        return false;
    }
    let body = json!({"content": message});
    let path = format!("/channels/{channel_id}/messages");
    match rest.post_json(&path, &token, body).await {
        Ok(res) if (200..300).contains(&res.status) => {
            log.push(format!("  ✓ posted Discord message to {channel_id}"));
            true
        }
        Ok(res) => {
            log.push(format!("✗ Discord post failed: status {}", res.status));
            false
        }
        Err(error) => {
            log.push(format!("✗ Discord post failed: {}", discord_redact(&error)));
            false
        }
    }
}

pub(super) fn discord_serve_positionals(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--host" | "--port" | "--channel" | "--message" => i += 1,
            "--dry-run" => {}
            value => out.push(value.to_owned()),
        }
        i += 1;
    }
    out
}

pub(super) fn discord_serve_loopback_once(host: &str, port: &str, log: &mut Vec<String>) -> bool {
    let addr = format!("{host}:{port}");
    let listener = match TcpListener::bind(&addr) {
        Ok(listener) => listener,
        Err(error) => {
            log.push(format!(
                "✗ failed to bind discord serve: {}",
                discord_redact(&error.to_string())
            ));
            return false;
        }
    };
    if let Ok((mut stream, _)) = listener.accept() {
        let mut buffer = [0_u8; 512];
        let _ = stream.read(&mut buffer);
        let _ = stream.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\n\r\nok");
        log.push("  ✓ handled one loopback request".to_owned());
    }
    true
}
