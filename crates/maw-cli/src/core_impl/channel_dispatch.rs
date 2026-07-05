const DISPATCH_120: &[DispatcherEntry] = &[DispatcherEntry { command: "channel", handler: Handler::Sync(channel_run_command) }];

const CHANNEL_HELP: &str = "usage: maw channel <subcommand> [args]\n\nsubcommands:\n  ls [oracle] [--json] [-v] list channels (all or for specific oracle)\n  add <oracle> <plugin>    add channel plugin to oracle\n  rm <oracle> <plugin>     remove channel plugin from oracle\n  providers                list available channel providers\n  setup <oracle>           interactive channel setup wizard\n  test <oracle>            test channel configuration\n  migrate --to-repo [...]  copy global ~/.claude/channels/<oracle>/config.json\n                           into each oracle's <repo>/.claude/channel.json\n                           ([oracle...] empty = all; --dry-run / --remove-global)\n\nshorthand: discord → plugin:discord@claude-plugins-official\ngithub: prefix → delegates to setup wizard";

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct ChannelConfig {
    plugins: Vec<ChannelPlugin>,
    #[serde(skip_serializing_if = "Option::is_none")]
    token_source: Option<String>,
    #[serde(rename = "permissionMode", skip_serializing_if = "Option::is_none")]
    permission_mode: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
struct ChannelPlugin {
    id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    env: Option<std::collections::BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mcp: Option<ChannelMcpConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    dev: Option<bool>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
struct ChannelMcpConfig {
    command: String,
    args: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    untrusted: Option<bool>,
}

#[derive(Debug, Clone)]
struct ChannelAddArgs {
    oracle: String,
    plugin_id: String,
    repo_path: Option<std::path::PathBuf>,
    env: std::collections::BTreeMap<String, String>,
    pass_key: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelProvider {
    short_name: String,
    plugin_id: String,
    kind: &'static str,
}


#[derive(Debug, Clone)]
struct ChannelSetupArgs {
    oracle: String,
    provider: ChannelSetupProvider,
    pass_key: Option<String>,
    guild_id: Option<String>,
    env: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChannelSetupProvider {
    Discord,
    Telegram,
    Imessage,
    Github(String),
}

impl ChannelSetupProvider {
    fn name(&self) -> &str {
        match self {
            Self::Discord => "discord",
            Self::Telegram => "telegram",
            Self::Imessage => "imessage",
            Self::Github(_) => "github",
        }
    }

    fn plugin_id(&self) -> String {
        match self {
            Self::Discord => "plugin:discord@claude-plugins-official".to_owned(),
            Self::Telegram => "plugin:telegram@claude-plugins-official".to_owned(),
            Self::Imessage => "plugin:imessage@claude-plugins-official".to_owned(),
            Self::Github(_) => "server:relay".to_owned(),
        }
    }
}

#[derive(Debug, Clone)]
struct ChannelDiscordGuild {
    id: String,
    name: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelAccessConfig {
    dm_policy: String,
    allow_from: Vec<String>,
    groups: serde_json::Value,
    pending: serde_json::Value,
}


#[derive(Debug, Clone)]
struct ChannelMigrateArgs {
    targets: Vec<String>,
    dry_run: bool,
    remove_global: bool,
}

#[derive(Debug, Default)]
struct ChannelMigrateCounts {
    migrated: usize,
    skipped: usize,
    failed: usize,
}

fn channel_run_command(argv: &[String]) -> CliOutput {
    match channel_run(argv) {
        Ok(stdout) | Err((0, stdout)) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err((code, message)) => CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn channel_run(argv: &[String]) -> Result<String, (i32, String)> {
    let sub = argv.first().map(|value| value.to_ascii_lowercase());
    match sub.as_deref() {
        Some("help" | "--help" | "-h") => Ok(format!("{CHANNEL_HELP}\n")),
        Some("ls" | "list") | None => channel_ls(&argv[1.min(argv.len())..]),
        Some("providers") => channel_providers(&argv[1..]),
        Some("test") => channel_test(&argv[1..]),
        Some("add") => channel_add(&argv[1..]),
        Some("rm" | "remove") => channel_rm(&argv[1..]),
        Some("setup") => channel_setup(&argv[1..]),
        Some("migrate") => channel_migrate(&argv[1..]),
        Some(_) => Ok(channel_short_usage()),
    }
}

fn channel_short_usage() -> String {
    "usage: maw channel <add|rm|ls|providers|setup|test|migrate> [oracle] [plugin]\n\n  maw channel providers                          list available providers\n  maw channel setup hermes-discord discord       interactive wizard\n  maw channel setup myoracle github:org/repo     git channel wizard\n  maw channel add hermes-discord discord         quick register\n  maw channel add myoracle github:org/repo       git channel\n  maw channel rm hermes-discord discord          remove channel\n  maw channel ls                                 list all\n  maw channel test hermes-discord                verify connectivity\n  maw channel migrate --to-repo [oracle...]      global → repo (#1195)\n\n  maw wake <oracle> auto-injects --channels when config exists\n".to_owned()
}

fn channel_add(argv: &[String]) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let add_args = channel_parse_add(argv)?;
    let path = channel_config_path_for_add(&add_args.oracle, add_args.repo_path.as_deref());
    let mut config = channel_load_config_at(&path).unwrap_or_default();
    if config.plugins.iter().any(|plugin| plugin.id == add_args.plugin_id) {
        return Ok(format!("  \x1b[33m⚠\x1b[0m '{}' already registered for {}\n", add_args.plugin_id, add_args.oracle));
    }

    let plugin = channel_new_plugin(&add_args);
    if let Some(pass_key) = &add_args.pass_key { config.token_source = Some(format!("pass:{pass_key}")); }
    config.plugins.push(plugin.clone());
    channel_archive_existing_config(&path)?;
    channel_save_config_at(&path, &config)?;

    let mut stdout = String::new();
    if let Some(repo_path) = &add_args.repo_path {
        channel_save_repo_gitignore(repo_path)?;
        let _ = writeln!(stdout, "  \x1b[36m📁\x1b[0m repo mode — wrote {}/.claude/channel.json", repo_path.display());
    }
    let _ = writeln!(stdout, "  \x1b[32m✅\x1b[0m channel added: {} → {}", add_args.oracle, add_args.plugin_id);
    channel_push_added_env(&mut stdout, &plugin);
    channel_push_added_token(&mut stdout, &config);
    let _ = writeln!(stdout, "     next: \x1b[36mmaw wake {}\x1b[0m (channels auto-injected)", add_args.oracle);
    Ok(stdout)
}

fn channel_rm(argv: &[String]) -> Result<String, (i32, String)> {
    let (oracle, plugin) = channel_parse_rm(argv)?;
    let Some(mut config) = channel_load_oracle_config(&oracle) else {
        return Ok(format!("  \x1b[90mno channels for {oracle}\x1b[0m\n"));
    };
    if config.plugins.is_empty() { return Ok(format!("  \x1b[90mno channels for {oracle}\x1b[0m\n")); }

    let path = channel_oracle_config_path(&oracle);
    channel_archive_existing_config(&path)?;
    if let Some(plugin_id) = plugin {
        config.plugins.retain(|plugin| plugin.id != plugin_id);
        channel_save_config_at(&path, &config)?;
        Ok(format!("  \x1b[32m✓\x1b[0m removed {plugin_id} from {oracle}\n"))
    } else {
        config.plugins.clear();
        channel_save_config_at(&path, &config)?;
        Ok(format!("  \x1b[32m✓\x1b[0m removed all channels from {oracle}\n"))
    }
}

