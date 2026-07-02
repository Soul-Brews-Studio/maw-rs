fn parse_plugin_manifest_invoke_source(value: &str) -> Result<InvokeSource, String> {
    match value {
        "cli" => Ok(InvokeSource::Cli),
        "api" => Ok(InvokeSource::Api),
        "peer" => Ok(InvokeSource::Peer),
        other => Err(format!("plugin-manifest invoke: unknown --source {other}")),
    }
}

struct PluginManifestImportArgs {
    plugin: String,
    symbol: String,
    module_symbols: BTreeMap<String, String>,
}

fn parse_plugin_manifest_registry_args(
    argv: &[String],
    include_import_args: bool,
) -> Result<
    (
        bool,
        DiscoverPackagesOptions,
        Option<PluginManifestImportArgs>,
    ),
    String,
> {
    let mut plan_json = false;
    let mut scan_dirs = Vec::new();
    let mut disabled_plugins = Vec::new();
    let mut runtime_version = "1.0.0".to_owned();
    let mut use_cache = false;
    let mut plugin = None;
    let mut symbol = None;
    let mut module_symbols = BTreeMap::new();
    let command = if include_import_args {
        "plugin-manifest import-symbol"
    } else {
        "plugin-manifest discover"
    };
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--scan-dir" => {
                scan_dirs.push(take_plugin_manifest_path(argv, index, "--scan-dir")?);
                index += 1;
            }
            "--disabled" => {
                disabled_plugins.push(take_plugin_manifest_value(argv, index, "--disabled")?);
                index += 1;
            }
            "--runtime-version" => {
                runtime_version = take_plugin_manifest_value(argv, index, "--runtime-version")?;
                index += 1;
            }
            "--use-cache" => use_cache = true,
            "--plugin" if include_import_args => {
                plugin = Some(take_plugin_manifest_value(argv, index, "--plugin")?);
                index += 1;
            }
            "--symbol" if include_import_args => {
                symbol = Some(take_plugin_manifest_value(argv, index, "--symbol")?);
                index += 1;
            }
            "--module-symbol" if include_import_args => {
                let raw = take_plugin_manifest_value(argv, index, "--module-symbol")?;
                let Some((name, value)) = raw.split_once('=') else {
                    return Err(
                        "plugin-manifest import-symbol: --module-symbol must be name=value"
                            .to_owned(),
                    );
                };
                module_symbols.insert(name.to_owned(), value.to_owned());
                index += 1;
            }
            other => return Err(format!("{command}: unknown argument {other}")),
        }
        index += 1;
    }
    if scan_dirs.is_empty() {
        return Err(format!("{command}: --scan-dir is required"));
    }
    let options = DiscoverPackagesOptions {
        scan_dirs,
        disabled_plugins,
        runtime_version,
        use_cache,
    };
    let import = if include_import_args {
        Some(PluginManifestImportArgs {
            plugin: plugin
                .ok_or_else(|| "plugin-manifest import-symbol: --plugin is required".to_owned())?,
            symbol: symbol
                .ok_or_else(|| "plugin-manifest import-symbol: --symbol is required".to_owned())?,
            module_symbols,
        })
    } else {
        None
    };
    Ok((plan_json, options, import))
}

fn take_plugin_manifest_path(
    argv: &[String],
    index: usize,
    name: &str,
) -> Result<std::path::PathBuf, String> {
    Ok(std::path::PathBuf::from(take_plugin_manifest_value(
        argv, index, name,
    )?))
}

fn take_plugin_manifest_value(argv: &[String], index: usize, name: &str) -> Result<String, String> {
    argv.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("plugin-manifest: missing {name} value"))
}

fn plugin_manifest_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs plugin-manifest parse --dir <dir> --json <json> [--plan-json]\n       maw-rs plugin-manifest load --dir <dir> [--plan-json]\n       maw-rs plugin-manifest discover --scan-dir <dir>... [--disabled <name>]... [--runtime-version <version>] [--use-cache] [--plan-json]\n       maw-rs plugin-manifest import-symbol --scan-dir <dir>... --plugin <name> --symbol <name> [--module-symbol <name=value>]... [--disabled <name>]... [--runtime-version <version>] [--plan-json]\n       maw-rs plugin-manifest invoke --scan-dir <dir>... --plugin <name> [--source <cli|api|peer>] [--arg <arg>]... [--disabled <name>]... [--runtime-version <version>] [--plan-json]\n"
        ),
    }
}

