// Showing what is installed, and which tier each verb actually runs at.
//
// A plugin can be present in several forms at once -- shipped wasm, dev source,
// shadowed by a native verb -- so the interesting column is the effective tier,
// not the presence. Compact and table renderings share the tier resolution so
// they cannot disagree.

#[derive(Default)]
struct PluginLsOptions {
    verbose: bool,
    tiers: Vec<PluginTier>,
    api_only: bool,
}

fn parse_plugin_ls_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    // Default runtime_version is the ABI-derived host_abi_version()
    // (overridable with --runtime-version).
    let mut options = DiscoverPackagesOptions::default();
    let mut ls_options = PluginLsOptions::default();
    let mut scan_dirs = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "-v" | "--verbose" => ls_options.verbose = true,
            "--core" => ls_options.tiers.push(PluginTier::Core),
            "--standard" => ls_options.tiers.push(PluginTier::Standard),
            "--extra" => ls_options.tiers.push(PluginTier::Extra),
            "--api" => ls_options.api_only = true,
            "--help" | "-h" => return Err(PluginParseError::Help),
            "--scan-dir" => {
                scan_dirs.push(
                    take_plugin_manifest_path(argv, index, "--scan-dir")
                        .map_err(PluginParseError::Usage)?,
                );
                index += 1;
            }
            "--disabled" => {
                options.disabled_plugins.push(
                    take_plugin_manifest_value(argv, index, "--disabled")
                        .map_err(PluginParseError::Usage)?,
                );
                index += 1;
            }
            "--runtime-version" => {
                options.runtime_version = take_plugin_manifest_value(argv, index, "--runtime-version")
                    .map_err(PluginParseError::Usage)?;
                index += 1;
            }
            "--use-cache" => options.use_cache = true,
            other => {
                return Err(PluginParseError::Usage(format!(
                    "plugin ls: unknown argument {other}"
                )));
            }
        }
        index += 1;
    }
    if !scan_dirs.is_empty() {
        options.scan_dirs = scan_dirs;
    }

    Ok(PluginAction::Ls { options, ls_options })
}

fn plugin_ls_help() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: "usage: maw plugin <init|build|install|create|ls|info|remove|enable <name...>|disable> [args]\n  ls: compact by default; use -v for full table; filters: --core --standard --extra --api\n".to_owned(),
        stderr: String::new(),
    }
}

fn render_plugin_ls(plugins: &[LoadedPlugin], options: &PluginLsOptions) -> String {
    let mut rows = plugins
        .iter()
        .map(PluginLsRow::new)
        .filter(|row| options.tiers.is_empty() || options.tiers.contains(&row.tier))
        .filter(|row| !options.api_only || row.api_path.is_some())
        .collect::<Vec<_>>();
    rows.sort_by_key(|row| (plugin_tier_order(row.tier), row.name.to_owned()));

    if rows.is_empty() {
        return if plugins.is_empty() {
            "no plugins installed\n".to_owned()
        } else {
            format!("no plugins{}.\n", plugin_ls_filter_label(options))
        };
    }

    if !options.verbose {
        return render_plugin_ls_compact(&rows, options);
    }

    render_plugin_ls_table(&rows)
}

