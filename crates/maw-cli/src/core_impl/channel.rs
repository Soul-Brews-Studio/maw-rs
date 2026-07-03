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

fn channel_parse_add(argv: &[String]) -> Result<ChannelAddArgs, (i32, String)> {
    if argv.len() < 2 {
        return Err((1, "usage: maw channel add <oracle> <plugin-id>".to_owned()));
    }
    let oracle = channel_validate_name("oracle", &argv[0])?;
    let plugin_id = channel_expand_plugin_id(&argv[1])?;
    let mut repo_path = None;
    let mut env = std::collections::BTreeMap::new();
    let mut pass_key = None;
    let mut index = 2;
    while index < argv.len() {
        match argv[index].as_str() {
            "--repo" => {
                let value = channel_take_flag_value(argv, index, "--repo")?;
                repo_path = Some(channel_validate_repo_path(value)?);
                index += 2;
            }
            "--env" => {
                let value = channel_take_flag_value(argv, index, "--env")?;
                let (key, env_value) = channel_validate_env_assignment(value)?;
                env.insert(key, env_value);
                index += 2;
            }
            "--pass" => {
                let value = channel_take_flag_value(argv, index, "--pass")?;
                pass_key = Some(channel_validate_pass_key(value)?);
                index += 2;
            }
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            other if other.starts_with('-') => return Err((2, format!("channel add: unknown flag {other}"))),
            other => return Err((2, format!("channel add: unexpected argument {other}"))),
        }
    }
    Ok(ChannelAddArgs { oracle, plugin_id, repo_path, env, pass_key })
}

fn channel_parse_rm(argv: &[String]) -> Result<(String, Option<String>), (i32, String)> {
    match argv {
        [] => Err((1, "usage: maw channel rm <oracle> [plugin-id]".to_owned())),
        [oracle] => Ok((channel_validate_name("oracle", oracle)?, None)),
        [oracle, plugin] => Ok((channel_validate_name("oracle", oracle)?, Some(channel_expand_plugin_id(plugin)?))),
        _ => Err((2, "channel rm accepts oracle and optional plugin only".to_owned())),
    }
}

fn channel_new_plugin(args: &ChannelAddArgs) -> ChannelPlugin {
    let mut env = args.env.clone();
    if args.plugin_id.contains("discord") && !env.contains_key("DISCORD_STATE_DIR") {
        let state_dir = if args.repo_path.is_some() { ".claude/channel-state".to_owned() } else { format!("~/.claude/channels/{}", args.oracle) };
        env.insert("DISCORD_STATE_DIR".to_owned(), state_dir);
    }
    ChannelPlugin { id: args.plugin_id.clone(), env: (!env.is_empty()).then_some(env), ..ChannelPlugin::default() }
}

fn channel_push_added_env(stdout: &mut String, plugin: &ChannelPlugin) {
    use std::fmt::Write as _;

    if let Some(env) = &plugin.env {
        for (key, value) in env {
            let value = channel_display_env_value(key, value);
            let _ = writeln!(stdout, "     env: {key}={value}");
        }
    }
}

fn channel_push_added_token(stdout: &mut String, config: &ChannelConfig) {
    use std::fmt::Write as _;

    if let Some(token_source) = &config.token_source {
        let token_source = channel_display_token_source(token_source);
        let _ = writeln!(stdout, "     token: {token_source}");
    }
}

fn channel_expand_plugin_id(value: &str) -> Result<String, (i32, String)> {
    if value.starts_with("github:") {
        return Err((1, "channel add: github providers are handled by the setup slice".to_owned()));
    }
    channel_validate_plugin_id(value)?;
    if value.contains(':') || value.contains('@') { Ok(value.to_owned()) } else { Ok(format!("plugin:{value}@claude-plugins-official")) }
}

fn channel_validate_plugin_id(value: &str) -> Result<(), (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains("..")
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err((2, "channel: invalid plugin".to_owned()));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '@')) {
        return Err((2, "channel: invalid plugin".to_owned()));
    }
    Ok(())
}

fn channel_take_flag_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, (i32, String)> {
    argv.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (2, format!("channel add: missing {flag} value")))
}

fn channel_validate_env_assignment(value: &str) -> Result<(String, String), (i32, String)> {
    let Some((key, env_value)) = value.split_once('=') else {
        return Err((2, "channel add: --env must be KEY=VALUE".to_owned()));
    };
    if key.is_empty()
        || key.starts_with('-')
        || key.chars().any(char::is_control)
        || !key.chars().all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
    {
        return Err((2, "channel: invalid env key".to_owned()));
    }
    if env_value.chars().any(char::is_control) { return Err((2, "channel: invalid env value".to_owned())); }
    Ok((key.to_owned(), env_value.to_owned()))
}

fn channel_validate_pass_key(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains("..")
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err((2, "channel: invalid pass key".to_owned()));
    }
    Ok(value.to_owned())
}

fn channel_validate_repo_path(value: &str) -> Result<std::path::PathBuf, (i32, String)> {
    let path = std::path::Path::new(value);
    if value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err((2, "channel: invalid repo path".to_owned()));
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => return Err((2, "channel: invalid repo path".to_owned())),
            std::path::Component::Normal(name) if name.to_string_lossy().starts_with('-') => {
                return Err((2, "channel: invalid repo path".to_owned()));
            }
            _ => {}
        }
    }
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        std::env::current_dir().map(|cwd| cwd.join(path)).map_err(|error| (1, format!("channel: cannot resolve repo path: {error}")))
    }
}



fn channel_migrate(input: &[String]) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let migrate_args = channel_parse_migrate(input)?;
    let stems = if migrate_args.targets.is_empty() {
        channel_list_all_configs().into_iter().map(|(oracle, _)| oracle).collect::<Vec<_>>()
    } else {
        migrate_args.targets.clone()
    };
    if stems.is_empty() { return Ok("  no oracles with global channel config to migrate\n".to_owned()); }

    let mut counts = ChannelMigrateCounts::default();
    let mut stdout = String::new();
    for stem in stems {
        channel_migrate_one(&stem, &migrate_args, &mut counts, &mut stdout)?;
    }
    let _ = writeln!(stdout, "\n  {} migrated, {} skipped, {} failed", counts.migrated, counts.skipped, counts.failed);
    if counts.migrated > 0 && !migrate_args.remove_global && !migrate_args.dry_run {
        stdout.push_str("  tip: re-run with --remove-global to delete the global config copies.\n");
    }
    Ok(stdout)
}

fn channel_parse_migrate(argv: &[String]) -> Result<ChannelMigrateArgs, (i32, String)> {
    let mut to_repo = false;
    let mut dry_run = false;
    let mut remove_global = false;
    let mut targets = Vec::new();
    for arg in argv {
        match arg.as_str() {
            "--to-repo" => to_repo = true,
            "--dry-run" => dry_run = true,
            "--remove-global" => remove_global = true,
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("channel migrate: unknown flag {value}"))),
            value => targets.push(channel_validate_name("oracle", value)?),
        }
    }
    if !to_repo { return Err((1, channel_migrate_usage())); }
    Ok(ChannelMigrateArgs { targets, dry_run, remove_global })
}

fn channel_migrate_usage() -> String {
    "usage: maw channel migrate --to-repo [oracle...] [--dry-run] [--remove-global]\n  copies global ~/.claude/channels/<oracle>/config.json into\n  <repo>/.claude/channel.json so config travels with the repo (#1195).\n\n  no [oracle...] args = migrate every oracle with global config.\n  --dry-run            = show what would happen, no writes.\n  --remove-global      = delete the global config after a successful copy.".to_owned()
}

fn channel_migrate_one(stem: &str, args: &ChannelMigrateArgs, counts: &mut ChannelMigrateCounts, stdout: &mut String) -> Result<(), (i32, String)> {
    use std::fmt::Write as _;

    let Some(global) = channel_load_oracle_config(stem) else {
        let _ = writeln!(stdout, "  \x1b[90m·\x1b[0m {stem}: no global config — skip");
        counts.skipped += 1;
        return Ok(());
    };
    let Some(repo_path) = channel_resolve_repo_for_stem(stem) else {
        let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m {stem}: no local repo (tried ghq for '{stem}' and '-oracle' variants) — skip");
        counts.failed += 1;
        return Ok(());
    };
    let repo_config = channel_repo_config_path(&repo_path);
    if channel_load_config_at(&repo_config).is_some() {
        let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m {stem}: {}/.claude/channel.json already exists — skip (delete it first)", repo_path.display());
        counts.skipped += 1;
        return Ok(());
    }
    if args.dry_run {
        let _ = writeln!(stdout, "  \x1b[36m·\x1b[0m DRY-RUN {stem}: would write {}/.claude/channel.json ({} plugin(s))", repo_path.display(), global.plugins.len());
        counts.migrated += 1;
        return Ok(());
    }

    channel_save_config_at(&repo_config, &global)?;
    channel_save_repo_gitignore(&repo_path)?;
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {stem}: → {}/.claude/channel.json", repo_path.display());
    if args.remove_global { channel_remove_global_after_copy(stem, stdout)?; }
    counts.migrated += 1;
    Ok(())
}

