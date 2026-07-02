enum XdgPlanAction {
    Paths { plan_json: bool, env: MawXdgEnv },
    CorePaths { plan_json: bool, env: MawXdgEnv },
    ValidateInstance { plan_json: bool, name: String },
}

struct XdgCliEnvArgs {
    plan_json: bool,
    home: String,
    vars: Vec<(String, String)>,
}

struct XdgResolvedPaths {
    xdg_enabled: bool,
    runtime_home: String,
    data_dir: String,
    state_dir: String,
    cache_dir: String,
    config_dir: String,
    data_path: String,
    state_path: String,
    cache_path: String,
    config_path: String,
}

impl XdgResolvedPaths {
    fn from_env(env: &MawXdgEnv) -> Self {
        Self {
            xdg_enabled: is_maw_xdg_enabled(env),
            runtime_home: path_string(maw_runtime_home_dir(env)),
            data_dir: path_string(maw_data_dir(env)),
            state_dir: path_string(maw_state_dir(env)),
            cache_dir: path_string(maw_cache_dir(env)),
            config_dir: path_string(maw_config_dir(env)),
            data_path: path_string(maw_data_path(env, &["plugins"])),
            state_path: path_string(maw_state_path(env, &["peers.json"])),
            cache_path: path_string(maw_cache_path(env, &["registry-cache.json"])),
            config_path: path_string(maw_config_path(env, &["maw.config.json"])),
        }
    }
}

fn parse_xdg_plan_args(argv: &[String]) -> Result<XdgPlanAction, String> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err("xdg: expected paths, core-paths, or validate-instance".to_owned());
    };
    match kind {
        "paths" => {
            let parsed = parse_xdg_env_args(&argv[1..])?;
            Ok(XdgPlanAction::Paths {
                plan_json: parsed.plan_json,
                env: MawXdgEnv::with_vars(parsed.home, parsed.vars),
            })
        }
        "core-paths" => {
            let parsed = parse_xdg_env_args(&argv[1..])?;
            Ok(XdgPlanAction::CorePaths {
                plan_json: parsed.plan_json,
                env: MawXdgEnv::with_vars(parsed.home, parsed.vars),
            })
        }
        "validate-instance" => parse_xdg_validate_instance_args(&argv[1..]),
        other => Err(format!("xdg: unknown subcommand {other}")),
    }
}

fn parse_xdg_env_args(argv: &[String]) -> Result<XdgCliEnvArgs, String> {
    let mut plan_json = false;
    let mut home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
    let mut vars = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--home" => {
                home = take_xdg_value(argv, index, "--home")?;
                index += 1;
            }
            "--env" => {
                let raw = take_xdg_value(argv, index, "--env")?;
                let Some((key, value)) = raw.split_once('=') else {
                    return Err("xdg: --env must be KEY=VALUE".to_owned());
                };
                vars.push((key.to_owned(), value.to_owned()));
                index += 1;
            }
            other => return Err(format!("xdg: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(XdgCliEnvArgs {
        plan_json,
        home,
        vars,
    })
}

fn parse_xdg_validate_instance_args(argv: &[String]) -> Result<XdgPlanAction, String> {
    let mut plan_json = false;
    let mut name = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--name" => {
                name = Some(take_xdg_value(argv, index, "--name")?);
                index += 1;
            }
            other => return Err(format!("xdg validate-instance: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(XdgPlanAction::ValidateInstance {
        plan_json,
        name: name.ok_or_else(|| "xdg validate-instance: --name is required".to_owned())?,
    })
}

fn take_xdg_value(argv: &[String], index: usize, name: &str) -> Result<String, String> {
    argv.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("xdg: missing {name} value"))
}

fn render_xdg_paths_json(paths: &XdgResolvedPaths) -> String {
    format!(
        "{{\"command\":\"xdg\",\"kind\":\"paths\",\"xdgEnabled\":{},\"runtimeHome\":{},\"dataDir\":{},\"stateDir\":{},\"cacheDir\":{},\"configDir\":{},\"dataPath\":{},\"statePath\":{},\"cachePath\":{},\"configPath\":{}}}\n",
        paths.xdg_enabled,
        json_string(&paths.runtime_home),
        json_string(&paths.data_dir),
        json_string(&paths.state_dir),
        json_string(&paths.cache_dir),
        json_string(&paths.config_dir),
        json_string(&paths.data_path),
        json_string(&paths.state_path),
        json_string(&paths.cache_path),
        json_string(&paths.config_path)
    )
}

