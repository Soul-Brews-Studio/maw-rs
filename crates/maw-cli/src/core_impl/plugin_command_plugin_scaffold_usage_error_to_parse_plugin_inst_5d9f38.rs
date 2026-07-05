fn plugin_scaffold_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs plugin-scaffold validate-name --name <name> [--plan-json]\n       maw-rs plugin-scaffold manifest --name <name> (--rust|--as) [--plan-json]\n       maw-rs plugin-scaffold constants [--plan-json]\n"
        ),
    }
}

fn run_plugin_plan(argv: &[String]) -> CliOutput {
    let action = match parse_plugin_args(argv) {
        Ok(action) => action,
        Err(PluginParseError::Usage(message)) => return plugin_usage_error(&message),
        Err(PluginParseError::Help) => return plugin_ls_help(),
    };

    match action {
        PluginAction::Ls { options, ls_options } => {
            let report = discover_packages(&options);
            CliOutput {
                code: 0,
                stdout: render_plugin_ls(&report.plugins, &ls_options),
                stderr: String::new(),
            }
        }
        PluginAction::InferCapabilities { source, plan_json } => {
            let caps = infer_plugin_capabilities(&source);
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!("{{\"command\":\"plugin\",\"kind\":\"infer-capabilities\",\"capabilities\":{}}}\n", json_string_array(&caps))
                } else {
                    format!("{}\n", caps.join("\n"))
                },
                stderr: String::new(),
            }
        }
        PluginAction::Build { dir, emit_types, plan_json } => match build_js_plugin_dir(&dir, emit_types) {
            Ok(summary) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    render_plugin_build_summary_json(&summary)
                } else {
                    format!("built {}@{} {}\n", summary.name, summary.version, path_string(&summary.bundle_path))
                },
                stderr: String::new(),
            },
            Err(message) => plugin_usage_error(&message),
        },
        PluginAction::Init { name, dir, plan_json } => match init_js_plugin_dir(&name, &dir) {
            Ok(summary) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!("{{\"command\":\"plugin\",\"kind\":\"init\",\"name\":{},\"dir\":{},\"manifestPath\":{},\"entryPath\":{}}}\n", json_string(&summary.name), json_string(&path_string(&summary.dir)), json_string(&path_string(&summary.manifest_path)), json_string(&path_string(&summary.entry_path)))
                } else {
                    format!("initialized {} {}\n", summary.name, path_string(&summary.dir))
                },
                stderr: String::new(),
            },
            Err(message) => plugin_usage_error(&message),
        },
        PluginAction::Install { source_dir, install_root, plan_json } => match install_built_plugin_dir(&source_dir, &install_root) {
            Ok(summary) => CliOutput {
                code: 0,
                stdout: if plan_json {
                    let copied = summary.copied_files.iter().map(path_string).collect::<Vec<_>>();
                    format!("{{\"command\":\"plugin\",\"kind\":\"install\",\"name\":{},\"version\":{},\"sourceDir\":{},\"installDir\":{},\"copiedFiles\":{}}}\n", json_string(&summary.name), json_string(&summary.version), json_string(&path_string(&summary.source_dir)), json_string(&path_string(&summary.install_dir)), json_string_array(&copied))
                } else {
                    format!("installed {}@{} {}\n", summary.name, summary.version, path_string(&summary.install_dir))
                },
                stderr: String::new(),
            },
            Err(message) => plugin_usage_error(&message),
        },
    }
}

enum PluginAction {
    Ls {
        options: DiscoverPackagesOptions,
        ls_options: PluginLsOptions,
    },
    InferCapabilities { source: String, plan_json: bool },
    Build { dir: std::path::PathBuf, emit_types: bool, plan_json: bool },
    Init { name: String, dir: std::path::PathBuf, plan_json: bool },
    Install { source_dir: std::path::PathBuf, install_root: std::path::PathBuf, plan_json: bool },
}

#[derive(Default)]
struct PluginLsOptions {
    verbose: bool,
    tiers: Vec<PluginTier>,
    api_only: bool,
}

enum PluginParseError {
    Usage(String),
    Help,
}

fn parse_plugin_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err(PluginParseError::Usage("plugin: expected ls".to_owned()));
    };
    match kind {
        "ls" | "list" => parse_plugin_ls_args(&argv[1..]),
        "infer-capabilities" => parse_plugin_infer_args(&argv[1..]),
        "build" => parse_plugin_build_args(&argv[1..]),
        "init" => parse_plugin_init_args(&argv[1..]),
        "install" => parse_plugin_install_args(&argv[1..]),
        other => Err(PluginParseError::Usage(format!(
            "plugin: unknown subcommand {other}"
        ))),
    }
}


fn parse_plugin_infer_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut plan_json = false;
    let mut source = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--source" => {
                source = Some(take_plugin_manifest_value(argv, index, "--source").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            "--file" => {
                let path = take_plugin_manifest_path(argv, index, "--file").map_err(PluginParseError::Usage)?;
                source = Some(std::fs::read_to_string(&path).map_err(|error| {
                    PluginParseError::Usage(format!("plugin infer-capabilities: read failed: {error}"))
                })?);
                index += 1;
            }
            other => return Err(PluginParseError::Usage(format!("plugin infer-capabilities: unknown argument {other}"))),
        }
        index += 1;
    }
    Ok(PluginAction::InferCapabilities {
        source: source.ok_or_else(|| PluginParseError::Usage("plugin infer-capabilities: --source or --file is required".to_owned()))?,
        plan_json,
    })
}

fn parse_plugin_build_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut dir = std::path::PathBuf::from(".");
    let mut emit_types = false;
    let mut plan_json = false;
    let mut positional = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--types" => emit_types = true,
            "--plan-json" => plan_json = true,
            other if !other.starts_with('-') && !positional => {
                dir = std::path::PathBuf::from(other);
                positional = true;
            }
            other => return Err(PluginParseError::Usage(format!("plugin build: unknown argument {other}"))),
        }
        index += 1;
    }
    Ok(PluginAction::Build { dir, emit_types, plan_json })
}

fn parse_plugin_init_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut name = None;
    let mut dir = None;
    let mut plan_json = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--dir" => {
                dir = Some(take_plugin_manifest_path(argv, index, "--dir").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            other if !other.starts_with('-') && name.is_none() => name = Some(other.to_owned()),
            other => return Err(PluginParseError::Usage(format!("plugin init: unknown argument {other}"))),
        }
        index += 1;
    }
    let name = name.ok_or_else(|| PluginParseError::Usage("plugin init: name is required".to_owned()))?;
    let dir = dir.unwrap_or_else(|| std::path::PathBuf::from(&name));
    Ok(PluginAction::Init { name, dir, plan_json })
}

fn parse_plugin_install_args(argv: &[String]) -> Result<PluginAction, PluginParseError> {
    let mut source_dir = None;
    let mut install_root = None;
    let mut plan_json = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--root" => {
                install_root = Some(take_plugin_manifest_path(argv, index, "--root").map_err(PluginParseError::Usage)?);
                index += 1;
            }
            other if !other.starts_with('-') && source_dir.is_none() => source_dir = Some(std::path::PathBuf::from(other)),
            other => return Err(PluginParseError::Usage(format!("plugin install: unknown argument {other}"))),
        }
        index += 1;
    }
    Ok(PluginAction::Install {
        source_dir: source_dir.ok_or_else(|| PluginParseError::Usage("plugin install: source dir is required".to_owned()))?,
        install_root: install_root.ok_or_else(|| PluginParseError::Usage("plugin install: --root is required for native plan installs".to_owned()))?,
        plan_json,
    })
}