fn channel_resolve_repo_for_stem(stem: &str) -> Option<std::path::PathBuf> {
    let candidates = channel_repo_candidates(stem);
    if let Some(root) = std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_ROOT") {
        let root = std::path::PathBuf::from(root);
        return candidates.into_iter().map(|candidate| root.join(candidate)).find(|path| path.exists());
    }
    let output = std::process::Command::new("ghq").arg("list").arg("--full-path").output().ok()?;
    if !output.status.success() { return None; }
    let listing = String::from_utf8_lossy(&output.stdout);
    candidates.into_iter().find_map(|candidate| channel_match_repo_listing(&listing, &candidate))
}

fn channel_repo_candidates(stem: &str) -> Vec<String> {
    let alternate = stem.strip_suffix("-oracle").map_or_else(|| format!("{stem}-oracle"), str::to_owned);
    vec![stem.to_owned(), alternate]
}

fn channel_match_repo_listing(listing: &str, candidate: &str) -> Option<std::path::PathBuf> {
    let suffix = format!("/{candidate}");
    listing.lines().map(str::trim).filter(|line| !line.is_empty()).find_map(|line| {
        if line.ends_with(&suffix) || line.ends_with(candidate) { Some(std::path::PathBuf::from(line)) } else { None }
    })
}

fn channel_remove_global_after_copy(stem: &str, stdout: &mut String) -> Result<(), (i32, String)> {
    use std::fmt::Write as _;

    let config_path = channel_oracle_config_path(stem);
    channel_archive_existing_config(&config_path)?;
    match std::fs::remove_file(&config_path) {
        Ok(()) => {
            let dir = channel_state_dir(stem);
            let _ = std::fs::remove_dir(&dir);
            stdout.push_str("    \x1b[90m✓ removed global config\x1b[0m\n");
        }
        Err(error) => {
            let _ = writeln!(stdout, "    \x1b[33m⚠ failed to remove global: {error}\x1b[0m");
        }
    }
    Ok(())
}

fn channel_setup(input: &[String]) -> Result<String, (i32, String)> {
    let setup_args = channel_parse_setup(input)?;
    match &setup_args.provider {
        ChannelSetupProvider::Github(_) => channel_setup_github(&setup_args),
        _ => channel_setup_official(&setup_args),
    }
}

fn channel_parse_setup(argv: &[String]) -> Result<ChannelSetupArgs, (i32, String)> {
    if argv.len() < 2 {
        return Err((1, "usage: maw channel setup <oracle> <provider>".to_owned()));
    }
    let oracle = channel_validate_name("oracle", &argv[0])?;
    let provider = channel_validate_setup_provider(&argv[1])?;
    let mut pass_key = None;
    let mut guild_id = None;
    let mut env = std::collections::BTreeMap::new();
    let mut index = 2;
    while index < argv.len() {
        match argv[index].as_str() {
            "--pass" => {
                let value = channel_take_setup_flag_value(argv, index, "--pass")?;
                pass_key = Some(channel_validate_pass_key(value)?);
                index += 2;
            }
            "--guild" => {
                let value = channel_take_setup_flag_value(argv, index, "--guild")?;
                guild_id = Some(channel_validate_snowflake(value)?);
                index += 2;
            }
            "--env" => {
                let value = channel_take_setup_flag_value(argv, index, "--env")?;
                let (key, env_value) = channel_validate_env_assignment(value)?;
                env.insert(key, env_value);
                index += 2;
            }
            "--no-interactive" => index += 1,
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            other if other.starts_with('-') => return Err((2, format!("channel setup: unknown flag {other}"))),
            other => return Err((2, format!("channel setup: unexpected argument {other}"))),
        }
    }
    Ok(ChannelSetupArgs { oracle, provider, pass_key, guild_id, env })
}

fn channel_validate_setup_provider(value: &str) -> Result<ChannelSetupProvider, (i32, String)> {
    match value {
        "discord" => Ok(ChannelSetupProvider::Discord),
        "telegram" => Ok(ChannelSetupProvider::Telegram),
        "imessage" => Ok(ChannelSetupProvider::Imessage),
        provider if provider.starts_with("github:") => {
            let repo = provider.trim_start_matches("github:");
            channel_validate_github_repo(repo)?;
            Ok(ChannelSetupProvider::Github(repo.to_owned()))
        }
        _ => Err((2, format!("channel setup: unknown provider {value}"))),
    }
}

fn channel_validate_github_repo(value: &str) -> Result<(), (i32, String)> {
    let Some((org, repo)) = value.split_once('/') else {
        return Err((2, "channel setup: invalid github provider".to_owned()));
    };
    if org.is_empty() || repo.is_empty() || value.contains("..") || value.contains('\\') {
        return Err((2, "channel setup: invalid github provider".to_owned()));
    }
    for part in [org, repo] {
        if part.starts_with('-') || part.chars().any(char::is_control) {
            return Err((2, "channel setup: invalid github provider".to_owned()));
        }
        if !part.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
            return Err((2, "channel setup: invalid github provider".to_owned()));
        }
    }
    Ok(())
}

fn channel_take_setup_flag_value<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, (i32, String)> {
    argv.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| (2, format!("channel setup: missing {flag} value")))
}

fn channel_validate_snowflake(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.len() > 32
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || !value.chars().all(|ch| ch.is_ascii_digit())
    {
        return Err((2, "channel setup: invalid guild snowflake".to_owned()));
    }
    Ok(value.to_owned())
}

trait ChannelGithubRunner {
    fn ghq_root(&self) -> Result<std::path::PathBuf, (i32, String)>;
    fn repo_exists(&self, path: &std::path::Path) -> bool;
    fn ghq_get(&self, repo: &str, url: &str, root: &std::path::Path) -> Result<(), (i32, String)>;
    fn file_exists(&self, path: &std::path::Path) -> bool;
    fn read_to_string(&self, path: &std::path::Path) -> Result<Option<String>, (i32, String)>;
    fn bun_install_stub(&self, repo: &std::path::Path) -> Result<(), (i32, String)>;
}

struct ChannelSystemGithubRunner;

impl ChannelGithubRunner for ChannelSystemGithubRunner {
    fn ghq_root(&self) -> Result<std::path::PathBuf, (i32, String)> {
        if let Some(root) = std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_ROOT") {
            return Ok(std::path::PathBuf::from(root));
        }
        let stdout = channel_run_ghq(&["root"], std::time::Duration::from_secs(10))?;
        let root = stdout.trim();
        if root.is_empty() {
            return Err((1, "channel setup: ghq root returned empty path".to_owned()));
        }
        Ok(std::path::PathBuf::from(root))
    }

    fn repo_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn ghq_get(&self, repo: &str, url: &str, root: &std::path::Path) -> Result<(), (i32, String)> {
        if std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_GET_FAIL").is_some() {
            return Err((1, "channel setup: ghq get failed".to_owned()));
        }
        if let Some(log_path) = std::env::var_os("MAW_RS_CHANNEL_FAKE_GHQ_GET_LOG") {
            use std::io::Write as _;

            let path = std::path::PathBuf::from(log_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel setup: fake ghq log dir failed: {error}")))?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| (1, format!("channel setup: fake ghq log failed: {error}")))?;
            writeln!(file, "ghq get {url}").map_err(|error| (1, format!("channel setup: fake ghq log failed: {error}")))?;
            std::fs::create_dir_all(channel_github_repo_path(root, repo))
                .map_err(|error| (1, format!("channel setup: fake ghq create repo failed: {error}")))?;
            return Ok(());
        }
        let _ = repo;
        channel_run_ghq(&["get", url], std::time::Duration::from_secs(30)).map(|_| ())
    }

    fn file_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn read_to_string(&self, path: &std::path::Path) -> Result<Option<String>, (i32, String)> {
        match std::fs::read_to_string(path) {
            Ok(raw) => Ok(Some(raw)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err((1, format!("channel setup: read {} failed: {error}", path.display()))),
        }
    }

    fn bun_install_stub(&self, repo: &std::path::Path) -> Result<(), (i32, String)> {
        if let Some(log_path) = std::env::var_os("MAW_RS_CHANNEL_FAKE_BUN_INSTALL_LOG") {
            use std::io::Write as _;

            let path = std::path::PathBuf::from(log_path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel setup: fake bun log dir failed: {error}")))?;
            }
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(|error| (1, format!("channel setup: fake bun log failed: {error}")))?;
            writeln!(file, "bun install --cwd {}", repo.display()).map_err(|error| (1, format!("channel setup: fake bun log failed: {error}")))?;
        }
        if std::env::var_os("MAW_RS_CHANNEL_FAKE_BUN_INSTALL_FAIL").is_some() {
            return Err((1, "channel setup: bun install failed".to_owned()));
        }
        Ok(())
    }
}