fn render_xdg_core_paths_json(paths: &MawCorePaths) -> String {
    format!(
        "{{\"command\":\"xdg\",\"kind\":\"core-paths\",\"runtimeHome\":{},\"configDir\":{},\"fleetDir\":{},\"configFile\":{}}}\n",
        json_string(&path_string(&paths.runtime_home)),
        json_string(&path_string(&paths.config_dir)),
        json_string(&path_string(&paths.fleet_dir)),
        json_string(&path_string(&paths.config_file))
    )
}

fn path_string(path: impl AsRef<std::path::Path>) -> String {
    path.as_ref().to_string_lossy().into_owned()
}

fn run_xdg_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => return xdg_constants_usage_error(&format!("xdg constants: unknown arg {arg}")),
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_xdg_constants_json()
        } else {
            "xdg constants modes=legacy,xdg,MAW_HOME actions=paths,core-paths,validate-instance\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_xdg_constants_json() -> String {
    r#"{"command":"xdg","action":"constants","actions":["paths","core-paths","validate-instance"],"truthyMawXdg":["1","true","yes","on"],"overrideEnv":["MAW_HOME","MAW_CONFIG_DIR","MAW_DATA_DIR","MAW_STATE_DIR","MAW_CACHE_DIR"],"xdgBaseEnv":["XDG_CONFIG_HOME","XDG_DATA_HOME","XDG_STATE_HOME","XDG_CACHE_HOME"],"legacyDirs":{"runtime":"$HOME/.maw","config":"$HOME/.config/maw","data":"$HOME/.maw","state":"$HOME/.maw","cache":"$HOME/.maw"},"xdgDirs":{"runtime":"$XDG_STATE_HOME/maw","config":"$XDG_CONFIG_HOME/maw","data":"$XDG_DATA_HOME/maw","state":"$XDG_STATE_HOME/maw","cache":"$XDG_CACHE_HOME/maw"},"samplePaths":{"data":["plugins"],"state":["peers.json"],"cache":["registry-cache.json"],"config":["maw.config.json"]},"corePaths":{"fleetDir":"configDir/fleet","configFile":"configDir/maw.config.json"},"instanceName":{"maxBytes":32,"first":"lowercase ascii alnum","rest":"lowercase ascii alnum, underscore, hyphen"}}
"#
    .to_owned()
}

fn xdg_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", xdg_constants_usage()),
    }
}

fn xdg_constants_usage() -> &'static str {
    "usage: maw-rs xdg constants [--plan-json]"
}

fn xdg_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs xdg paths [--home <dir>] [--env <KEY=VALUE>]... [--plan-json]\n       maw-rs xdg core-paths [--home <dir>] [--env <KEY=VALUE>]... [--plan-json]\n       maw-rs xdg validate-instance --name <name> [--plan-json]\n       maw-rs xdg constants [--plan-json]\n"
        ),
    }
}

fn run_plugin_scaffold_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_plugin_scaffold_constants_plan(&argv[1..]);
    }

    let action = match parse_plugin_scaffold_args(argv) {
        Ok(action) => action,
        Err(message) => return plugin_scaffold_usage_error(&message),
    };
    match action {
        PluginScaffoldAction::ValidateName { plan_json, name } => {
            let error = validate_plugin_name(&name);
            let valid = error.is_none();
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    let error_json = error.map_or("null".to_owned(), |error| json_string(&error));
                    format!(
                        "{{\"command\":\"plugin-scaffold\",\"kind\":\"validate-name\",\"name\":{},\"valid\":{valid},\"error\":{error_json}}}\n",
                        json_string(&name)
                    )
                } else if valid {
                    "valid\n".to_owned()
                } else {
                    format!("{}\n", error.expect("invalid name has error"))
                },
                stderr: String::new(),
            }
        }
        PluginScaffoldAction::Manifest {
            plan_json,
            name,
            language,
        } => {
            let manifest_text = build_manifest_json(&name, language);
            let manifest: serde_json::Value = serde_json::from_str(&manifest_text)
                .expect("maw-plugin-scaffold emits valid manifest JSON");
            let language_name = match language {
                ScaffoldLanguage::Rust => "rust",
                ScaffoldLanguage::AssemblyScript => "assemblyscript",
            };
            CliOutput {
                code: 0,
                stdout: if plan_json {
                    format!(
                        "{{\"command\":\"plugin-scaffold\",\"kind\":\"manifest\",\"language\":{},\"manifest\":{manifest}}}\n",
                        json_string(language_name)
                    )
                } else {
                    manifest_text
                },
                stderr: String::new(),
            }
        }
    }
}

