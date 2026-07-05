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