fn channel_setup_github(args: &ChannelSetupArgs) -> Result<String, (i32, String)> {
    channel_setup_github_with_runner(args, &ChannelSystemGithubRunner)
}

fn channel_setup_github_with_runner(args: &ChannelSetupArgs, runner: &dyn ChannelGithubRunner) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let ChannelSetupProvider::Github(repo) = &args.provider else {
        return Err((2, "channel setup: github runner used for non-github provider".to_owned()));
    };
    let root_raw = runner.ghq_root()?;
    let root = channel_canonicalize_ghq_root(&root_raw)?;
    let repo_path = channel_github_repo_path(&root, repo);
    let url = format!("https://github.com/{repo}");
    let mut cloned = false;

    if !runner.repo_exists(&repo_path) {
        runner.ghq_get(repo, &url, &root)?;
        cloned = true;
    }
    let repo_canon = channel_canonicalize_github_repo(&root, &repo_path)?;

    let mcp = channel_github_mcp_config(runner, &repo_canon)?;
    let bun_result = if runner.file_exists(&repo_canon.join("package.json")) {
        Some(runner.bun_install_stub(&repo_canon))
    } else {
        None
    };
    let config_path = channel_oracle_config_path(&args.oracle);
    let mut config = channel_load_config_at(&config_path).unwrap_or_default();
    let mut wrote_config = false;
    let plugin = channel_github_plugin(args, repo, &repo_canon, &mcp);
    if let Some(pass_key) = &args.pass_key {
        config.token_source = Some(format!("pass:{pass_key}"));
    }
    if config.plugins.iter().any(|existing| existing.id == plugin.id) {
        if args.pass_key.is_some() {
            channel_archive_existing_config(&config_path)?;
            channel_save_config_private_at(&config_path, &config)?;
            wrote_config = true;
        }
    } else {
        config.plugins.push(plugin.clone());
        channel_archive_existing_config(&config_path)?;
        channel_save_config_private_at(&config_path, &config)?;
        wrote_config = true;
    }

    let mut stdout = String::new();
    let _ = writeln!(stdout, "\n  \x1b[36;1m🔧 Git Channel Setup for {}\x1b[0m", args.oracle);
    let _ = writeln!(stdout, "  {}", "─".repeat(45));
    let _ = writeln!(stdout, "\n  \x1b[36mStep 1/4: Locate repo\x1b[0m");
    let _ = writeln!(stdout, "  source: github:{repo}");
    let _ = writeln!(stdout, "  ghq root: {}", root.display());
    let _ = writeln!(stdout, "  repo: {}", repo_canon.display());
    if cloned {
        stdout.push_str("  \x1b[32m✓\x1b[0m cloned with ghq get\n");
    } else {
        stdout.push_str("  \x1b[32m✓\x1b[0m repo already present — clone skipped\n");
    }
    let _ = writeln!(stdout, "\n  \x1b[36mStep 2/4: Token + dependencies\x1b[0m");
    if let Some(pass_key) = &args.pass_key {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m token source: pass:{pass_key} (reference only)");
    } else {
        stdout.push_str("  \x1b[90m· token source not configured\x1b[0m\n");
    }
    match bun_result {
        Some(Ok(())) => stdout.push_str("  \x1b[33m⚠\x1b[0m package.json found — bun install stubbed/best-effort; continuing\n"),
        Some(Err((_, message))) => {
            let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m {message}; continuing");
        }
        None => stdout.push_str("  \x1b[90m· no package.json — bun install skipped\x1b[0m\n"),
    }
    let _ = writeln!(stdout, "\n  \x1b[36mStep 3/4: MCP record\x1b[0m");
    let untrusted_label = if mcp.config.untrusted == Some(true) { " (untrusted .mcp.json, validated)" } else { " (native default)" };
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {} → {}{}{}", mcp.plugin_id, mcp.config.command, channel_display_args(&mcp.config.args), untrusted_label);
    stdout.push_str("  \x1b[90m· setup records MCP only; it does not spawn the MCP command\x1b[0m\n");
    let _ = writeln!(stdout, "\n  \x1b[36mStep 4/4: Register config\x1b[0m");
    if wrote_config {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m wrote {} (0600)", channel_tilde_path(&config_path));
    } else {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m already registered: {} → {}", args.oracle, plugin.id);
    }
    stdout.push_str("  \x1b[90m· dev-server spawn is out of scope for setup\x1b[0m\n");
    stdout.push_str("\n  \x1b[32m✅ Git channel setup complete\x1b[0m\n");
    Ok(stdout)
}

#[derive(Debug)]
struct ChannelGithubMcp {
    plugin_id: String,
    config: ChannelMcpConfig,
}

fn channel_github_plugin(args: &ChannelSetupArgs, repo: &str, repo_path: &std::path::Path, mcp: &ChannelGithubMcp) -> ChannelPlugin {
    ChannelPlugin {
        id: mcp.plugin_id.clone(),
        env: (!args.env.is_empty()).then_some(args.env.clone()),
        source: Some(format!("github:{repo}")),
        path: Some(repo_path.display().to_string()),
        mcp: Some(mcp.config.clone()),
        dev: Some(true),
    }
}

fn channel_github_mcp_config(runner: &dyn ChannelGithubRunner, repo_path: &std::path::Path) -> Result<ChannelGithubMcp, (i32, String)> {
    let path = repo_path.join(".mcp.json");
    let Some(raw) = runner.read_to_string(&path)? else {
        return Ok(ChannelGithubMcp {
            plugin_id: "server:relay".to_owned(),
            config: ChannelMcpConfig {
                command: "bun".to_owned(),
                args: vec!["run".to_owned(), "--cwd".to_owned(), repo_path.display().to_string(), "start".to_owned()],
                untrusted: None,
            },
        });
    };
    channel_parse_untrusted_mcp(&raw, repo_path)
}

