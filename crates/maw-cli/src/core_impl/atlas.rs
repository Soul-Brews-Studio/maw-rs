use maw_discord::is_numeric_snowflake;

const DISPATCH_116: &[DispatcherEntry] = &[DispatcherEntry {
    command: "atlas",
    handler: Handler::Async(atlas_async_native),
}];

const ATLAS_USAGE: &str = "usage: maw atlas <bot> [--guild <id>] [--all-guilds] [--with-threads] [--json]";
const ATLAS_FAKE_DISCORD_ENV: &str = "MAW_RS_ATLAS_FAKE_DISCORD";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct AtlasArgs {
    bot: String,
    guild: Option<String>,
    all_guilds: bool,
    with_threads: bool,
    json: bool,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, Default)]
struct AtlasFakeDiscord {
    bot: String,
    #[serde(default)]
    gateway_events: Vec<String>,
    #[serde(default)]
    guilds: Vec<AtlasGuild>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq, Default)]
struct AtlasGuild {
    id: String,
    name: String,
    #[serde(default)]
    channels: Vec<AtlasChannel>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
struct AtlasChannel {
    id: String,
    name: String,
    #[serde(rename = "type", default)]
    kind: u8,
    #[serde(default)]
    enabled: bool,
    #[serde(default = "atlas_default_require_mention")]
    require_mention: bool,
    #[serde(default)]
    allow_from: Vec<String>,
}

fn atlas_default_require_mention() -> bool { true }

fn atlas_async_native(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { atlas_run_async(&args).await })
}

async fn atlas_run_async(argv: &[String]) -> CliOutput {
    let parsed = match atlas_parse_args(argv) {
        Ok(parsed) => parsed,
        Err(message) if message == ATLAS_USAGE => return atlas_ok(ATLAS_USAGE),
        Err(message) => return atlas_error(&message),
    };
    if let Some(fake) = atlas_fake_discord() {
        return match atlas_render_fake(&parsed, &fake).await {
            Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
            Err(message) => atlas_error(&message),
        };
    }
    atlas_run_real(parsed).await
}

async fn atlas_run_real(parsed: AtlasArgs) -> CliOutput {
    let mut args = if parsed.guild.is_some() {
        vec!["channels".to_owned(), parsed.bot.clone()]
    } else {
        vec!["inventory".to_owned(), parsed.bot.clone()]
    };
    if let Some(guild) = parsed.guild {
        args.push("--guild".to_owned());
        args.push(guild);
    }
    if parsed.all_guilds {
        args.push("--all-guilds".to_owned());
    }
    if parsed.with_threads {
        args.push("--with-threads".to_owned());
    }
    if parsed.json {
        args.push("--json".to_owned());
    }
    let output = run_discord_command(args).await;
    CliOutput { code: output.code, stdout: output.stdout, stderr: output.stderr }
}

fn atlas_parse_args(argv: &[String]) -> Result<AtlasArgs, String> {
    let mut parsed = AtlasArgs::default();
    let mut index = 0;
    while index < argv.len() {
        let token = &argv[index];
        match token.as_str() {
            "help" | "--help" | "-h" => return Err(ATLAS_USAGE.to_owned()),
            "--" => return Err("atlas: -- separator is not allowed".to_owned()),
            "--json" => parsed.json = true,
            "--all-guilds" => parsed.all_guilds = true,
            "--with-threads" => parsed.with_threads = true,
            "--guild" => {
                let guild = atlas_take_value(argv, &mut index, "--guild")?;
                atlas_validate_snowflake("guild", &guild)?;
                parsed.guild = Some(guild);
            }
            value if value.starts_with("--guild=") => {
                let guild = atlas_validate_value("--guild", &value["--guild=".len()..])?;
                atlas_validate_snowflake("guild", &guild)?;
                parsed.guild = Some(guild);
            }
            value if value.starts_with('-') => return Err(format!("atlas: unknown argument {value}")),
            value => atlas_set_bot(&mut parsed, value)?,
        }
        index += 1;
    }
    if parsed.bot.is_empty() {
        return Err(ATLAS_USAGE.to_owned());
    }
    Ok(parsed)
}

fn atlas_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    *index += 1;
    let Some(value) = argv.get(*index) else { return Err(format!("atlas: {flag} requires a value")); };
    atlas_validate_value(flag, value)
}

fn atlas_set_bot(parsed: &mut AtlasArgs, value: &str) -> Result<(), String> {
    if !parsed.bot.is_empty() {
        return Err(format!("atlas: unexpected argument {value}"));
    }
    parsed.bot = atlas_validate_value("bot", value)?;
    Ok(())
}

fn atlas_validate_value(label: &str, value: &str) -> Result<String, String> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || value.contains('\0')
        || value == "--"
    {
        return Err(format!("atlas: invalid {label} value"));
    }
    Ok(value.to_owned())
}

fn atlas_validate_snowflake(label: &str, value: &str) -> Result<(), String> {
    if is_numeric_snowflake(value) {
        Ok(())
    } else {
        Err(format!("atlas: invalid {label} id '{value}'"))
    }
}

fn atlas_fake_discord() -> Option<AtlasFakeDiscord> {
    let raw = std::env::var(ATLAS_FAKE_DISCORD_ENV).ok()?;
    serde_json::from_str(&raw).ok()
}