fn render_plugin_discover_json(
    options: &DiscoverPackagesOptions,
    plugins: &[LoadedPlugin],
    warnings: &[String],
) -> String {
    let scan_dirs = options
        .scan_dirs
        .iter()
        .map(path_string)
        .collect::<Vec<_>>();
    let plugin_json = plugins
        .iter()
        .map(render_loaded_plugin_json)
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"command\":\"plugin-manifest\",\"kind\":\"discover\",\"scanDirs\":{},\"runtimeVersion\":{},\"disabledPlugins\":{},\"useCache\":{},\"plugins\":[{plugin_json}],\"warnings\":{}}}\n",
        json_string_array(&scan_dirs),
        json_string(&options.runtime_version),
        json_string_array(&options.disabled_plugins),
        options.use_cache,
        json_string_array(warnings)
    )
}

fn render_plugin_import_symbol_json(
    plugin: &str,
    symbol: &str,
    value: &str,
    module_path: Option<&std::path::Path>,
    warnings: &[String],
) -> String {
    format!(
        "{{\"command\":\"plugin-manifest\",\"kind\":\"import-symbol\",\"plugin\":{},\"symbol\":{},\"value\":{},\"modulePath\":{},\"warnings\":{}}}\n",
        json_string(plugin),
        json_string(symbol),
        json_string(value),
        module_path.map_or_else(|| "null".to_owned(), |path| {
            json_string(&path_string(path))
        }),
        json_string_array(warnings)
    )
}

fn render_plugin_invoke_json(
    plugin: &LoadedPlugin,
    ctx: &InvokeContext,
    result: &InvokeResult,
    warnings: &[String],
) -> String {
    let runtime_mode = match plugin.kind {
        LoadedPluginKind::Wasm => "extism-wasm",
        LoadedPluginKind::Ts => "universal-cli",
    };
    format!(
        "{{\"command\":\"plugin-manifest\",\"kind\":\"invoke\",\"plugin\":{},\"source\":{},\"args\":{},\"result\":{},\"runtime\":{{\"mode\":{},\"pluginKind\":{},\"wasmExport\":{},\"hostFnCount\":{},\"noBunFallback\":true,\"tsRefused\":false,\"muslStaticCompat\":\"extism-crate-linked\"}},\"warnings\":{}}}\n",
        json_string(&plugin.manifest.name),
        json_string(ctx.source.as_str()),
        json_string_array(&ctx.args),
        render_invoke_result_json(result),
        json_string(runtime_mode),
        json_string(plugin.kind.as_str()),
        json_string(&plugin.wasm_export),
        HOST_FN_NAMES.len(),
        json_string_array(warnings)
    )
}

fn render_invoke_result_json(result: &InvokeResult) -> String {
    format!(
        "{{\"ok\":{},\"output\":{},\"error\":{}}}",
        result.ok,
        json_opt_string(result.output.as_deref()),
        json_opt_string(result.error.as_deref())
    )
}

fn render_loaded_plugin_json(plugin: &LoadedPlugin) -> String {
    format!(
        "{{\"dir\":{},\"wasmPath\":{},\"entryPath\":{},\"kind\":{},\"disabled\":{},\"manifest\":{}}}",
        json_string(&path_string(&plugin.dir)),
        json_string(&path_string(&plugin.wasm_path)),
        plugin.entry_path.as_ref().map_or_else(|| "null".to_owned(), |path| {
            json_string(&path_string(path))
        }),
        json_string(plugin.kind.as_str()),
        plugin.disabled,
        render_plugin_manifest_json(&plugin.manifest)
    )
}

fn render_plugin_manifest_json(manifest: &PluginManifest) -> String {
    let weight = manifest
        .weight
        .map_or_else(|| "null".to_owned(), |weight| weight.to_string());
    format!(
        "{{\"name\":{},\"version\":{},\"weight\":{weight},\"tier\":{},\"wasm\":{},\"entry\":{},\"sdk\":{},\"cli\":{},\"api\":{},\"description\":{},\"author\":{},\"target\":{},\"capabilityNamespaces\":{},\"capabilities\":{},\"capabilityWarnings\":{},\"artifact\":{}}}",
        json_string(&manifest.name),
        json_string(&manifest.version),
        manifest.tier.map_or_else(|| "null".to_owned(), |tier| json_string(tier.as_str())),
        json_opt_string(manifest.wasm.as_deref()),
        json_opt_string(manifest.entry.as_deref()),
        json_string(&manifest.sdk),
        render_plugin_cli_json(manifest.cli.as_ref()),
        render_plugin_api_json(manifest.api.as_ref()),
        json_opt_string(manifest.description.as_deref()),
        json_opt_string(manifest.author.as_deref()),
        manifest.target.map_or_else(|| "null".to_owned(), |target| json_string(target.as_str())),
        manifest.capability_namespaces.as_ref().map_or_else(|| "null".to_owned(), |values| json_string_array(values)),
        manifest.capabilities.as_ref().map_or_else(|| "null".to_owned(), |values| json_string_array(values)),
        json_string_array(&manifest.capability_warnings),
        manifest.artifact.as_ref().map_or_else(|| "null".to_owned(), |artifact| {
            format!(
                "{{\"path\":{},\"sha256\":{}}}",
                json_string(&artifact.path),
                json_opt_string(artifact.sha256.as_deref())
            )
        })
    )
}

