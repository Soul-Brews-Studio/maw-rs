fn parse_plugin_ls_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut options = DiscoverPackagesOptions {
        runtime_version: "1.0.0".to_owned(),
        ..DiscoverPackagesOptions::default()
    };
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

fn plugin_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs plugin ls [-v|--verbose] [--core] [--standard] [--extra] [--api] [--scan-dir <dir>]... [--disabled <name>]... [--runtime-version <version>] [--use-cache]\n       maw-rs plugin <infer-capabilities|build|init|install> [args]\n"
        ),
    }
}

fn plugin_ls_help() -> CliOutput {
    CliOutput {
        code: 0,
        stdout: "usage: maw plugin <init|build|install|create|ls|info|remove|enable <name...>|disable> [args]\n  ls: compact by default; use -v for full table; filters: --core --standard --extra --api\n".to_owned(),
        stderr: String::new(),
    }
}


fn render_plugin_build_summary_json(summary: &maw_plugin_manifest::PluginBuildSummary) -> String {
    let dts = summary
        .dts_path
        .as_ref()
        .map_or_else(|| "null".to_owned(), |path| json_string(&path_string(path)));
    format!(
        r#"{{"command":"plugin","kind":"build","name":{},"version":{},"dir":{},"bundlePath":{},"sizeBytes":{},"capabilities":{},"inferredOnly":{},"declaredOnly":{},"sha256":{},"manifestPath":{},"dtsPath":{dts}}}
"#,
        json_string(&summary.name),
        json_string(&summary.version),
        json_string(&path_string(&summary.dir)),
        json_string(&path_string(&summary.bundle_path)),
        summary.size_bytes,
        json_string_array(&summary.capabilities),
        json_string_array(&summary.inferred_only),
        json_string_array(&summary.declared_only),
        json_string(&summary.sha256),
        json_string(&path_string(&summary.manifest_path)),
    )
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