fn channel_parse_untrusted_mcp(raw: &str, repo_path: &std::path::Path) -> Result<ChannelGithubMcp, (i32, String)> {
    let value: serde_json::Value =
        serde_json::from_str(raw).map_err(|error| (2, format!("channel setup: invalid .mcp.json: {error}")))?;
    let servers = value
        .get("mcpServers")
        .and_then(serde_json::Value::as_object)
        .filter(|object| !object.is_empty())
        .ok_or_else(|| (2, "channel setup: .mcp.json missing mcpServers".to_owned()))?;
    let (name, server) = servers
        .iter()
        .next()
        .ok_or_else(|| (2, "channel setup: .mcp.json missing mcpServers".to_owned()))?;
    let server_name = channel_validate_mcp_server_name(name)?;
    let command = server.get("command").and_then(serde_json::Value::as_str).unwrap_or("bun");
    let command = channel_validate_mcp_command(command)?;
    let args = server
        .get("args")
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| {
                    item.as_str()
                        .ok_or_else(|| (2, "channel setup: invalid .mcp.json args".to_owned()))
                        .and_then(|arg| channel_validate_mcp_arg(arg, repo_path))
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?
        .unwrap_or_default();
    Ok(ChannelGithubMcp {
        plugin_id: format!("server:{server_name}"),
        config: ChannelMcpConfig { command, args, untrusted: Some(true) },
    })
}

fn channel_validate_mcp_server_name(value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'))
    {
        return Err((2, "channel setup: invalid .mcp.json server name".to_owned()));
    }
    Ok(value.to_owned())
}

fn channel_validate_mcp_command(value: &str) -> Result<String, (i32, String)> {
    if channel_mcp_token_invalid(value) || value.contains('/') || value.contains('\\') || value.chars().any(char::is_whitespace) {
        return Err((2, "channel setup: invalid .mcp.json command".to_owned()));
    }
    Ok(value.to_owned())
}

fn channel_validate_mcp_arg(value: &str, repo_path: &std::path::Path) -> Result<String, (i32, String)> {
    let resolved = value.replace("${CLAUDE_PLUGIN_ROOT}", &repo_path.display().to_string());
    if channel_mcp_token_invalid(&resolved) {
        return Err((2, "channel setup: invalid .mcp.json args".to_owned()));
    }
    Ok(resolved)
}

fn channel_mcp_token_invalid(value: &str) -> bool {
    value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.chars().any(char::is_control)
        || value.chars().any(|ch| matches!(ch, ';' | '&' | '|' | '$' | '<' | '>' | '`' | '"' | '\'' | '(' | ')' | '{' | '}'))
}

fn channel_display_args(args: &[String]) -> String {
    if args.is_empty() { String::new() } else { format!(" {}", args.join(" ")) }
}

fn channel_github_repo_path(root: &std::path::Path, repo: &str) -> std::path::PathBuf {
    let (org, name) = repo.split_once('/').unwrap_or((repo, ""));
    root.join("github.com").join(org).join(name)
}

fn channel_canonicalize_ghq_root(path: &std::path::Path) -> Result<std::path::PathBuf, (i32, String)> {
    let root = path.canonicalize().map_err(|error| (1, format!("channel setup: ghq root unavailable: {error}")))?;
    if !root.is_dir() {
        return Err((1, "channel setup: ghq root is not a directory".to_owned()));
    }
    Ok(root)
}

fn channel_canonicalize_github_repo(root: &std::path::Path, repo_path: &std::path::Path) -> Result<std::path::PathBuf, (i32, String)> {
    let repo = repo_path.canonicalize().map_err(|error| (1, format!("channel setup: cloned repo missing: {error}")))?;
    if repo == root || !repo.starts_with(root) {
        return Err((1, "channel setup: cloned repo escaped ghq root".to_owned()));
    }
    Ok(repo)
}

fn channel_run_ghq(args: &[&str], timeout: std::time::Duration) -> Result<String, (i32, String)> {
    use std::io::Read as _;

    let mut child = std::process::Command::new("ghq")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|error| (1, format!("channel setup: ghq unavailable: {error}")))?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(|error| (1, format!("channel setup: ghq wait failed: {error}")))? {
            let mut stdout = String::new();
            if let Some(mut pipe) = child.stdout.take() {
                pipe.read_to_string(&mut stdout).map_err(|error| (1, format!("channel setup: ghq stdout failed: {error}")))?;
            }
            if !status.success() {
                return Err((1, "channel setup: ghq command failed".to_owned()));
            }
            return Ok(stdout);
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Err((1, "channel setup: ghq command timed out".to_owned()));
        }
        std::thread::sleep(std::time::Duration::from_millis(25));
    }
}

fn channel_setup_official(args: &ChannelSetupArgs) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let provider = args.provider.name();
    let plugin_id = args.provider.plugin_id();
    let total = if matches!(args.provider, ChannelSetupProvider::Imessage) { 4 } else { 7 };
    let mut stdout = String::new();
    let _ = writeln!(stdout, "\n  \x1b[36;1m🔧 {provider} Channel Setup for {}\x1b[0m", args.oracle);
    let _ = writeln!(stdout, "  {}", "─".repeat(45));

    channel_push_setup_step(&mut stdout, 1, total, "Plugin check");
    if channel_is_plugin_installed(provider) {
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {plugin_id} installed");
    } else {
        let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m {plugin_id} not installed");
        let _ = writeln!(stdout, "  \x1b[90mrun: /plugin install {provider}@claude-plugins-official\x1b[0m");
        return Ok(stdout);
    }

    if matches!(args.provider, ChannelSetupProvider::Imessage) {
        return channel_setup_imessage(args, stdout, total, &plugin_id);
    }

    let token = channel_setup_token(args, &mut stdout, total)?;
    let state_dir = channel_state_dir(&args.oracle);
    channel_push_setup_step(&mut stdout, 3, total, "State directory");
    channel_create_private_dir(&state_dir)?;
    let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m {}/", channel_tilde_path(&state_dir));
    if args.pass_key.is_none() { channel_rewrite_existing_env(provider, &state_dir, &token, &mut stdout)?; }

    if matches!(args.provider, ChannelSetupProvider::Discord) {
        channel_setup_discord_guild(args, &token, &mut stdout, total);
    } else {
        channel_push_setup_step(&mut stdout, 4, total, "Config");
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m ready");
    }

    channel_push_setup_step(&mut stdout, 5, total, "Access config + seed");
    channel_seed_access_json(&state_dir, &mut stdout)?;
    channel_setup_register(args, &plugin_id, &mut stdout, total)?;
    channel_push_setup_done(&mut stdout, &args.oracle, provider);
    Ok(stdout)
}

fn channel_setup_imessage(args: &ChannelSetupArgs, mut stdout: String, total: usize, plugin_id: &str) -> Result<String, (i32, String)> {
    channel_push_setup_step(&mut stdout, 2, total, "macOS check");
    if !channel_platform_is_macos() {
        stdout.push_str("  \x1b[31m✗\x1b[0m iMessage requires macOS\n");
        return Ok(stdout);
    }
    stdout.push_str("  \x1b[32m✓\x1b[0m macOS detected\n");
    stdout.push_str("  \x1b[90mℹ Full Disk Access required for Messages.app — grant when prompted\x1b[0m\n");
    channel_push_setup_step(&mut stdout, 3, total, "Register channel");
    let path = channel_oracle_config_path(&args.oracle);
    let mut config = channel_load_config_at(&path).unwrap_or_default();
    if !config.plugins.iter().any(|plugin| plugin.id == plugin_id) {
        config.plugins.push(ChannelPlugin { id: plugin_id.to_owned(), ..ChannelPlugin::default() });
        channel_archive_existing_config(&path)?;
        channel_save_config_at(&path, &config)?;
    }
    stdout.push_str("  \x1b[32m✓\x1b[0m registered\n");
    channel_push_setup_step(&mut stdout, 4, total, "Done!");
    channel_push_setup_done(&mut stdout, &args.oracle, "imessage");
    Ok(stdout)
}

fn channel_setup_token(args: &ChannelSetupArgs, stdout: &mut String, total: usize) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    let provider = args.provider.name();
    channel_push_setup_step(stdout, 2, total, "Bot token");
    if let Some(pass_key) = &args.pass_key {
        match channel_pass_show(pass_key) {
            Ok(token) if !token.is_empty() => {
                let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m token from pass: {pass_key}");
                if provider == "discord" {
                    if let Some(client_id) = channel_extract_client_id(&token) {
                        let _ = writeln!(stdout, "  \x1b[90mclient: {client_id}\x1b[0m");
                    }
                }
                return Ok(token);
            }
            _ => {
                let _ = writeln!(stdout, "  \x1b[31m✗\x1b[0m pass key '{pass_key}' not found");
                let _ = writeln!(stdout, "  \x1b[90mrun: pass insert {pass_key}\x1b[0m");
                return Err((0, stdout.clone()));
            }
        }
    }
    let env_file = channel_state_dir(&args.oracle).join(".env");
    if let Some(token) = channel_read_env_token(provider, &env_file) {
        stdout.push_str("  \x1b[32m✓\x1b[0m token found in .env\n");
        if provider == "discord" {
            if let Some(client_id) = channel_extract_client_id(&token) {
                let _ = writeln!(stdout, "  \x1b[90mclient: {client_id}\x1b[0m");
            }
        }
        return Ok(token);
    }
    let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m no token found");
    let _ = writeln!(stdout, "  \x1b[90mstore with: pass insert {provider}/{}-token\x1b[0m", args.oracle);
    let _ = writeln!(stdout, "  \x1b[90mthen: maw channel setup {} {provider} --pass {provider}/{}-token\x1b[0m", args.oracle, args.oracle);
    Err((0, stdout.clone()))
}