fn run_plugin_scaffold_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            other => {
                return plugin_scaffold_constants_usage_error(&format!(
                    "plugin-scaffold constants: unknown argument {other}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_plugin_scaffold_constants_json()
        } else {
            "plugin-scaffold constants actions=validate-name,manifest languages=rust,assemblyscript\n"
                .to_owned()
        },
        stderr: String::new(),
    }
}

fn render_plugin_scaffold_constants_json() -> String {
    r#"{"command":"plugin-scaffold","action":"constants","actions":["validate-name","manifest"],"languages":["rust","assemblyscript"],"nameRules":{"first":"lowercase ascii letter","rest":"lowercase ascii letters, digits, hyphen, underscore","emptyError":"name is required"},"manifestDefaults":{"version":"0.1.0","sdk":"^1.0.0","author":"","apiMethods":["GET","POST"]},"slugNormalization":{"slug":"underscores become hyphens","rustWasmArtifact":"hyphens become underscores"},"wasmPaths":{"rust":"./target/wasm32-unknown-unknown/release/<crate_name>.wasm","assemblyscript":"./build/release.wasm"},"copyTreeSkips":["target",".git","node_modules"],"guardErrors":["missing-type","conflicting-types","missing-name","invalid-name","destination-exists","scaffold"]}
"#
    .to_owned()
}

fn plugin_scaffold_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", plugin_scaffold_constants_usage()),
    }
}

fn plugin_scaffold_constants_usage() -> &'static str {
    "usage: maw-rs plugin-scaffold constants [--plan-json]"
}

enum PluginScaffoldAction {
    ValidateName {
        plan_json: bool,
        name: String,
    },
    Manifest {
        plan_json: bool,
        name: String,
        language: ScaffoldLanguage,
    },
}

fn parse_plugin_scaffold_args(argv: &[String]) -> Result<PluginScaffoldAction, String> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Err("plugin-scaffold: expected validate-name or manifest".to_owned());
    };
    match kind {
        "validate-name" => parse_plugin_scaffold_validate_args(&argv[1..]),
        "manifest" => parse_plugin_scaffold_manifest_args(&argv[1..]),
        other => Err(format!("plugin-scaffold: unknown subcommand {other}")),
    }
}

fn parse_plugin_scaffold_validate_args(argv: &[String]) -> Result<PluginScaffoldAction, String> {
    let mut plan_json = false;
    let mut name = None;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--name" => {
                name = Some(take_plugin_scaffold_value(argv, index, "--name")?);
                index += 1;
            }
            other => {
                return Err(format!(
                    "plugin-scaffold validate-name: unknown argument {other}"
                ))
            }
        }
        index += 1;
    }
    Ok(PluginScaffoldAction::ValidateName {
        plan_json,
        name: name.ok_or_else(|| "plugin-scaffold validate-name: --name is required".to_owned())?,
    })
}

fn parse_plugin_scaffold_manifest_args(argv: &[String]) -> Result<PluginScaffoldAction, String> {
    let mut plan_json = false;
    let mut name = None;
    let mut rust = false;
    let mut assembly_script = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--name" => {
                name = Some(take_plugin_scaffold_value(argv, index, "--name")?);
                index += 1;
            }
            "--rust" => rust = true,
            "--as" => assembly_script = true,
            other => {
                return Err(format!(
                    "plugin-scaffold manifest: unknown argument {other}"
                ))
            }
        }
        index += 1;
    }
    if !rust && !assembly_script {
        return Err("plugin-scaffold manifest: Specify either --rust or --as".to_owned());
    }
    if rust && assembly_script {
        return Err("plugin-scaffold manifest: Specify --rust or --as, not both".to_owned());
    }
    let name = name.ok_or_else(|| "plugin-scaffold manifest: --name is required".to_owned())?;
    if let Some(error) = validate_plugin_name(&name) {
        return Err(format!(
            "plugin-scaffold manifest: Invalid plugin name: {error}"
        ));
    }
    Ok(PluginScaffoldAction::Manifest {
        plan_json,
        name,
        language: if rust {
            ScaffoldLanguage::Rust
        } else {
            ScaffoldLanguage::AssemblyScript
        },
    })
}

fn take_plugin_scaffold_value(argv: &[String], index: usize, name: &str) -> Result<String, String> {
    argv.get(index + 1)
        .cloned()
        .ok_or_else(|| format!("plugin-scaffold: missing {name} value"))
}

