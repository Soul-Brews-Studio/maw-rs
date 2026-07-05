use maw_discord::is_numeric_snowflake;

const DISPATCH_116: &[DispatcherEntry] = &[DispatcherEntry {
    command: "atlas",
    handler: Handler::Async(atlas_async_native),
}];

const ATLAS_USAGE: &str = "usage: maw atlas <bot> [--guild <id>] [--all-guilds] [--with-threads] [--json]";
const ATLAS_FAKE_DISCORD_ENV: &str = "MAW_RS_ATLAS_FAKE_DISCORD";
const ATLAS_LEGACY_PLUGIN_FALLTHROUGH: &str = "atlas: legacy plugin fallthrough";
const ATLAS_LEGACY_SUBCOMMANDS: &[&str] = &["ls", "read", "backfill", "check", "wake", "vesicle"];

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
        Err(message) if message == ATLAS_LEGACY_PLUGIN_FALLTHROUGH => {
            let mut full_argv = Vec::with_capacity(argv.len() + 1);
            full_argv.push("atlas".to_owned());
            full_argv.extend(argv.iter().cloned());
            return dispatch_cli_plugin_or_unknown(&full_argv, "atlas");
        }
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
    if parsed.bot.is_empty() && atlas_is_legacy_subcommand(value) {
        return Err(ATLAS_LEGACY_PLUGIN_FALLTHROUGH.to_owned());
    }
    if !parsed.bot.is_empty() {
        return Err(format!("atlas: unexpected argument {value}"));
    }
    parsed.bot = atlas_validate_value("bot", value)?;
    Ok(())
}

fn atlas_is_legacy_subcommand(value: &str) -> bool {
    ATLAS_LEGACY_SUBCOMMANDS.contains(&value)
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

include!(concat!(env!("CARGO_MANIFEST_DIR"), "/src/core_impl/atlas_render.rs"));

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

    #[test]
    fn atlas_legacy_subcommands_fall_through_before_bot_parse() {
        for subcommand in ATLAS_LEGACY_SUBCOMMANDS {
            assert_eq!(
                atlas_parse_args(&atlas_strings(&[subcommand])).unwrap_err(),
                ATLAS_LEGACY_PLUGIN_FALLTHROUGH,
                "{subcommand}"
            );
        }
        let parsed = atlas_parse_args(&atlas_strings(&["nova", "--all-guilds", "--with-threads"])).expect("native bot route");
        assert_eq!(parsed.bot, "nova");
        assert!(parsed.all_guilds);
        assert!(parsed.with_threads);
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