fn channel_setup_discord_guild(args: &ChannelSetupArgs, token: &str, stdout: &mut String, total: usize) {
    use std::fmt::Write as _;

    channel_push_setup_step(stdout, 4, total, "Guild / Server");
    let guilds = channel_discord_guilds(token);
    if guilds.is_empty() {
        stdout.push_str("  \x1b[33m⚠\x1b[0m no guilds found — bot may need to be invited first\n");
        if let Some(client_id) = channel_extract_client_id(token) {
            let _ = writeln!(stdout, "  \x1b[90minvite: https://discord.com/oauth2/authorize?client_id={client_id}&scope=bot&permissions=101376\x1b[0m");
        }
        return;
    }
    for (index, guild) in guilds.iter().enumerate() {
        let selected = if args.guild_id.as_ref().is_some_and(|id| id == &guild.id) { " ←" } else { "" };
        let _ = writeln!(stdout, "    {}. {} ({}){selected}", index + 1, guild.name, guild.id);
    }
    let chosen = args
        .guild_id
        .as_ref()
        .and_then(|id| guilds.iter().find(|guild| &guild.id == id))
        .or_else(|| (args.guild_id.is_none()).then(|| guilds.first()).flatten());
    if let Some(guild) = chosen { let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m guild: {}", guild.name); }
}

fn channel_setup_register(args: &ChannelSetupArgs, plugin_id: &str, stdout: &mut String, total: usize) -> Result<(), (i32, String)> {
    use std::fmt::Write as _;

    channel_push_setup_step(stdout, 6, total, "Register channel");
    let path = channel_oracle_config_path(&args.oracle);
    let mut config = channel_load_config_at(&path).unwrap_or_default();
    let mut env = args.env.clone();
    if matches!(args.provider, ChannelSetupProvider::Discord) {
        env.entry("DISCORD_STATE_DIR".to_owned()).or_insert_with(|| format!("~/.claude/channels/{}", args.oracle));
    }
    let plugin = ChannelPlugin { id: plugin_id.to_owned(), env: (!env.is_empty()).then_some(env), ..ChannelPlugin::default() };
    if let Some(pass_key) = &args.pass_key { config.token_source = Some(format!("pass:{pass_key}")); }
    if config.plugins.iter().any(|existing| existing.id == plugin_id) {
        stdout.push_str("  \x1b[32m✓\x1b[0m already registered\n");
    } else {
        config.plugins.push(plugin);
        channel_archive_existing_config(&path)?;
        channel_save_config_at(&path, &config)?;
        let _ = writeln!(stdout, "  \x1b[32m✓\x1b[0m registered: {} → {plugin_id}", args.oracle);
    }
    Ok(())
}

fn channel_push_setup_step(stdout: &mut String, step: usize, total: usize, label: &str) {
    use std::fmt::Write as _;

    let _ = writeln!(stdout, "\n  \x1b[36mStep {step}/{total}: {label}\x1b[0m");
}

fn channel_push_setup_done(stdout: &mut String, oracle: &str, provider: &str) {
    use std::fmt::Write as _;

    stdout.push_str("\n  \x1b[32m✅ Setup complete!\x1b[0m\n\n");
    stdout.push_str("  Start oracle with channels:\n");
    let _ = writeln!(stdout, "    \x1b[36mmaw wake {oracle}\x1b[0m\n");
    stdout.push_str("  \x1b[90mNat pre-approved — no pairing needed. Bot responds immediately.\x1b[0m\n");
    if provider != "imessage" { stdout.push_str("  \x1b[90mAdd others: /discord:access allow <user-id>\x1b[0m\n"); }
}

fn channel_state_dir(oracle: &str) -> std::path::PathBuf {
    channel_channels_base().join(oracle)
}

fn channel_create_private_dir(path: &std::path::Path) -> Result<(), (i32, String)> {
    std::fs::create_dir_all(path).map_err(|error| (1, format!("channel: create state dir failed: {error}")))?;
    channel_chmod(path, 0o700)
}

fn channel_rewrite_existing_env(provider: &str, state_dir: &std::path::Path, token: &str, stdout: &mut String) -> Result<(), (i32, String)> {
    let token_key = if provider == "discord" { "DISCORD_BOT_TOKEN" } else { "TELEGRAM_BOT_TOKEN" };
    let env_file = state_dir.join(".env");
    if !env_file.exists() { return Ok(()); }
    channel_atomic_write_private(&env_file, &format!("{token_key}={token}\n"), 0o600)?;
    stdout.push_str("  \x1b[32m✓\x1b[0m .env written (0o600)\n");
    Ok(())
}

fn channel_read_env_token(provider: &str, env_file: &std::path::Path) -> Option<String> {
    let token_key = if provider == "discord" { "DISCORD_BOT_TOKEN" } else { "TELEGRAM_BOT_TOKEN" };
    let raw = std::fs::read_to_string(env_file).ok()?;
    for line in raw.lines() {
        let Some((key, value)) = line.split_once('=') else { continue; };
        if key == token_key && !value.trim().is_empty() { return Some(value.trim().to_owned()); }
    }
    None
}

fn channel_pass_show(pass_key: &str) -> Result<String, ()> {
    if let Some(fake) = std::env::var_os("MAW_RS_CHANNEL_FAKE_PASS_TOKEN") {
        return Ok(fake.to_string_lossy().trim().to_owned());
    }
    let output = std::process::Command::new("pass").arg("show").arg(pass_key).output().map_err(|_| ())?;
    if !output.status.success() { return Err(()); }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn channel_discord_guilds(_token: &str) -> Vec<ChannelDiscordGuild> {
    let Ok(raw) = std::env::var("MAW_RS_CHANNEL_FAKE_DISCORD_GUILDS") else { return Vec::new(); };
    raw.split(';')
        .filter_map(|entry| {
            let (id, name) = entry.split_once(':')?;
            Some(ChannelDiscordGuild { id: id.to_owned(), name: name.to_owned() })
        })
        .collect()
}

fn channel_extract_client_id(token: &str) -> Option<String> {
    let first = token.split('.').next()?;
    channel_decode_base64_segment(first).ok().filter(|value| !value.is_empty())
}

fn channel_decode_base64_segment(value: &str) -> Result<String, ()> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut bit_count = 0u8;
    let mut bytes = Vec::new();
    for byte in value.bytes() {
        let normalized = match byte { b'-' => b'+', b'_' => b'/', b'=' => continue, other => other };
        let Some(index) = TABLE.iter().position(|candidate| *candidate == normalized) else { return Err(()); };
        let sextet = u32::try_from(index).map_err(|_| ())?;
        bits = (bits << 6) | sextet;
        bit_count += 6;
        while bit_count >= 8 {
            bit_count -= 8;
            bytes.push(((bits >> bit_count) & 0xff) as u8);
        }
    }
    String::from_utf8(bytes).map_err(|_| ())
}

fn channel_seed_access_json(state_dir: &std::path::Path, stdout: &mut String) -> Result<(), (i32, String)> {
    let access_path = state_dir.join("access.json");
    let seed = channel_access_seed();
    if !access_path.exists() {
        channel_save_access_json(&access_path, &seed)?;
        stdout.push_str("  \x1b[32m✓\x1b[0m access.json seeded (Nat pre-approved, dmPolicy: allowlist)\n");
        stdout.push_str("  \x1b[90mno pairing needed — Nat can DM immediately\x1b[0m\n");
        return Ok(());
    }
    if channel_read_access_json(&access_path).is_some() {
        stdout.push_str("  \x1b[32m✓\x1b[0m existing access.json preserved\n");
        return Ok(());
    }
    channel_archive_existing_config(&access_path)?;
    channel_save_access_json(&access_path, &seed)?;
    stdout.push_str("  \x1b[32m✓\x1b[0m access.json reset + Nat seeded\n");
    Ok(())
}

fn channel_access_seed() -> ChannelAccessConfig {
    ChannelAccessConfig {
        dm_policy: "allowlist".to_owned(),
        allow_from: vec!["691531480689541170".to_owned()],
        groups: serde_json::json!({}),
        pending: serde_json::json!({}),
    }
}

fn channel_read_access_json(path: &std::path::Path) -> Option<serde_json::Value> {
    let value = channel_read_json(path)?;
    let object = value.as_object()?;
    object.get("dmPolicy")?.as_str()?;
    object.get("allowFrom")?.as_array()?;
    Some(value)
}

fn channel_save_access_json(path: &std::path::Path, access: &ChannelAccessConfig) -> Result<(), (i32, String)> {
    let json = serde_json::to_string_pretty(access).map_err(|error| (1, format!("channel: serialize access failed: {error}")))?;
    channel_atomic_write(path, &(json + "\n"))
}

fn channel_atomic_write_private(path: &std::path::Path, contents: &str, mode: u32) -> Result<(), (i32, String)> {
    let parent = path.parent().ok_or_else(|| (1, "channel: private path has no parent".to_owned()))?;
    std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel: create private dir failed: {error}")))?;
    let tmp_path = parent.join(channel_tmp_file_name(path));
    std::fs::write(&tmp_path, contents).map_err(|error| (1, format!("channel: write temp private failed: {error}")))?;
    channel_chmod(&tmp_path, mode)?;
    std::fs::rename(&tmp_path, path).map_err(|error| (1, format!("channel: rename temp private failed: {error}")))?;
    channel_chmod(path, mode)
}

#[cfg(unix)]
fn channel_chmod(path: &std::path::Path, mode: u32) -> Result<(), (i32, String)> {
    use std::os::unix::fs::PermissionsExt as _;

    let permissions = std::fs::Permissions::from_mode(mode);
    std::fs::set_permissions(path, permissions).map_err(|error| (1, format!("channel: chmod failed: {error}")))
}

#[cfg(not(unix))]
fn channel_chmod(_path: &std::path::Path, _mode: u32) -> Result<(), (i32, String)> { Ok(()) }

fn channel_tilde_path(path: &std::path::Path) -> String {
    let home = channel_home();
    path.strip_prefix(&home).map_or_else(|_| path.display().to_string(), |rest| format!("~/{}", rest.display()))
}

fn channel_platform_is_macos() -> bool {
    std::env::var("MAW_RS_CHANNEL_FAKE_PLATFORM").map_or(cfg!(target_os = "macos"), |value| value == "darwin")
}

fn channel_ls(argv: &[String]) -> Result<String, (i32, String)> {
    let (target, json, verbose) = channel_parse_ls(argv)?;
    if json { return Ok(channel_ls_json(target.as_deref())); }
    if let Some(target) = target { return Ok(channel_ls_one(&target, verbose)); }
    Ok(channel_ls_all(verbose))
}

fn channel_parse_ls(argv: &[String]) -> Result<(Option<String>, bool, bool), (i32, String)> {
    let mut target = None;
    let mut json = false;
    let mut verbose = false;
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            "--verbose" | "-v" => verbose = true,
            "--" => return Err((2, "channel: -- separator is not supported".to_owned())),
            value if value.starts_with('-') => return Err((2, format!("channel: unknown ls flag {value}"))),
            value => {
                if target.is_some() { return Err((2, "channel ls accepts at most one oracle".to_owned())); }
                target = Some(channel_validate_name("oracle", value)?);
            }
        }
    }
    Ok((target, json, verbose))
}