fn render_plugin_cli_json(cli: Option<&maw_plugin_manifest::PluginCli>) -> String {
    let Some(cli) = cli else {
        return "null".to_owned();
    };
    let flags = cli.flags.as_ref().map_or_else(
        || "null".to_owned(),
        |flags| {
            let entries = flags
                .iter()
                .map(|(name, kind)| format!("{}:{}", json_string(name), json_string(kind.as_str())))
                .collect::<Vec<_>>()
                .join(",");
            format!("{{{entries}}}")
        },
    );
    format!(
        "{{\"command\":{},\"aliases\":{},\"help\":{},\"flags\":{flags}}}",
        json_string(&cli.command),
        cli.aliases
            .as_ref()
            .map_or_else(|| "null".to_owned(), |values| json_string_array(values)),
        json_opt_string(cli.help.as_deref())
    )
}

fn render_plugin_api_json(api: Option<&maw_plugin_manifest::PluginApi>) -> String {
    let Some(api) = api else {
        return "null".to_owned();
    };
    let methods = api
        .methods
        .iter()
        .map(|method| method.as_str().to_owned())
        .collect::<Vec<_>>();
    format!(
        "{{\"path\":{},\"methods\":{}}}",
        json_string(&api.path),
        json_string_array(&methods)
    )
}

fn json_opt_string(value: Option<&str>) -> String {
    value.map_or_else(|| "null".to_owned(), json_string)
}

fn run_bind_host_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_bind_host_constants_plan(&argv[1..]);
    }

    let parsed = match parse_bind_host_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return bind_host_usage_error(&message),
    };
    let result = resolve_bind_host(
        &parsed.config,
        parsed.maw_host.as_deref(),
        parsed.peers_store_len,
    );
    CliOutput {
        code: 0,
        stdout: if parsed.plan_json {
            render_bind_host_plan_json(&parsed.config, parsed.maw_host.as_deref(), &result)
        } else {
            format!("{}\n", result.hostname)
        },
        stderr: String::new(),
    }
}

struct BindHostArgs {
    plan_json: bool,
    config: BindConfig,
    maw_host: Option<String>,
    peers_store_len: Result<usize, String>,
}

fn parse_bind_host_args(argv: &[String]) -> Result<BindHostArgs, String> {
    let mut options = BindHostArgs {
        plan_json: false,
        config: BindConfig::default(),
        maw_host: None,
        peers_store_len: Ok(0),
    };

    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => options.plan_json = true,
            "--config-peers-len" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("bind-host: missing --config-peers-len value".to_owned());
                };
                options.config.peers_len = parse_usize_arg(value, "bind-host: --config-peers-len")?;
                index += 1;
            }
            "--config-named-peers-len" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("bind-host: missing --config-named-peers-len value".to_owned());
                };
                options.config.named_peers_len =
                    parse_usize_arg(value, "bind-host: --config-named-peers-len")?;
                index += 1;
            }
            "--maw-host" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("bind-host: missing --maw-host value".to_owned());
                };
                options.maw_host = Some(value.to_owned());
                index += 1;
            }
            "--peers-store-len" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("bind-host: missing --peers-store-len value".to_owned());
                };
                options.peers_store_len =
                    Ok(parse_usize_arg(value, "bind-host: --peers-store-len")?);
                index += 1;
            }
            "--peers-store-error" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("bind-host: missing --peers-store-error value".to_owned());
                };
                options.peers_store_len = Err(value.to_owned());
                index += 1;
            }
            arg => return Err(format!("bind-host: unknown argument {arg}")),
        }
        index += 1;
    }

    Ok(options)
}

fn render_bind_host_plan_json(
    config: &BindConfig,
    maw_host: Option<&str>,
    result: &BindHostResult,
) -> String {
    let mut input_fields = vec![
        format!("\"configPeersLen\":{}", config.peers_len),
        format!("\"configNamedPeersLen\":{}", config.named_peers_len),
    ];
    if let Some(maw_host) = maw_host {
        input_fields.push(format!("\"mawHost\":{}", json_string(maw_host)));
    }
    let reason = result
        .reason
        .map_or("null".to_owned(), |reason| json_string(reason.as_str()));
    format!(
        "{{\"command\":\"bind-host\",\"input\":{{{}}},\"hostname\":{},\"reason\":{reason}}}\n",
        input_fields.join(","),
        json_string(&result.hostname)
    )
}