async fn atlas_render_fake(parsed: &AtlasArgs, fake: &AtlasFakeDiscord) -> Result<String, String> {
    if fake.bot != parsed.bot {
        return Err(format!("atlas: fake discord has bot '{}', requested '{}'", fake.bot, parsed.bot));
    }
    let guilds = atlas_filter_guilds(parsed, &fake.guilds)?;
    let gateway_events = atlas_gateway_observed_count(&fake.gateway_events).await;
    if parsed.json {
        return Ok(atlas_render_json(&fake.bot, gateway_events, &guilds));
    }
    Ok(atlas_render_text(&fake.bot, gateway_events, &guilds))
}

fn atlas_filter_guilds(parsed: &AtlasArgs, guilds: &[AtlasGuild]) -> Result<Vec<AtlasGuild>, String> {
    let mut selected = if let Some(guild_id) = &parsed.guild {
        guilds.iter().filter(|guild| &guild.id == guild_id).cloned().collect::<Vec<_>>()
    } else if parsed.all_guilds {
        guilds.to_vec()
    } else {
        guilds.iter().take(1).cloned().collect()
    };
    if !parsed.with_threads {
        for guild in &mut selected {
            guild.channels.retain(|channel| !matches!(channel.kind, 10..=12));
        }
    }
    atlas_validate_fake_ids(&selected)?;
    Ok(selected)
}

fn atlas_validate_fake_ids(guilds: &[AtlasGuild]) -> Result<(), String> {
    for guild in guilds {
        atlas_validate_snowflake("guild", &guild.id)?;
        for channel in &guild.channels {
            atlas_validate_snowflake("channel", &channel.id)?;
            for user in &channel.allow_from {
                atlas_validate_snowflake("user", user)?;
            }
        }
    }
    Ok(())
}

async fn atlas_gateway_observed_count(events: &[String]) -> usize {
    maw_discord::gateway::observe_mock_gateway_events(events).await
}

fn atlas_render_json(bot: &str, gateway_events: usize, guilds: &[AtlasGuild]) -> String {
    let value = serde_json::json!({
        "bot": bot,
        "gatewayEvents": gateway_events,
        "guilds": guilds,
    });
    format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default())
}

fn atlas_render_text(bot: &str, gateway_events: usize, guilds: &[AtlasGuild]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "🗺️ atlas — Discord oracle registry for {bot}");
    let _ = writeln!(out, "  gateway: {gateway_events} event(s) observed");
    let mut total_channels = 0usize;
    let mut total_enabled = 0usize;
    for guild in guilds {
        total_channels = total_channels.saturating_add(guild.channels.len());
        total_enabled = total_enabled.saturating_add(guild.channels.iter().filter(|channel| channel.enabled).count());
        let _ = writeln!(out, "  ▼ {} ({}) · {} channel(s)", guild.name, guild.id, guild.channels.len());
        let mut channels = guild.channels.clone();
        channels.sort_by(|left, right| left.name.cmp(&right.name));
        for channel in channels {
            let _ = writeln!(out, "{}", atlas_render_channel(&channel));
        }
    }
    let _ = writeln!(out, "summary: {} server(s) · {total_channels} channels visible · {total_enabled} enabled", guilds.len());
    out
}

fn atlas_render_channel(channel: &AtlasChannel) -> String {
    if channel.enabled {
        let mention = if channel.require_mention { "mention" } else { "all-msg" };
        let allow = if channel.allow_from.is_empty() { "EVERYONE".to_owned() } else { channel.allow_from.join(",") };
        format!("     ✓ #{:<36} {mention} {allow}", channel.name)
    } else {
        format!("     · #{:<36} (in guild, no access)", channel.name)
    }
}

fn atlas_ok(stdout: &str) -> CliOutput {
    CliOutput { code: 0, stdout: format!("{stdout}\n"), stderr: String::new() }
}

fn atlas_error(message: &str) -> CliOutput {
    let code = if message == ATLAS_USAGE { 2 } else { 1 };
    CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") }
}

#[cfg(test)]
mod atlas_tests {
    use super::*;

    fn atlas_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn atlas_parse_validates_snowflakes_and_guards_args() {
        let parsed = atlas_parse_args(&atlas_strings(&["nova", "--guild", "123456789012345678", "--json"])).expect("parse");
        assert_eq!(parsed.bot, "nova");
        assert_eq!(parsed.guild.as_deref(), Some("123456789012345678"));
        assert!(parsed.json);
        assert!(atlas_parse_args(&atlas_strings(&["nova", "--guild", "abc"])).unwrap_err().contains("invalid guild id"));
        assert!(atlas_parse_args(&atlas_strings(&["--bad"])).unwrap_err().contains("unknown argument"));
        assert!(atlas_parse_args(&atlas_strings(&["nova", "--"])).unwrap_err().contains("separator"));
        assert!(atlas_parse_args(&["no\npe".to_owned()]).unwrap_err().contains("invalid bot"));
    }

    #[tokio::test]
    async fn atlas_fake_gateway_subscribe_counts_events() {
        assert_eq!(atlas_gateway_observed_count(&["heartbeat".to_owned(), "heartbeat-ack".to_owned()]).await, 2);
    }

    #[test]
    fn atlas_dispatch_registers_native() {
        assert_eq!(dispatcher_status("atlas"), DispatchKind::Native);
        assert_eq!(DISPATCH_116[0].command, "atlas");
    }
}