fn channel_providers(argv: &[String]) -> Result<String, (i32, String)> {
    use std::fmt::Write as _;

    channel_reject_extra_args("providers", argv)?;
    let providers = channel_get_providers();
    let mut stdout = format!("  \x1b[36;1mChannel Providers\x1b[0m ({} available)\n\n", providers.len());
    stdout.push_str("  Provider        Type       Plugin ID                                     Status\n");
    stdout.push_str("  ─────────────── ────────── ───────────────────────────────────────────── ──────────\n");
    for provider in providers {
        let status = if channel_is_plugin_installed(&provider.short_name) { "\x1b[32m✓ installed\x1b[0m" } else { "\x1b[90mnot installed\x1b[0m" };
        let _ = writeln!(stdout, "  {:<15} {:<10} {:<45} {status}", provider.short_name, provider.kind, provider.plugin_id);
    }
    stdout.push_str("\n  Install: \x1b[36m/plugin install <provider>@claude-plugins-official\x1b[0m\n");
    stdout.push_str("  Custom:  \x1b[36mmaw channel add <oracle> server:<name>\x1b[0m (for .mcp.json servers)\n");
    Ok(stdout)
}

fn channel_test(argv: &[String]) -> Result<String, (i32, String)> {
    let target = channel_parse_test(argv)?;
    let Some(config) = channel_load_oracle_config(&target) else {
        return Err((1, format!("  \x1b[31m✗\x1b[0m no channels for {target}")));
    };
    if config.plugins.is_empty() { return Err((1, format!("  \x1b[31m✗\x1b[0m no channels for {target}"))); }
    let env = channel_effective_env(&config);
    let mut stdout = format!("  \x1b[36;1mChannel Test: {target}\x1b[0m\n\n");
    for plugin in &config.plugins {
        stdout.push_str("  ");
        stdout.push_str(&plugin.id);
        stdout.push('\n');
        for check in channel_checks(plugin, &config, &env) {
            stdout.push_str("    ");
            stdout.push_str(&check);
            stdout.push('\n');
        }
    }
    Ok(stdout)
}

fn channel_parse_test(argv: &[String]) -> Result<String, (i32, String)> {
    match argv {
        [] => Err((1, "  usage: maw channel test <oracle>".to_owned())),
        [target] => channel_validate_name("oracle", target),
        _ => Err((2, "channel test accepts exactly one oracle".to_owned())),
    }
}

fn channel_reject_extra_args(subcommand: &str, argv: &[String]) -> Result<(), (i32, String)> {
    if argv.iter().any(|arg| arg == "--") { return Err((2, "channel: -- separator is not supported".to_owned())); }
    if let Some(arg) = argv.first() { return Err((2, format!("channel {subcommand}: unexpected argument {arg}"))); }
    Ok(())
}

fn channel_validate_name(label: &str, value: &str) -> Result<String, (i32, String)> {
    if value.is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value == "."
        || value == ".."
        || value.contains("..")
        || value.contains('/')
        || value.contains('\\')
        || value.chars().any(char::is_control)
    {
        return Err((2, format!("channel: invalid {label}")));
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.')) {
        return Err((2, format!("channel: invalid {label}")));
    }
    Ok(value.to_owned())
}

fn channel_ls_json(target: Option<&str>) -> String {
    if let Some(target) = target {
        let config = channel_redacted_config(channel_load_oracle_config(target).unwrap_or_default());
        let mut value = serde_json::to_value(config).expect("channel config json");
        if let serde_json::Value::Object(map) = &mut value { map.insert("oracle".to_owned(), serde_json::json!(target)); }
        return format!("{}\n", serde_json::to_string_pretty(&value).expect("json"));
    }
    let oracles = channel_list_all_configs()
        .into_iter()
        .map(|(oracle, config)| serde_json::json!({ "oracle": oracle, "plugins": channel_redacted_config(config).plugins }))
        .collect::<Vec<_>>();
    format!("{}\n", serde_json::to_string_pretty(&serde_json::json!({ "oracles": oracles })).expect("json"))
}

fn channel_redacted_config(mut config: ChannelConfig) -> ChannelConfig {
    for plugin in &mut config.plugins {
        if let Some(env) = &mut plugin.env {
            for (key, value) in env.iter_mut() {
                if channel_is_secret_key(key) { "<redacted>".clone_into(value); }
            }
        }
    }
    if let Some(token_source) = &config.token_source {
        config.token_source = Some(channel_display_token_source(token_source));
    }
    config
}

fn channel_ls_one(target: &str, verbose: bool) -> String {
    let Some(config) = channel_load_oracle_config(target) else { return format!("  \x1b[90mno channels for {target}\x1b[0m\n"); };
    if config.plugins.is_empty() { return format!("  \x1b[90mno channels for {target}\x1b[0m\n"); }
    let mut stdout = format!("  \x1b[36;1m{target}\x1b[0m\n");
    for plugin in &config.plugins {
        stdout.push_str("    ");
        stdout.push_str(&plugin.id);
        stdout.push('\n');
        channel_push_plugin_env(&mut stdout, plugin, 6);
    }
    channel_push_token_source(&mut stdout, &config, 4);
    if verbose { channel_push_permission(&mut stdout, &config, 4); }
    stdout
}

fn channel_ls_all(verbose: bool) -> String {
    use std::fmt::Write as _;

    let all = channel_list_all_configs();
    if all.is_empty() {
        return "  \x1b[90mno oracles have channels configured\x1b[0m\n  add one: \x1b[36mmaw channel add <oracle> discord\x1b[0m\n".to_owned();
    }
    let mut stdout = format!("  \x1b[36;1mOracle{}Channel\x1b[0m\n", " ".repeat(24));
    let _ = writeln!(stdout, "  {}  {}", "─".repeat(30), "─".repeat(45));
    for (oracle, config) in &all {
        for plugin in &config.plugins {
            let _ = writeln!(stdout, "  {oracle:<30}  {}", plugin.id);
            if verbose {
                channel_push_plugin_env(&mut stdout, plugin, 32);
                channel_push_permission(&mut stdout, config, 32);
                channel_push_token_source(&mut stdout, config, 32);
            }
        }
    }
    let _ = writeln!(stdout, "\n  {} oracle(s) with channels", all.len());
    stdout
}

fn channel_push_plugin_env(stdout: &mut String, plugin: &ChannelPlugin, indent: usize) {
    use std::fmt::Write as _;

    if let Some(env) = &plugin.env {
        for (key, value) in env {
            let value = channel_display_env_value(key, value);
            let _ = writeln!(stdout, "{}\x1b[90m{key}={value}\x1b[0m", " ".repeat(indent));
        }
    }
}

fn channel_push_permission(stdout: &mut String, config: &ChannelConfig, indent: usize) {
    use std::fmt::Write as _;

    if let Some(mode) = &config.permission_mode {
        let _ = writeln!(stdout, "{}\x1b[90mpermissionMode: {mode}\x1b[0m", " ".repeat(indent));
    }
}