fn render_plugin_ls_compact(rows: &[PluginLsRow<'_>], options: &PluginLsOptions) -> String {
    let active = rows.iter().filter(|row| !row.disabled).count();
    let disabled = rows.len() - active;
    let core = rows
        .iter()
        .filter(|row| row.tier == PluginTier::Core)
        .count();
    let standard = rows
        .iter()
        .filter(|row| row.tier == PluginTier::Standard)
        .count();
    let extra = rows
        .iter()
        .filter(|row| row.tier == PluginTier::Extra)
        .count();
    let cli = rows.iter().filter(|row| row.has_cli).count();
    let api = rows.iter().filter(|row| row.api_path.is_some()).count();
    let missing = rows.iter().filter(|row| row.missing_executable).count();
    let health = if missing == 0 {
        "ok".to_owned()
    } else {
        format!(
            "{missing} missing executable{}",
            if missing == 1 { "" } else { "s" }
        )
    };

    format!(
        "{} plugin{} ({} active, {} disabled){}\n  core: {core} · standard: {standard} · extra: {extra}\n  cli: {cli} · api: {api} · health: {health}\n",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        active,
        disabled,
        plugin_ls_filter_label(options)
    )
}

fn render_plugin_ls_table(rows: &[PluginLsRow<'_>]) -> String {
    let mut output = String::new();
    for tier in [PluginTier::Core, PluginTier::Standard, PluginTier::Extra] {
        let tier_rows = rows
            .iter()
            .filter(|row| row.tier == tier)
            .collect::<Vec<_>>();
        if tier_rows.is_empty() {
            continue;
        }
        let widths = PluginLsWidths::new(&tier_rows);

        let _ = writeln!(output, "\n\x1b[1m{}\x1b[0m ({})", tier.as_str(), tier_rows.len());
        writeln_padded_row(
            &mut output,
            &["name", "version", "tier", "surfaces", "dir"],
            &widths,
        );
        writeln_separator(&mut output, &widths);

        for row in tier_rows {
            let tier_label = format!(
                "{} {}",
                plugin_ls_tier_icon(row.tier, row.disabled),
                if row.disabled { "disabled" } else { row.tier.as_str() }
            );
            writeln_padded_row(
                &mut output,
                &[row.name, row.version, &tier_label, &row.surfaces, &row.dir],
                &widths,
            );
        }
    }

    let active = rows.iter().filter(|row| !row.disabled).count();
    let disabled = rows.len() - active;
    if disabled > 0 {
        let _ = writeln!(
            output,
            "\n{active} active. {disabled} disabled — use 'maw plugin ls --all' to see them."
        );
    } else {
        let _ = writeln!(output, "\n{active} active");
    }
    output
}

fn plugin_ls_filter_label(options: &PluginLsOptions) -> String {
    let mut parts = options
        .tiers
        .iter()
        .map(|tier| tier.as_str())
        .collect::<Vec<_>>();
    if options.api_only {
        parts.push("api");
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!(" matching {}", parts.join("+"))
    }
}

struct PluginLsRow<'a> {
    name: &'a str,
    version: &'a str,
    tier: PluginTier,
    surfaces: String,
    dir: String,
    disabled: bool,
    has_cli: bool,
    missing_executable: bool,
    api_path: Option<&'a str>,
}

impl<'a> PluginLsRow<'a> {
    fn new(plugin: &'a LoadedPlugin) -> Self {
        let manifest = &plugin.manifest;
        let cli_command = plugin_ls_cli_command(plugin);
        let api_path = manifest.api.as_ref().map(|api| api.path.as_str());
        let executable_path = match plugin.kind {
            LoadedPluginKind::Ts => plugin.entry_path.as_ref(),
            LoadedPluginKind::Wasm => (!plugin.wasm_path.as_os_str().is_empty()).then_some(&plugin.wasm_path),
        };
        Self {
            name: &manifest.name,
            version: &manifest.version,
            tier: plugin_ls_effective_tier(manifest),
            surfaces: plugin_ls_surfaces(cli_command.as_deref(), api_path),
            dir: shorten_home(&plugin.dir),
            disabled: plugin.disabled,
            has_cli: cli_command.is_some(),
            missing_executable: executable_path.is_some_and(|path| !path.exists()),
            api_path,
        }
    }
}

struct PluginLsWidths {
    name: usize,
    version: usize,
    tier: usize,
    surfaces: usize,
    dir: usize,
}

impl PluginLsWidths {
    fn new(rows: &[&PluginLsRow<'_>]) -> Self {
        let mut widths = Self {
            name: "name".chars().count(),
            version: "version".chars().count(),
            tier: "tier".chars().count(),
            surfaces: "surfaces".chars().count(),
            dir: "dir".chars().count(),
        };
        for row in rows {
            widths.name = widths.name.max(row.name.chars().count());
            widths.version = widths.version.max(row.version.chars().count());
            let tier_label = format!("{} {}", plugin_ls_tier_icon(row.tier, row.disabled), row.tier.as_str());
            widths.tier = widths.tier.max(tier_label.chars().count());
            widths.surfaces = widths.surfaces.max(row.surfaces.chars().count());
            widths.dir = widths.dir.max(row.dir.chars().count());
        }
        widths
    }
}

fn writeln_padded_row(output: &mut String, cells: &[&str; 5], widths: &PluginLsWidths) {
    let padded = [
        pad_end_chars(cells[0], widths.name),
        pad_end_chars(cells[1], widths.version),
        pad_end_chars(cells[2], widths.tier),
        pad_end_chars(cells[3], widths.surfaces),
        pad_end_chars(cells[4], widths.dir),
    ];
    let _ = writeln!(
        output,
        "{}  {}  {}  {}  {}",
        padded[0], padded[1], padded[2], padded[3], padded[4]
    );
}

fn writeln_separator(output: &mut String, widths: &PluginLsWidths) {
    let _ = writeln!(
        output,
        "{}  {}  {}  {}  {}",
        "─".repeat(widths.name),
        "─".repeat(widths.version),
        "─".repeat(widths.tier),
        "─".repeat(widths.surfaces),
        "─".repeat(widths.dir)
    );
}

fn pad_end_chars(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        value.to_owned()
    } else {
        format!("{}{}", value, " ".repeat(width - len))
    }
}

fn plugin_ls_surfaces(cli_command: Option<&str>, api_path: Option<&str>) -> String {
    let mut surfaces = Vec::new();
    if let Some(command) = cli_command {
        surfaces.push(format!("cli:{command}"));
    }
    if let Some(api_path) = api_path {
        surfaces.push(format!("api:{api_path}"));
    }
    if surfaces.is_empty() {
        "—".to_owned()
    } else {
        surfaces.join(", ")
    }
}

fn plugin_ls_cli_command(plugin: &LoadedPlugin) -> Option<String> {
    plugin.manifest.cli.as_ref().map_or_else(
        || match plugin.kind {
            LoadedPluginKind::Ts if plugin.entry_path.is_some() => Some(plugin.manifest.name.clone()),
            LoadedPluginKind::Wasm if !plugin.wasm_path.as_os_str().is_empty() => {
                Some(plugin.manifest.name.clone())
            }
            LoadedPluginKind::Ts | LoadedPluginKind::Wasm => None,
        },
        |cli| Some(cli.command.clone()),
    )
}

fn plugin_ls_effective_tier(manifest: &PluginManifest) -> PluginTier {
    manifest
        .tier
        .unwrap_or_else(|| plugin_ls_weight_to_tier(manifest.weight.unwrap_or(50)))
}

fn plugin_ls_weight_to_tier(weight: u64) -> PluginTier {
    if weight < 10 {
        PluginTier::Core
    } else if weight < 50 {
        PluginTier::Standard
    } else {
        PluginTier::Extra
    }
}

fn plugin_tier_order(tier: PluginTier) -> u8 {
    match tier {
        PluginTier::Core => 0,
        PluginTier::Standard => 1,
        PluginTier::Extra => 2,
    }
}

fn plugin_ls_tier_icon(tier: PluginTier, disabled: bool) -> &'static str {
    if disabled {
        "\x1b[90m○\x1b[0m"
    } else {
        match tier {
            PluginTier::Core => "\x1b[32m●\x1b[0m",
            PluginTier::Standard => "\x1b[36m●\x1b[0m",
            PluginTier::Extra => "\x1b[33m●\x1b[0m",
        }
    }
}

fn shorten_home(path: &Path) -> String {
    let raw = path_string(path);
    std::env::var("HOME").map_or(raw.clone(), |home| {
        raw.strip_prefix(&home)
            .map_or(raw.clone(), |suffix| format!("~{suffix}"))
    })
}