fn channel_push_token_source(stdout: &mut String, config: &ChannelConfig, indent: usize) {
    use std::fmt::Write as _;

    if let Some(token_source) = &config.token_source {
        let token_source = channel_display_token_source(token_source);
        let _ = writeln!(stdout, "{}\x1b[90mtoken: {token_source}\x1b[0m", " ".repeat(indent));
    }
}

fn channel_display_env_value(key: &str, value: &str) -> String {
    if channel_is_secret_key(key) { "<redacted>".to_owned() } else { value.to_owned() }
}

fn channel_display_token_source(value: &str) -> String {
    if matches!(value.split_once(':'), Some(("pass" | "env" | "keychain", _))) { value.to_owned() } else { "<redacted>".to_owned() }
}

fn channel_is_secret_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    ["TOKEN", "SECRET", "PASSWORD", "PASS", "PRIVATE_KEY"].iter().any(|needle| upper.contains(needle))
}

fn channel_get_providers() -> Vec<ChannelProvider> {
    let mut providers = vec![
        channel_provider("discord", "plugin:discord@claude-plugins-official", "chat"),
        channel_provider("telegram", "plugin:telegram@claude-plugins-official", "chat"),
        channel_provider("imessage", "plugin:imessage@claude-plugins-official", "chat"),
        channel_provider("fakechat", "plugin:fakechat@claude-plugins-official", "chat"),
    ];
    providers.extend(channel_custom_providers());
    providers
}

fn channel_provider(short_name: &str, plugin_id: &str, kind: &'static str) -> ChannelProvider {
    ChannelProvider { short_name: short_name.to_owned(), plugin_id: plugin_id.to_owned(), kind }
}

fn channel_custom_providers() -> Vec<ChannelProvider> {
    let mut providers = Vec::new();
    for path in [std::env::current_dir().ok().map(|cwd| cwd.join(".mcp.json")), Some(channel_home().join(".claude.json"))].into_iter().flatten() {
        let Some(json) = channel_read_json(&path) else { continue; };
        let Some(servers) = json.get("mcpServers").and_then(serde_json::Value::as_object) else { continue; };
        for name in servers.keys() {
            if channel_validate_name("server", name).is_ok() { providers.push(channel_provider(name, &format!("server:{name}"), "custom")); }
        }
    }
    providers
}

fn channel_is_plugin_installed(short_name: &str) -> bool {
    channel_home().join(".claude/plugins/cache/claude-plugins-official").join(short_name).exists()
}

fn channel_checks(plugin: &ChannelPlugin, config: &ChannelConfig, env: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    let mut checks = Vec::new();
    if plugin.id.starts_with("plugin:") {
        let name = plugin.id.split(':').nth(1).and_then(|value| value.split('@').next()).unwrap_or_default();
        if channel_is_plugin_installed(name) { checks.push("\x1b[32m✓ plugin installed\x1b[0m".to_owned()); } else { checks.push("\x1b[31m✗ plugin not installed\x1b[0m".to_owned()); }
    }
    if let Some(dir) = env.get("DISCORD_STATE_DIR").or_else(|| plugin.env.as_ref().and_then(|map| map.get("DISCORD_STATE_DIR"))) {
        if std::path::Path::new(dir).exists() { checks.push("\x1b[32m✓ state dir exists\x1b[0m".to_owned()); } else { checks.push(format!("\x1b[31m✗ state dir missing: {dir}\x1b[0m")); }
    }
    if env.contains_key("DISCORD_BOT_TOKEN") || env.contains_key("TELEGRAM_BOT_TOKEN") { checks.push("\x1b[32m✓ token available\x1b[0m".to_owned()); } else if let Some(token_source) = &config.token_source { checks.push(format!("\x1b[32m✓ token source: {token_source}\x1b[0m")); } else { checks.push("\x1b[33m⚠ no token configured\x1b[0m".to_owned()); }
    checks
}

fn channel_effective_env(config: &ChannelConfig) -> std::collections::BTreeMap<String, String> {
    let mut env = std::collections::BTreeMap::new();
    for plugin in &config.plugins {
        if let Some(plugin_env) = &plugin.env { env.extend(plugin_env.clone()); }
    }
    let home = channel_home();
    for value in env.values_mut() {
        if let Some(stripped) = value.strip_prefix("~/") { *value = home.join(stripped).to_string_lossy().into_owned(); }
    }
    env
}


fn channel_config_path_for_add(oracle: &str, repo_path: Option<&std::path::Path>) -> std::path::PathBuf {
    repo_path.map_or_else(|| channel_oracle_config_path(oracle), channel_repo_config_path)
}

fn channel_oracle_config_path(oracle: &str) -> std::path::PathBuf {
    channel_channels_base().join(oracle).join("config.json")
}

fn channel_repo_config_path(repo_path: &std::path::Path) -> std::path::PathBuf {
    repo_path.join(".claude").join("channel.json")
}

fn channel_load_config_at(path: &std::path::Path) -> Option<ChannelConfig> {
    channel_read_json(path).and_then(|value| serde_json::from_value(value).ok())
}

fn channel_save_config_at(path: &std::path::Path, config: &ChannelConfig) -> Result<(), (i32, String)> {
    let json = serde_json::to_string_pretty(config).map_err(|error| (1, format!("channel: serialize config failed: {error}")))?;
    channel_atomic_write(path, &(json + "\n"))
}

fn channel_save_config_private_at(path: &std::path::Path, config: &ChannelConfig) -> Result<(), (i32, String)> {
    let json = serde_json::to_string_pretty(config).map_err(|error| (1, format!("channel: serialize config failed: {error}")))?;
    channel_atomic_write_private(path, &(json + "\n"), 0o600)
}

fn channel_atomic_write(path: &std::path::Path, contents: &str) -> Result<(), (i32, String)> {
    let parent = path.parent().ok_or_else(|| (1, "channel: config path has no parent".to_owned()))?;
    std::fs::create_dir_all(parent).map_err(|error| (1, format!("channel: create config dir failed: {error}")))?;
    let tmp_path = parent.join(channel_tmp_file_name(path));
    std::fs::write(&tmp_path, contents).map_err(|error| (1, format!("channel: write temp config failed: {error}")))?;
    std::fs::rename(&tmp_path, path).map_err(|error| (1, format!("channel: rename temp config failed: {error}")))
}

fn channel_tmp_file_name(path: &std::path::Path) -> String {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("config.json");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!(".{name}.tmp.{}.{}", std::process::id(), nanos)
}

fn channel_archive_existing_config(path: &std::path::Path) -> Result<(), (i32, String)> {
    let Ok(contents) = std::fs::read_to_string(path) else { return Ok(()); };
    let parent = path.parent().ok_or_else(|| (1, "channel: config path has no parent".to_owned()))?;
    let archive_dir = parent.join("archive");
    let archive_name = channel_archive_file_name(path);
    channel_atomic_write(&archive_dir.join(archive_name), &contents)
}

fn channel_archive_file_name(path: &std::path::Path) -> String {
    let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("config.json");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    format!("{name}.{}.{}.bak", std::process::id(), nanos)
}

fn channel_save_repo_gitignore(repo_path: &std::path::Path) -> Result<(), (i32, String)> {
    let gitignore = repo_path.join(".gitignore");
    let entry = ".claude/.env";
    let existing = std::fs::read_to_string(&gitignore).unwrap_or_default();
    if existing.lines().any(|line| line.trim() == entry) { return Ok(()); }
    let mut next = existing;
    if !next.is_empty() && !next.ends_with('\n') { next.push('\n'); }
    next.push_str("\n# Channel bot token — never commit\n.claude/.env\n");
    channel_atomic_write(&gitignore, &next)
}

fn channel_list_all_configs() -> Vec<(String, ChannelConfig)> {
    let base = channel_channels_base();
    let Ok(entries) = std::fs::read_dir(base) else { return Vec::new(); };
    let mut configs = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) { continue; }
        let oracle = entry.file_name().to_string_lossy().into_owned();
        if channel_validate_name("oracle", &oracle).is_err() { continue; }
        if let Some(config) = channel_load_oracle_config(&oracle) {
            if !config.plugins.is_empty() { configs.push((oracle, config)); }
        }
    }
    configs.sort_by(|left, right| left.0.cmp(&right.0));
    configs
}

fn channel_load_oracle_config(oracle: &str) -> Option<ChannelConfig> {
    let path = channel_channels_base().join(oracle).join("config.json");
    channel_read_json(&path).and_then(|value| serde_json::from_value(value).ok())
}

fn channel_read_json(path: &std::path::Path) -> Option<serde_json::Value> {
    let raw = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn channel_channels_base() -> std::path::PathBuf { channel_home().join(".claude").join("channels") }

fn channel_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
}

#[cfg(test)]
mod channel_pr301_tests {
    use super::*;

    #[test]
    fn channel_plugin_schema_accepts_github_setup_fields() {
        let raw = r#"{
          "plugins": [
            {
              "id": "server:relay",
              "source": "github:ARRA-01/claude-channel-relay",
              "path": "/tmp/ghq/github.com/ARRA-01/claude-channel-relay",
              "mcp": { "command": "node", "args": ["server.js"] },
              "dev": true,
              "env": { "CHANNEL_MODE": "dev" }
            }
          ],
          "token_source": "pass:github/hermes-token"
        }"#;
        let config: ChannelConfig = serde_json::from_str(raw).expect("schema parses");
        let plugin = config.plugins.first().expect("plugin");
        assert_eq!(plugin.source.as_deref(), Some("github:ARRA-01/claude-channel-relay"));
        assert_eq!(plugin.path.as_deref(), Some("/tmp/ghq/github.com/ARRA-01/claude-channel-relay"));
        assert_eq!(plugin.dev, Some(true));
        assert_eq!(
            plugin.mcp.as_ref().expect("mcp"),
            &ChannelMcpConfig { command: "node".to_owned(), args: vec!["server.js".to_owned()], untrusted: None }
        );
        let serialized = serde_json::to_string(&config).expect("serialize");
        assert!(serialized.contains("\"source\""));
        assert!(serialized.contains("\"path\""));
        assert!(serialized.contains("\"mcp\""));
        assert!(serialized.contains("\"dev\""));
    }

    #[derive(Debug)]
    struct FakeGithubRunner {
        root: std::path::PathBuf,
        bun_fail: bool,
        calls: std::cell::RefCell<Vec<String>>,
    }

    impl FakeGithubRunner {
        fn new(root: std::path::PathBuf) -> Self {
            Self { root, bun_fail: false, calls: std::cell::RefCell::new(Vec::new()) }
        }

        fn with_bun_fail(root: std::path::PathBuf) -> Self {
            Self { root, bun_fail: true, calls: std::cell::RefCell::new(Vec::new()) }
        }
    }

    impl ChannelGithubRunner for FakeGithubRunner {
        fn ghq_root(&self) -> Result<std::path::PathBuf, (i32, String)> {
            self.calls.borrow_mut().push("ghq root".to_owned());
            Ok(self.root.clone())
        }

        fn repo_exists(&self, path: &std::path::Path) -> bool {
            self.calls.borrow_mut().push(format!("exists {}", path.display()));
            path.exists()
        }

        fn ghq_get(&self, repo: &str, url: &str, root: &std::path::Path) -> Result<(), (i32, String)> {
            self.calls.borrow_mut().push(format!("ghq get {url}"));
            std::fs::create_dir_all(channel_github_repo_path(root, repo)).expect("fake clone");
            Ok(())
        }

        fn file_exists(&self, path: &std::path::Path) -> bool {
            self.calls.borrow_mut().push(format!("file-exists {}", path.display()));
            path.exists()
        }

        fn read_to_string(&self, path: &std::path::Path) -> Result<Option<String>, (i32, String)> {
            self.calls.borrow_mut().push(format!("read {}", path.display()));
            match std::fs::read_to_string(path) {
                Ok(raw) => Ok(Some(raw)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err((1, error.to_string())),
            }
        }

        fn bun_install_stub(&self, repo: &std::path::Path) -> Result<(), (i32, String)> {
            self.calls.borrow_mut().push(format!("bun-stub {}", repo.display()));
            if self.bun_fail {
                Err((1, "channel setup: bun install failed".to_owned()))
            } else {
                Ok(())
            }
        }
    }

    fn temp_path(name: &str) -> std::path::PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("maw-rs-channel-{name}-{stamp}"));
        std::fs::create_dir_all(&path).expect("temp dir");
        path
    }

    fn github_args() -> ChannelSetupArgs {
        let mut env = std::collections::BTreeMap::new();
        env.insert("CHANNEL_MODE".to_owned(), "dev".to_owned());
        ChannelSetupArgs {
            oracle: "relay-oracle".to_owned(),
            provider: ChannelSetupProvider::Github("ARRA-01/claude-channel-relay".to_owned()),
            pass_key: Some("github/relay-token".to_owned()),
            guild_id: None,
            env,
        }
    }

    fn seed_repo(root: &std::path::Path, mcp: Option<&str>, package_json: bool) -> std::path::PathBuf {
        let repo = channel_github_repo_path(root, "ARRA-01/claude-channel-relay");
        std::fs::create_dir_all(&repo).expect("repo");
        if let Some(mcp) = mcp {
            std::fs::write(repo.join(".mcp.json"), mcp).expect("mcp");
        }
        if package_json {
            std::fs::write(repo.join("package.json"), "{}\n").expect("package");
        }
        repo
    }

    #[test]
    fn channel_github_prb_records_untrusted_mcp_without_autospawn_or_token_value() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = super::EnvVarRestore::capture("HOME");
        let home = temp_path("home-record");
        let ghq = temp_path("ghq-record");
        std::env::set_var("HOME", &home);
        let repo = seed_repo(
            &ghq,
            Some(r#"{"mcpServers":{"relay":{"command":"bun","args":["run","start"]}}}"#),
            true,
        );
        let runner = FakeGithubRunner::new(ghq.clone());

        let stdout = channel_setup_github_with_runner(&github_args(), &runner).expect("setup");
        assert!(stdout.contains("setup records MCP only; it does not spawn"));
        assert!(stdout.contains("pass:github/relay-token (reference only)"));
        assert!(!stdout.contains("ghp_"));
        let calls = runner.calls.borrow().join("\n");
        assert!(calls.contains("bun-stub"));
        assert!(!calls.contains("bun run start"), "mcp command must not spawn at setup: {calls}");

        let config = channel_load_config_at(&channel_oracle_config_path("relay-oracle")).expect("config");
        assert_eq!(config.token_source.as_deref(), Some("pass:github/relay-token"));
        assert_eq!(config.plugins.len(), 1);
        let plugin = &config.plugins[0];
        assert_eq!(plugin.id, "server:relay");
        assert_eq!(plugin.source.as_deref(), Some("github:ARRA-01/claude-channel-relay"));
        assert_eq!(plugin.path.as_deref(), Some(repo.canonicalize().expect("repo canon").to_string_lossy().as_ref()));
        assert_eq!(plugin.dev, Some(true));
        let mcp = plugin.mcp.as_ref().expect("mcp");
        assert_eq!(mcp.command, "bun");
        assert_eq!(mcp.args, vec!["run".to_owned(), "start".to_owned()]);
        assert_eq!(mcp.untrusted, Some(true));
    }

    #[test]
    fn channel_github_prb_rejects_untrusted_mcp_leading_dash_before_config_write() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = super::EnvVarRestore::capture("HOME");
        let home = temp_path("home-reject");
        let ghq = temp_path("ghq-reject");
        std::env::set_var("HOME", &home);
        seed_repo(
            &ghq,
            Some(r#"{"mcpServers":{"relay":{"command":"bun","args":["--cwd","/tmp"]}}}"#),
            false,
        );
        let runner = FakeGithubRunner::new(ghq);

        let err = channel_setup_github_with_runner(&github_args(), &runner).expect_err("reject untrusted arg");
        assert_eq!(err.0, 2);
        assert!(err.1.contains("invalid .mcp.json args"));
        assert!(!channel_oracle_config_path("relay-oracle").exists());
        let calls = runner.calls.borrow().join("\n");
        assert!(!calls.contains("bun-stub"));
    }

    #[test]
    fn channel_github_prb_default_mcp_allows_internal_cwd_and_bun_failure_warns_continue() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _home = super::EnvVarRestore::capture("HOME");
        let home = temp_path("home-default");
        let ghq = temp_path("ghq-default");
        std::env::set_var("HOME", &home);
        let repo = seed_repo(&ghq, None, true);
        let runner = FakeGithubRunner::with_bun_fail(ghq);

        let stdout = channel_setup_github_with_runner(&github_args(), &runner).expect("setup");
        assert!(stdout.contains("bun install failed; continuing"));
        let config = channel_load_config_at(&channel_oracle_config_path("relay-oracle")).expect("config");
        let mcp = config.plugins[0].mcp.as_ref().expect("mcp");
        assert_eq!(mcp.untrusted, None);
        assert_eq!(
            mcp.args,
            vec!["run".to_owned(), "--cwd".to_owned(), repo.canonicalize().expect("repo").display().to_string(), "start".to_owned()]
        );
    }
}
