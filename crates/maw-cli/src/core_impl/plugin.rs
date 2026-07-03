const DISPATCH_102: &[DispatcherEntry] = &[DispatcherEntry {
    command: "plugin",
    handler: Handler::Sync(plugin_run_command),
}];

const PLUGIN_USAGE: &str = "usage: maw plugin <ls|info|install|remove|enable|disable|init|create|build|dev> [args]\n  ls/list                  list installed plugins\n  info <name>              show manifest and resolved paths\n  install <dir> --root R   install a built plugin directory\n  remove <name> --yes      archive installed plugin directory (Nothing Deleted)\n  enable <name...>         enable plugins in the local disabled registry\n  disable <name>           disable one plugin in the local disabled registry\n  init|create <name> [--rust] create file-only JS or Rust WASM plugin scaffold\n  build [dir] [--watch]    build Rust WASM plugins with cargo or AssemblyScript-compatible TS to WASM\n  dev [dir]                bounded one-build dev alias for plugin builds";
const PLUGIN_AS_TS_BOUNDARY: &str = "AssemblyScript ship-tier builds accept the repo's AS-compatible .ts subset; arbitrary Bun/Node TS still needs Javy (`cargo install javy`) or a prebuilt WASM artifact.";

fn plugin_run_command(argv: &[String]) -> CliOutput {
    if wants_help_before_positionals(argv, &[]) || argv.first().is_some_and(|arg| arg == "help") {
        return plugin_ok(PLUGIN_USAGE);
    }
    match plugin_parse_kind(argv).and_then(|kind| plugin_dispatch_kind(kind, &argv[1..])) {
        Ok(output) => output,
        Err(message) if message.is_empty() => plugin_ok(PLUGIN_USAGE),
        Err(message) => plugin_error(2, &message),
    }
}

fn plugin_parse_kind(argv: &[String]) -> Result<&str, String> {
    let Some(kind) = argv.first().map(String::as_str) else { return Err(String::new()); };
    if matches!(kind, "--help" | "-h" | "help") { return Err(String::new()); }
    if kind == "--" || kind.starts_with('-') { return Err("plugin: subcommand must not start with '-' or be '--'".to_owned()); }
    Ok(kind)
}

fn plugin_dispatch_kind(kind: &str, rest: &[String]) -> Result<CliOutput, String> {
    match kind {
        "ls" | "list" => Ok(run_plugin_plan(&plugin_with_subcommand("ls", rest))),
        "init" | "install" | "infer-capabilities" => Ok(run_plugin_plan(&plugin_with_subcommand(kind, rest))),
        "create" | "scaffold" => plugin_create(rest),
        "info" => plugin_info(rest),
        "enable" => plugin_enable(rest),
        "disable" => plugin_disable(rest),
        "remove" | "rm" | "uninstall" => plugin_remove(rest),
        "build" | "dev" => plugin_build_or_dev(kind, rest),
        other => Err(format!("plugin: unknown subcommand {other}")),
    }
}

fn plugin_with_subcommand(kind: &str, rest: &[String]) -> Vec<String> {
    let mut argv = Vec::with_capacity(rest.len() + 1);
    argv.push(kind.to_owned());
    argv.extend(rest.iter().cloned());
    argv
}

fn plugin_create(argv: &[String]) -> Result<CliOutput, String> {
    let parsed = plugin_parse_create(argv)?;
    if parsed.rust {
        return plugin_create_rust(&parsed);
    }
    match init_js_plugin_dir(&parsed.name, &parsed.dir) {
        Ok(summary) => Ok(CliOutput {
            code: 0,
            stdout: if parsed.plan_json { plugin_init_summary_json(&summary) } else { format!("created plugin {} {}\n", summary.name, path_string(&summary.dir)) },
            stderr: String::new(),
        }),
        Err(message) => Err(message),
    }
}

struct PluginCreateArgs { name: String, dir: std::path::PathBuf, plan_json: bool, rust: bool }

fn plugin_parse_create(argv: &[String]) -> Result<PluginCreateArgs, String> {
    let mut name = None;
    let mut dir = None;
    let mut plan_json = false;
    let mut rust = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => plan_json = true,
            "--rust" => rust = true,
            "--dir" => { dir = Some(plugin_take_path(argv, index, "--dir")?); index += 1; }
            other if !other.starts_with('-') && name.is_none() => name = Some(other.to_owned()),
            other => return Err(format!("plugin create: unknown argument {other}")),
        }
        index += 1;
    }
    let name = plugin_validate_name(&name.ok_or_else(|| "plugin create: name is required".to_owned())?)?;
    let dir = dir.unwrap_or_else(|| std::path::PathBuf::from(&name));
    Ok(PluginCreateArgs { name, dir, plan_json, rust })
}

fn plugin_create_rust(parsed: &PluginCreateArgs) -> Result<CliOutput, String> {
    let template_root = plugin_create_rust_template_root()?;
    let rust_template = template_root.join("rust");
    let as_template = template_root.join("as");
    plugin_write_builtin_rust_template(&rust_template)?;
    std::fs::create_dir_all(&as_template)
        .map_err(|error| format!("plugin create: as template create failed: {error}"))?;
    let request = PluginCreateRequest {
        name: Some(parsed.name.clone()),
        rust: true,
        assembly_script: false,
        dest: parsed.dir.clone(),
    };
    let result = cmd_plugin_create(&request, &rust_template, &as_template, "extism-pdk");
    let _ = std::fs::remove_dir_all(&template_root);
    result.map_err(|error| error.to_string())?;
    Ok(CliOutput {
        code: 0,
        stdout: if parsed.plan_json {
            plugin_create_rust_summary_json(&parsed.name, &parsed.dir)
        } else {
            format!("created plugin {} {}\n", parsed.name.replace('_', "-"), path_string(&parsed.dir))
        },
        stderr: String::new(),
    })
}

fn plugin_create_rust_template_root() -> Result<std::path::PathBuf, String> {
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    let root = std::env::temp_dir().join(format!(
        "maw-rs-plugin-create-rust-template-{}-{millis}",
        std::process::id()
    ));
    std::fs::create_dir_all(&root)
        .map_err(|error| format!("plugin create: template root create failed: {error}"))?;
    Ok(root)
}

fn plugin_write_builtin_rust_template(template: &std::path::Path) -> Result<(), String> {
    std::fs::create_dir_all(template.join("src"))
        .map_err(|error| format!("plugin create: rust template create failed: {error}"))?;
    std::fs::write(
        template.join("Cargo.toml"),
        r#"[package]
name = "hello-rust"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib"]

[dependencies]
extism-pdk = "=1.4.1"

[workspace]
"#,
    )
    .map_err(|error| format!("plugin create: rust Cargo.toml template failed: {error}"))?;
    std::fs::write(
        template.join("src").join("lib.rs"),
        r##"use extism_pdk::*;

#[plugin_fn]
pub fn handle(_input: String) -> FnResult<String> {
    Ok(r#"{"ok":true}"#.to_owned())
}
"##,
    )
    .map_err(|error| format!("plugin create: rust lib.rs template failed: {error}"))?;
    Ok(())
}

fn plugin_create_rust_summary_json(name: &str, dir: &std::path::Path) -> String {
    format!(
        "{{\"command\":\"plugin\",\"kind\":\"create\",\"language\":\"rust\",\"name\":{},\"dir\":{},\"manifestPath\":{},\"entryPath\":{}}}\n",
        json_string(&name.replace('_', "-")),
        json_string(&path_string(dir)),
        json_string(&path_string(dir.join("plugin.json"))),
        json_string(&path_string(dir.join("src").join("lib.rs")))
    )
}

fn plugin_info(argv: &[String]) -> Result<CliOutput, String> {
    let parsed = plugin_parse_named_scan(argv, "info")?;
    let plugin = plugin_find_loaded(&parsed.name, &parsed.options)?;
    Ok(CliOutput { code: 0, stdout: plugin_render_info(&plugin, parsed.json), stderr: String::new() })
}

struct PluginNamedScanArgs { name: String, options: DiscoverPackagesOptions, json: bool }

fn plugin_parse_named_scan(argv: &[String], subcommand: &str) -> Result<PluginNamedScanArgs, String> {
    let mut options = plugin_discover_options();
    let mut scan_dirs = Vec::new();
    let mut name = None;
    let mut json = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--json" => json = true,
            "--scan-dir" | "--root" => { scan_dirs.push(plugin_take_path(argv, index, argv[index].as_str())?); index += 1; }
            "--disabled" => { options.disabled_plugins.push(plugin_take_value(argv, index, "--disabled")?); index += 1; }
            other if !other.starts_with('-') && name.is_none() => name = Some(other.to_owned()),
            other => return Err(format!("plugin {subcommand}: unknown argument {other}")),
        }
        index += 1;
    }
    if !scan_dirs.is_empty() { options.scan_dirs = scan_dirs; }
    plugin_add_registry_disabled(&mut options);
    let name = plugin_validate_name(&name.ok_or_else(|| format!("plugin {subcommand}: name is required"))?)?;
    Ok(PluginNamedScanArgs { name, options, json })
}

fn plugin_enable(argv: &[String]) -> Result<CliOutput, String> {
    let toggle = plugin_parse_toggle(argv, true)?;
    let mut disabled = plugin_read_disabled(&toggle.root);
    let before = disabled.len();
    disabled.retain(|name| !toggle.names.contains(name));
    plugin_write_disabled(&toggle.root, &disabled)?;
    Ok(plugin_ok(&format!("enabled {} plugin{} ({} changed)", toggle.names.len(), plugin_plural(toggle.names.len()), before - disabled.len())))
}

fn plugin_disable(argv: &[String]) -> Result<CliOutput, String> {
    let toggle = plugin_parse_toggle(argv, false)?;
    let mut disabled = plugin_read_disabled(&toggle.root);
    for name in &toggle.names {
        if !disabled.contains(name) { disabled.push(name.clone()); }
    }
    disabled.sort();
    plugin_write_disabled(&toggle.root, &disabled)?;
    Ok(plugin_ok(&format!("disabled {} plugin{}", toggle.names.len(), plugin_plural(toggle.names.len()))))
}

struct PluginToggleArgs { root: std::path::PathBuf, names: Vec<String> }

fn plugin_parse_toggle(argv: &[String], many: bool) -> Result<PluginToggleArgs, String> {
    let mut root = None;
    let mut names = Vec::new();
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--root" | "--scan-dir" => { root = Some(plugin_take_path(argv, index, argv[index].as_str())?); index += 1; }
            other if !other.starts_with('-') => names.push(plugin_validate_name(other)?),
            other => return Err(format!("plugin toggle: unknown argument {other}")),
        }
        index += 1;
    }
    if names.is_empty() { return Err("plugin toggle: name is required".to_owned()); }
    if !many && names.len() != 1 { return Err("plugin disable: expected exactly one name".to_owned()); }
    Ok(PluginToggleArgs { root: root.unwrap_or_else(plugin_default_root), names })
}

fn plugin_remove(argv: &[String]) -> Result<CliOutput, String> {
    let removal = plugin_parse_remove(argv)?;
    let plugin = plugin_find_loaded(&removal.name, &removal.options)?;
    let archive = plugin_archive_dir(&removal.archive_root, &removal.name);
    std::fs::create_dir_all(&removal.archive_root).map_err(|error| format!("plugin remove: archive root failed: {error}"))?;
    std::fs::rename(&plugin.dir, &archive).map_err(|error| format!("plugin remove: archive failed: {error}"))?;
    Ok(plugin_ok(&format!("removed {} -> {}", removal.name, path_string(&archive))))
}

struct PluginRemoveArgs { name: String, options: DiscoverPackagesOptions, archive_root: std::path::PathBuf }

fn plugin_parse_remove(argv: &[String]) -> Result<PluginRemoveArgs, String> {
    let mut options = plugin_discover_options();
    let mut scan_dirs = Vec::new();
    let mut archive_root = std::env::temp_dir();
    let mut name = None;
    let mut yes = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--yes" | "-y" => yes = true,
            "--scan-dir" | "--root" => { scan_dirs.push(plugin_take_path(argv, index, argv[index].as_str())?); index += 1; }
            "--archive-root" => { archive_root = plugin_take_path(argv, index, "--archive-root")?; index += 1; }
            other if !other.starts_with('-') && name.is_none() => name = Some(other.to_owned()),
            other => return Err(format!("plugin remove: unknown argument {other}")),
        }
        index += 1;
    }
    if !yes { return Err("plugin remove: refusing without --yes".to_owned()); }
    if !scan_dirs.is_empty() { options.scan_dirs = scan_dirs; }
    let name = plugin_validate_name(&name.ok_or_else(|| "plugin remove: name is required".to_owned())?)?;
    Ok(PluginRemoveArgs { name, options, archive_root })
}

fn plugin_find_loaded(name: &str, options: &DiscoverPackagesOptions) -> Result<LoadedPlugin, String> {
    discover_packages(options).plugins.into_iter().find(|plugin| plugin.manifest.name == name).ok_or_else(|| format!("plugin '{name}' not found"))
}

fn plugin_render_info(plugin: &LoadedPlugin, json: bool) -> String {
    if json { return plugin_info_json(plugin); }
    let manifest = &plugin.manifest;
    format!("{}@{}\n  tier: {}\n  kind: {}\n  disabled: {}\n  dir: {}\n  entry: {}\n  wasm: {}\n", manifest.name, manifest.version, manifest.tier.unwrap_or(PluginTier::Core).as_str(), plugin.kind.as_str(), plugin.disabled, path_string(&plugin.dir), plugin.entry_path.as_ref().map_or_else(|| "-".to_owned(), path_string), if plugin.wasm_path.as_os_str().is_empty() { "-".to_owned() } else { path_string(&plugin.wasm_path) })
}

fn plugin_info_json(plugin: &LoadedPlugin) -> String {
    let manifest = &plugin.manifest;
    format!("{{\"name\":{},\"version\":{},\"tier\":{},\"kind\":{},\"disabled\":{},\"dir\":{},\"entryPath\":{},\"wasmPath\":{}}}\n", json_string(&manifest.name), json_string(&manifest.version), json_string(manifest.tier.unwrap_or(PluginTier::Core).as_str()), json_string(plugin.kind.as_str()), plugin.disabled, json_string(&path_string(&plugin.dir)), plugin.entry_path.as_ref().map_or_else(|| "null".to_owned(), |path| json_string(&path_string(path))), if plugin.wasm_path.as_os_str().is_empty() { "null".to_owned() } else { json_string(&path_string(&plugin.wasm_path)) })
}

#[derive(Debug, Clone)]
struct PluginBuildArgs { dir: std::path::PathBuf, watch: bool }

#[derive(Debug, Clone, PartialEq, Eq)]
enum PluginProjectKind {
    RustWasm { name: String, wasm: String },
    TsAssemblyScript { name: String, entry: String, export: String },
    UnsupportedWasm(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginCargoOutput { status: i32, stdout: String, stderr: String }

trait PluginBuildRunner {
    fn plugin_run_cargo(&mut self, dir: &std::path::Path, args: &[String]) -> Result<PluginCargoOutput, String>;
    fn plugin_run_assemblyscript(
        &mut self,
        sdk_dir: &std::path::Path,
        dir: &std::path::Path,
        entry_path: &std::path::Path,
        output_path: &std::path::Path,
    ) -> Result<PluginCargoOutput, String>;
    fn plugin_after_build_watch(&mut self, _dir: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct PluginRealBuildRunner;

impl PluginBuildRunner for PluginRealBuildRunner {
    fn plugin_run_cargo(&mut self, dir: &std::path::Path, args: &[String]) -> Result<PluginCargoOutput, String> {
        let child = std::process::Command::new("cargo")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("plugin build: failed to run cargo: {error}"))?;
        plugin_wait_for_build_tool("cargo", child)
    }

    fn plugin_run_assemblyscript(
        &mut self,
        sdk_dir: &std::path::Path,
        dir: &std::path::Path,
        entry_path: &std::path::Path,
        output_path: &std::path::Path,
    ) -> Result<PluginCargoOutput, String> {
        let asc = plugin_assemblyscript_compiler_path(sdk_dir);
        if !asc.is_file() {
            return Err(plugin_assemblyscript_missing_error(sdk_dir, &asc));
        }
        plugin_ensure_sdk_self_link(sdk_dir)?;
        let args = plugin_assemblyscript_args(sdk_dir, dir, entry_path, output_path);
        let child = std::process::Command::new(&asc)
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("plugin build: failed to run AssemblyScript compiler: {error}"))?;
        plugin_wait_for_build_tool("AssemblyScript compiler", child)
    }
}

fn plugin_wait_for_build_tool(tool: &str, mut child: std::process::Child) -> Result<PluginCargoOutput, String> {
    let timeout = plugin_build_timeout();
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                let output = child
                    .wait_with_output()
                    .map_err(|error| format!("plugin build: failed to collect {tool} output: {error}"))?;
                return Ok(PluginCargoOutput {
                    status: output.status.code().unwrap_or(1),
                    stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                    stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                });
            }
            Ok(None) if start.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Ok(PluginCargoOutput {
                    status: 124,
                    stdout: String::new(),
                    stderr: format!("{tool} timed out after {}ms", timeout.as_millis()),
                });
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("plugin build: failed to wait for {tool}: {error}"));
            }
        }
    }
}

fn plugin_build_timeout() -> std::time::Duration {
    std::env::var("MAW_PLUGIN_BUILD_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|millis| (1_000..=900_000).contains(millis))
        .map_or_else(|| std::time::Duration::from_mins(5), std::time::Duration::from_millis)
}

fn plugin_build_or_dev(kind: &str, argv: &[String]) -> Result<CliOutput, String> {
    plugin_build_or_dev_with_runner(kind, argv, &mut PluginRealBuildRunner)
}

fn plugin_build_or_dev_with_runner(kind: &str, argv: &[String], runner: &mut impl PluginBuildRunner) -> Result<CliOutput, String> {
    let parsed = plugin_parse_build_args(kind, argv)?;
    match plugin_detect_project_kind(&parsed.dir)? {
        PluginProjectKind::RustWasm { name, wasm } => plugin_build_rust_wasm(kind, &parsed, &name, &wasm, runner),
        PluginProjectKind::TsAssemblyScript { name, entry, export } => {
            plugin_build_ts_assemblyscript(kind, &parsed, &name, &entry, &export, runner)
        }
        PluginProjectKind::UnsupportedWasm(message) => Err(message),
    }
}

fn plugin_parse_build_args(kind: &str, argv: &[String]) -> Result<PluginBuildArgs, String> {
    let mut dir = None;
    let mut watch = kind == "dev";
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--watch" if kind == "build" => watch = true,
            "--types" => {}
            "--" => return Err(format!("plugin {kind}: -- separator is not allowed")),
            value if value.starts_with('-') => return Err(format!("plugin {kind}: unknown argument {value}")),
            value if dir.is_none() => dir = Some(plugin_validate_build_dir(value)?),
            other => return Err(format!("plugin {kind}: unexpected argument {other}")),
        }
        index += 1;
    }
    let dir = match dir {
        Some(path) => path,
        None => std::env::current_dir().map_err(|error| format!("plugin {kind}: current dir failed: {error}"))?,
    };
    Ok(PluginBuildArgs { dir, watch })
}

fn plugin_detect_project_kind(dir: &std::path::Path) -> Result<PluginProjectKind, String> {
    let manifest_path = dir.join("plugin.json");
    if !manifest_path.exists() { return Err(format!("no plugin.json in {}", dir.display())); }
    let text = std::fs::read_to_string(&manifest_path).map_err(|error| format!("invalid plugin.json: {error}"))?;
    let raw: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("invalid plugin.json: {error}"))?;
    let name = raw.get("name").and_then(serde_json::Value::as_str).unwrap_or("plugin").to_owned();
    let target = raw.get("target").and_then(serde_json::Value::as_str);
    let source_entry = plugin_source_manifest_entry(&raw)?;
    let has_js_artifact = raw
        .get("artifact")
        .and_then(|value| value.get("path"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|path| !plugin_path_has_wasm_extension(path));
    let wasm = raw.get("wasm").and_then(serde_json::Value::as_str);
    if target == Some("js") || source_entry.is_some() || has_js_artifact {
        let entry = source_entry.ok_or_else(|| {
            "plugin build: JS artifact manifests need a .ts source entry for the AssemblyScript ship-tier build".to_owned()
        })?;
        let export = plugin_manifest_entry_export(&raw)?;
        return Ok(PluginProjectKind::TsAssemblyScript { name, entry, export });
    }
    let Some(wasm) = wasm else {
        return Ok(PluginProjectKind::UnsupportedWasm(
            "plugin build: manifest needs either a .ts source entry or a Rust wasm path".to_owned(),
        ));
    };
    if !dir.join("Cargo.toml").exists() {
        return Ok(PluginProjectKind::UnsupportedWasm("plugin build: wasm project is not a Rust cargo plugin yet (#70-out)".to_owned()));
    }
    plugin_validate_wasm_manifest_path(wasm)?;
    Ok(PluginProjectKind::RustWasm { name, wasm: wasm.to_owned() })
}

fn plugin_source_manifest_entry(raw: &serde_json::Value) -> Result<Option<String>, String> {
    let Some(entry) = raw.get("entry") else { return Ok(None); };
    if let Some(path) = entry.as_str() {
        if plugin_path_has_wasm_extension(path) {
            return Ok(None);
        }
        plugin_validate_ts_entry_manifest_path(path)?;
        return Ok(Some(path.to_owned()));
    }
    if entry.as_object().is_some() {
        return Ok(None);
    }
    Err("plugin build: entry must be a string source path or wasm entry object".to_owned())
}

fn plugin_manifest_entry_export(raw: &serde_json::Value) -> Result<String, String> {
    let Some(entry) = raw.get("entry").and_then(serde_json::Value::as_object) else {
        return Ok("handle".to_owned());
    };
    let Some(export) = entry.get("export") else {
        return Ok("handle".to_owned());
    };
    export
        .as_str()
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| "plugin build: entry.export must be a non-empty string".to_owned())
}

fn plugin_build_ts_assemblyscript(
    kind: &str,
    parsed: &PluginBuildArgs,
    _name: &str,
    entry: &str,
    export: &str,
    runner: &mut impl PluginBuildRunner,
) -> Result<CliOutput, String> {
    let entry_path = parsed.dir.join(entry);
    if !entry_path.is_file() {
        return Err(format!("plugin {kind}: TS entry not found: {}", entry_path.display()));
    }
    let sdk_dir = plugin_wasm_sdk_dir()?;
    let build_dir = parsed.dir.join(".maw-build");
    std::fs::create_dir_all(&build_dir).map_err(|error| format!("plugin {kind}: build dir create failed: {error}"))?;
    let wasm_path = build_dir.join("plugin.wasm");
    let _ = std::fs::remove_file(&wasm_path);
    let output = runner.plugin_run_assemblyscript(&sdk_dir, &parsed.dir, &entry_path, &wasm_path)?;
    if output.status != 0 {
        return Err(format!(
            "plugin {kind}: AssemblyScript build failed{}",
            plugin_assemblyscript_failure_detail(&output)
        ));
    }
    if !wasm_path.is_file() {
        return Err(format!("plugin {kind}: AssemblyScript output missing: {}", wasm_path.display()));
    }
    let artifact = plugin_emit_ts_wasm_ship_artifact(&parsed.dir, &wasm_path, export)?;
    if parsed.watch { runner.plugin_after_build_watch(&parsed.dir)?; }
    let digest = artifact.sha256.strip_prefix("sha256:").unwrap_or(&artifact.sha256);
    let mut stdout = format!(
        "ship tier ready: plugin.wasm (sha256 {digest}) — remove \"runtime\": \"bun-dev\" or leave it as dev fallback\n"
    );
    if parsed.watch { stdout.push_str("  watch: bounded one-shot\n"); }
    Ok(CliOutput { code: 0, stdout, stderr: String::new() })
}

fn plugin_build_rust_wasm(kind: &str, parsed: &PluginBuildArgs, name: &str, wasm: &str, runner: &mut impl PluginBuildRunner) -> Result<CliOutput, String> {
    let cargo_args = plugin_cargo_build_args();
    let output = runner.plugin_run_cargo(&parsed.dir, &cargo_args)?;
    if output.status != 0 {
        return Err(format!("plugin {kind}: cargo build failed{}", plugin_cargo_failure_detail(&output)));
    }
    let wasm_path = parsed.dir.join(wasm);
    if !wasm_path.is_file() { return Err(format!("plugin {kind}: wasm output missing: {wasm}")); }
    let artifact = plugin_emit_wasm_dist(&parsed.dir, &wasm_path)?;
    if parsed.watch { runner.plugin_after_build_watch(&parsed.dir)?; }
    let mut stdout = format!(
        "built Rust WASM plugin {name}\n  target: wasm32-unknown-unknown\n  wasm: {wasm}\n  dist: {}\n  sha256: {}\n",
        "dist/plugin.wasm",
        artifact.sha256
    );
    if parsed.watch { stdout.push_str("  watch: bounded one-shot\n"); }
    Ok(CliOutput { code: 0, stdout, stderr: String::new() })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginWasmDistArtifact {
    sha256: String,
}

fn plugin_emit_ts_wasm_ship_artifact(dir: &std::path::Path, wasm_path: &std::path::Path, export: &str) -> Result<PluginWasmDistArtifact, String> {
    let manifest_path = dir.join("plugin.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|error| format!("invalid plugin.json: {error}"))?;
    let mut raw: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("invalid plugin.json: {error}"))?;
    let object = raw
        .as_object_mut()
        .ok_or_else(|| "plugin.json: manifest root must be an object".to_owned())?;
    let bundle_path = dir.join("plugin.wasm");
    std::fs::copy(wasm_path, &bundle_path).map_err(|error| format!("plugin build: copy wasm failed: {error}"))?;
    let sha256 = hash_file(&bundle_path).map_err(|error| format!("plugin build: wasm hash failed: {error}"))?;
    object.insert("target".to_owned(), serde_json::json!("wasm"));
    object.insert("wasm".to_owned(), serde_json::json!("./plugin.wasm"));
    object.insert(
        "entry".to_owned(),
        serde_json::json!({"kind":"wasm","path":"plugin.wasm","export":export}),
    );
    object.insert(
        "artifact".to_owned(),
        serde_json::json!({"path":"./plugin.wasm","sha256":sha256}),
    );
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&raw)
            .map_err(|error| format!("plugin build: plugin.json serialize failed: {error}"))?
            + "\n",
    )
    .map_err(|error| format!("plugin build: plugin.json write failed: {error}"))?;
    Ok(PluginWasmDistArtifact { sha256 })
}

fn plugin_emit_wasm_dist(dir: &std::path::Path, wasm_path: &std::path::Path) -> Result<PluginWasmDistArtifact, String> {
    let manifest_path = dir.join("plugin.json");
    let text = std::fs::read_to_string(&manifest_path).map_err(|error| format!("invalid plugin.json: {error}"))?;
    let mut raw: serde_json::Value = serde_json::from_str(&text).map_err(|error| format!("invalid plugin.json: {error}"))?;
    let export = raw
        .get("entry")
        .and_then(|entry| entry.get("export"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("handle")
        .to_owned();
    let object = raw
        .as_object_mut()
        .ok_or_else(|| "plugin.json: manifest root must be an object".to_owned())?;
    let dist_dir = dir.join("dist");
    std::fs::create_dir_all(&dist_dir).map_err(|error| format!("plugin build: dist create failed: {error}"))?;
    let bundle_path = dist_dir.join("plugin.wasm");
    std::fs::copy(wasm_path, &bundle_path).map_err(|error| format!("plugin build: copy wasm failed: {error}"))?;
    let sha256 = hash_file(&bundle_path).map_err(|error| format!("plugin build: wasm hash failed: {error}"))?;
    object.insert("target".to_owned(), serde_json::json!("wasm"));
    object.insert("wasm".to_owned(), serde_json::json!("plugin.wasm"));
    object.insert(
        "entry".to_owned(),
        serde_json::json!({"kind":"wasm","path":"plugin.wasm","export":export}),
    );
    object.insert(
        "artifact".to_owned(),
        serde_json::json!({"path":"./plugin.wasm","sha256":sha256}),
    );
    let dist_manifest_path = dist_dir.join("plugin.json");
    std::fs::write(
        &dist_manifest_path,
        serde_json::to_string_pretty(&raw)
            .map_err(|error| format!("plugin build: dist plugin.json serialize failed: {error}"))?
            + "
",
    )
    .map_err(|error| format!("plugin build: dist plugin.json write failed: {error}"))?;
    Ok(PluginWasmDistArtifact { sha256 })
}

fn plugin_cargo_build_args() -> Vec<String> {
    ["build", "--release", "--target", "wasm32-unknown-unknown"].iter().map(|value| (*value).to_owned()).collect()
}

fn plugin_assemblyscript_args(
    sdk_dir: &std::path::Path,
    dir: &std::path::Path,
    entry_path: &std::path::Path,
    output_path: &std::path::Path,
) -> Vec<String> {
    let entry_arg = entry_path.strip_prefix(dir).unwrap_or(entry_path);
    let output_arg = output_path.strip_prefix(dir).unwrap_or(output_path);
    let abort_export = entry_arg.with_extension("");
    // Anchor bare-import resolution at the pinned SDK's node_modules so a plugin's
    // .ts can `import ... from "@maw-rs/wasm-sdk"` (or the "@extism/as-pdk" it
    // re-exports) with no per-plugin install. asc walks --path like node_modules;
    // plugin_ensure_sdk_self_link makes @maw-rs/wasm-sdk resolve to the SDK itself.
    let sdk_modules = sdk_dir.join("node_modules");
    vec![
        path_string(entry_arg),
        "--outFile".to_owned(),
        path_string(output_arg),
        "--path".to_owned(),
        path_string(&sdk_modules),
        "--runtime".to_owned(),
        "stub".to_owned(),
        "--use".to_owned(),
        format!("abort={}/myAbort", path_string(&abort_export)),
        "--exportRuntime".to_owned(),
        "--optimizeLevel".to_owned(),
        "3".to_owned(),
        "--shrinkLevel".to_owned(),
        "0".to_owned(),
        "--converge".to_owned(),
        "--noAssert".to_owned(),
    ]
}

fn plugin_wasm_sdk_dir() -> Result<std::path::PathBuf, String> {
    if let Some(path) = std::env::var_os("MAW_WASM_SDK_DIR").map(std::path::PathBuf::from) {
        return plugin_validate_wasm_sdk_dir(path);
    }
    plugin_validate_wasm_sdk_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("packages")
            .join("wasm-sdk"),
    )
}

fn plugin_validate_wasm_sdk_dir(path: std::path::PathBuf) -> Result<std::path::PathBuf, String> {
    if path.join("package.json").is_file() {
        Ok(path)
    } else {
        Err(format!(
            "plugin build: WASM SDK toolchain missing: expected {}\nset MAW_WASM_SDK_DIR to packages/wasm-sdk from this repo",
            path_string(path.join("package.json"))
        ))
    }
}

fn plugin_assemblyscript_compiler_path(sdk_dir: &std::path::Path) -> std::path::PathBuf {
    if cfg!(windows) {
        sdk_dir.join("node_modules").join(".bin").join("asc.cmd")
    } else {
        sdk_dir.join("node_modules").join(".bin").join("asc")
    }
}

fn plugin_assemblyscript_missing_error(sdk_dir: &std::path::Path, asc: &std::path::Path) -> String {
    format!(
        "plugin build: AssemblyScript compiler not found: {}\ninstall it with: npm ci --prefix {}",
        path_string(asc),
        path_string(sdk_dir)
    )
}

// Link the pinned SDK into its own node_modules as @maw-rs/wasm-sdk so `asc --path
// <sdk>/node_modules` resolves that bare import to the SDK source (asc keys package
// resolution on the scoped directory name, which only the SDK dir name lacks). The
// link stays inside the pinned SDK dir — no network, no floating resolution — and is
// idempotent so repeated builds are cheap.
fn plugin_ensure_sdk_self_link(sdk_dir: &std::path::Path) -> Result<(), String> {
    let scope = sdk_dir.join("node_modules").join("@maw-rs");
    let link = scope.join("wasm-sdk");
    if link.exists() {
        return Ok(());
    }
    // Clear a stale/broken link so the create below stays idempotent.
    let _ = std::fs::remove_file(&link);
    std::fs::create_dir_all(&scope)
        .map_err(|error| format!("plugin build: wasm-sdk resolution dir create failed: {error}"))?;
    plugin_symlink_dir(sdk_dir, &link).map_err(|error| {
        format!(
            "plugin build: wasm-sdk self-link failed: {error}\nexpected link {}",
            path_string(&link)
        )
    })
}

#[cfg(unix)]
fn plugin_symlink_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn plugin_symlink_dir(target: &std::path::Path, link: &std::path::Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_dir(target, link)
}

fn plugin_cargo_failure_detail(output: &PluginCargoOutput) -> String {
    let detail = if output.stderr.trim().is_empty() { output.stdout.trim() } else { output.stderr.trim() };
    if detail.is_empty() { String::new() } else { format!(": {detail}") }
}

fn plugin_assemblyscript_failure_detail(output: &PluginCargoOutput) -> String {
    let detail = if output.stderr.trim().is_empty() { output.stdout.trim() } else { output.stderr.trim() };
    if detail.is_empty() {
        format!(": {PLUGIN_AS_TS_BOUNDARY}")
    } else {
        format!(": {detail}\n{PLUGIN_AS_TS_BOUNDARY}")
    }
}

fn plugin_validate_build_dir(value: &str) -> Result<std::path::PathBuf, String> {
    if value.trim() != value || value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("plugin build: dir must be non-empty, unpadded, not start with '-', and contain no control characters".to_owned());
    }
    let path = std::path::PathBuf::from(value);
    if path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err("plugin build: dir must not contain .. segments".to_owned());
    }
    path.canonicalize().map_err(|error| format!("plugin build: invalid dir: {error}"))
}

fn plugin_path_has_wasm_extension(value: &str) -> bool {
    std::path::Path::new(value)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
}

fn plugin_path_has_ts_extension(value: &str) -> bool {
    std::path::Path::new(value)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("ts"))
}

fn plugin_validate_ts_entry_manifest_path(value: &str) -> Result<(), String> {
    if value.trim() != value || value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("plugin build: TS entry must be non-empty, unpadded, not start with '-', and contain no control characters".to_owned());
    }
    let path = std::path::Path::new(value);
    if path.is_absolute() || path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err("plugin build: TS entry must be relative and stay inside plugin dir".to_owned());
    }
    if plugin_path_has_ts_extension(value) {
        Ok(())
    } else {
        Err(format!(
            "plugin build: AssemblyScript ship-tier builds require a .ts entry; JS entry needs Javy (`cargo install javy`) or prebuilt WASM\n{PLUGIN_AS_TS_BOUNDARY}"
        ))
    }
}

fn plugin_validate_wasm_manifest_path(value: &str) -> Result<(), String> {
    if value.trim() != value || value.is_empty() || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("plugin build: wasm path must be non-empty, unpadded, not start with '-', and contain no control characters".to_owned());
    }
    let path = std::path::Path::new(value);
    if path.is_absolute() || path.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
        return Err("plugin build: wasm path must be relative and stay inside plugin dir".to_owned());
    }
    let normalized = value.strip_prefix("./").unwrap_or(value);
    if !normalized.starts_with("target/wasm32-unknown-unknown/release/") || !plugin_path_has_wasm_extension(normalized) {
        return Err("plugin build: Rust wasm path must target wasm32-unknown-unknown release output".to_owned());
    }
    Ok(())
}

fn plugin_init_summary_json(summary: &maw_plugin_manifest::PluginInitSummary) -> String {
    format!("{{\"command\":\"plugin\",\"kind\":\"create\",\"name\":{},\"dir\":{},\"manifestPath\":{},\"entryPath\":{}}}\n", json_string(&summary.name), json_string(&path_string(&summary.dir)), json_string(&path_string(&summary.manifest_path)), json_string(&path_string(&summary.entry_path)))
}

fn plugin_discover_options() -> DiscoverPackagesOptions {
    DiscoverPackagesOptions { runtime_version: "1.0.0".to_owned(), ..DiscoverPackagesOptions::default() }
}

fn plugin_add_registry_disabled(options: &mut DiscoverPackagesOptions) {
    if let Some(root) = options.scan_dirs.first() { options.disabled_plugins.extend(plugin_read_disabled(root)); }
}

fn plugin_read_disabled(root: &std::path::Path) -> Vec<String> {
    let path = plugin_disabled_path(root);
    let Ok(text) = std::fs::read_to_string(path) else { return Vec::new(); };
    serde_json::from_str::<Vec<String>>(&text).unwrap_or_default().into_iter().filter(|name| plugin_validate_name(name).is_ok()).collect()
}

fn plugin_write_disabled(root: &std::path::Path, names: &[String]) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|error| format!("plugin toggle: root failed: {error}"))?;
    let text = serde_json::to_string_pretty(names).map_err(|error| format!("plugin toggle: serialize failed: {error}"))? + "\n";
    std::fs::write(plugin_disabled_path(root), text).map_err(|error| format!("plugin toggle: write failed: {error}"))
}

fn plugin_disabled_path(root: &std::path::Path) -> std::path::PathBuf { root.join(".disabled.json") }

fn plugin_archive_dir(root: &std::path::Path, name: &str) -> std::path::PathBuf {
    root.join(format!("maw-plugin-{name}-{}", now_iso_utc()))
}

fn plugin_default_root() -> std::path::PathBuf { maw_data_path(&real_xdg_env(), &["plugins"]) }

fn plugin_validate_name(value: &str) -> Result<String, String> {
    if value.is_empty() || value.starts_with('-') || value == "--" || value.chars().any(char::is_whitespace) { return Err(format!("plugin: invalid plugin name {value:?}")); }
    Ok(value.to_owned())
}

fn plugin_take_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    argv.get(index + 1).filter(|value| !value.starts_with('-')).cloned().ok_or_else(|| format!("plugin: missing {flag} value"))
}

fn plugin_take_path(argv: &[String], index: usize, flag: &str) -> Result<std::path::PathBuf, String> {
    Ok(std::path::PathBuf::from(plugin_take_value(argv, index, flag)?))
}

fn plugin_plural(count: usize) -> &'static str { if count == 1 { "" } else { "s" } }

fn plugin_ok(message: &str) -> CliOutput { CliOutput { code: 0, stdout: format!("{message}\n"), stderr: String::new() } }

fn plugin_error(code: i32, message: &str) -> CliOutput { CliOutput { code, stdout: String::new(), stderr: format!("{message}\n{PLUGIN_USAGE}\n") } }

#[cfg(test)]
mod plugin_native_tests {
    use super::{
        path_string, plugin_assemblyscript_args, plugin_build_or_dev_with_runner,
        plugin_cargo_build_args, plugin_run_command, plugin_wasm_sdk_dir, PluginBuildRunner,
        PluginCargoOutput, DISPATCH_102,
    };
    use std::path::{Path, PathBuf};

    fn plugin_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn plugin_temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("maw-rs-plugin-native-{label}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("temp root");
        root
    }

    fn plugin_write(root: &Path, name: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).expect("plugin dir");
        std::fs::write(dir.join("index.ts"), "export function handle() {}\n").expect("entry");
        std::fs::write(dir.join("plugin.json"), format!(r#"{{"name":"{name}","version":"1.0.0","sdk":"*","entry":"index.ts","cli":{{"command":"{name}"}}}}"#)).expect("manifest");
    }

    fn plugin_write_rust(root: &Path, name: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("target/wasm32-unknown-unknown/release")).expect("target");
        std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"route_probe\"\nversion=\"0.1.0\"\nedition=\"2021\"\n").expect("cargo");
        std::fs::write(
            dir.join("plugin.json"),
            format!(r#"{{"name":"{name}","version":"0.1.0","sdk":"*","wasm":"./target/wasm32-unknown-unknown/release/route_probe.wasm"}}"#),
        ).expect("manifest");
        dir
    }

    #[derive(Debug, Default)]
    struct FakeBuildRunner {
        cargo_calls: Vec<Vec<String>>,
        assemblyscript_calls: Vec<Vec<String>>,
        assemblyscript_error: Option<String>,
        watched: bool,
    }

    impl PluginBuildRunner for FakeBuildRunner {
        fn plugin_run_cargo(&mut self, dir: &Path, args: &[String]) -> Result<PluginCargoOutput, String> {
            self.cargo_calls.push(args.to_vec());
            std::fs::write(dir.join("target/wasm32-unknown-unknown/release/route_probe.wasm"), b"\0asm").expect("fake wasm");
            Ok(PluginCargoOutput { status: 0, stdout: String::new(), stderr: String::new() })
        }

        fn plugin_run_assemblyscript(
            &mut self,
            sdk_dir: &Path,
            dir: &Path,
            entry_path: &Path,
            output_path: &Path,
        ) -> Result<PluginCargoOutput, String> {
            self.assemblyscript_calls
                .push(plugin_assemblyscript_args(sdk_dir, dir, entry_path, output_path));
            if let Some(error) = &self.assemblyscript_error {
                return Err(error.clone());
            }
            std::fs::write(output_path, b"\0asm").expect("fake wasm");
            Ok(PluginCargoOutput { status: 0, stdout: String::new(), stderr: String::new() })
        }

        fn plugin_after_build_watch(&mut self, _dir: &Path) -> Result<(), String> {
            self.watched = true;
            Ok(())
        }
    }

    #[test]
    fn plugin_dispatch_registers_scope_split_command() {
        assert_eq!(DISPATCH_102.len(), 1);
        assert_eq!(DISPATCH_102[0].command, "plugin");
    }

    #[test]
    fn plugin_management_ls_and_info_are_full_native() {
        let root = plugin_temp_root("info");
        plugin_write(&root, "alpha");
        let ls = plugin_run_command(&plugin_args(&["ls", "--scan-dir", &root.display().to_string()]));
        assert_eq!(ls.code, 0, "{}", ls.stderr);
        assert!(ls.stdout.contains("1 plugin (1 active, 0 disabled)"));
        let info = plugin_run_command(&plugin_args(&["info", "alpha", "--scan-dir", &root.display().to_string()]));
        assert_eq!(info.code, 0, "{}", info.stderr);
        assert!(info.stdout.contains("alpha@1.0.0"));
        assert!(info.stdout.contains("kind: ts"));
    }

    #[test]
    fn plugin_build_ts_assemblyscript_emits_ship_manifest_and_output() {
        let root = plugin_temp_root("ts-build");
        plugin_write(&root, "alpha");
        let dir = root.join("alpha");
        let canonical_dir = dir.canonicalize().expect("canonical plugin dir");
        let mut runner = FakeBuildRunner::default();
        let out = plugin_build_or_dev_with_runner("build", &plugin_args(&[&dir.display().to_string()]), &mut runner).expect("build");
        assert!(runner.cargo_calls.is_empty(), "TS build must not call cargo");
        let sdk_dir = plugin_wasm_sdk_dir().expect("pinned wasm-sdk dir");
        assert_eq!(
            runner.assemblyscript_calls,
            vec![plugin_assemblyscript_args(
                &sdk_dir,
                &canonical_dir,
                &canonical_dir.join("index.ts"),
                &canonical_dir.join(".maw-build").join("plugin.wasm"),
            )]
        );
        let args = &runner.assemblyscript_calls[0];
        let path_index = args.iter().position(|arg| arg == "--path").expect("asc --path present");
        assert_eq!(
            args[path_index + 1],
            path_string(sdk_dir.join("node_modules")),
            "asc must resolve bare imports against the pinned SDK node_modules"
        );
        assert!(out.stdout.starts_with("ship tier ready: plugin.wasm (sha256 "), "{}", out.stdout);
        assert!(
            out.stdout.contains("remove \"runtime\": \"bun-dev\" or leave it as dev fallback"),
            "{}",
            out.stdout
        );
        assert!(out.stderr.is_empty());
        assert!(dir.join("plugin.wasm").is_file());
        let manifest = std::fs::read_to_string(dir.join("plugin.json")).expect("manifest");
        assert!(manifest.contains(r#""target": "wasm""#), "{manifest}");
        assert!(manifest.contains(r#""kind": "wasm""#), "{manifest}");
        assert!(manifest.contains(r#""path": "plugin.wasm""#), "{manifest}");
        assert!(manifest.contains(r#""sha256": "sha256:"#), "{manifest}");
    }

    #[test]
    fn plugin_build_ts_missing_toolchain_error_is_actionable() {
        let root = plugin_temp_root("ts-missing-toolchain");
        plugin_write(&root, "alpha");
        let dir = root.join("alpha");
        let mut runner = FakeBuildRunner {
            assemblyscript_error: Some(
                "plugin build: AssemblyScript compiler not found: /sdk/node_modules/.bin/asc\ninstall it with: npm ci --prefix /sdk"
                    .to_owned(),
            ),
            ..FakeBuildRunner::default()
        };
        let err = plugin_build_or_dev_with_runner("build", &plugin_args(&[&dir.display().to_string()]), &mut runner)
            .expect_err("missing toolchain");
        assert!(err.contains("AssemblyScript compiler not found"), "{err}");
        assert!(err.contains("npm ci --prefix"), "{err}");
        assert!(runner.cargo_calls.is_empty(), "missing TS toolchain must not call cargo");
    }

    #[test]
    fn plugin_create_rust_flag_scaffolds_rust_wasm_plugin_without_delegation() {
        let root = plugin_temp_root("create-rust");
        let dir = root.join("route-probe");
        let out = plugin_run_command(&plugin_args(&[
            "create",
            "--rust",
            "route-probe",
            "--dir",
            &dir.display().to_string(),
        ]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(
            out.stdout,
            include_str!("../../tests/fixtures/native-plugin-create/plugin-create-rust.stdout")
                .replace("<DIR>", &path_string(&dir))
        );
        assert!(out.stderr.is_empty());
        assert!(!out.stdout.contains("DELEGATED-MAW"));

        let cargo = std::fs::read_to_string(dir.join("Cargo.toml")).expect("Cargo.toml");
        assert!(cargo.contains(r#"name = "route-probe""#), "{cargo}");
        assert!(cargo.contains(r#"extism-pdk = "=1.4.1""#), "{cargo}");
        assert!(cargo.contains(r#"crate-type = ["cdylib"]"#), "{cargo}");
        let lib = std::fs::read_to_string(dir.join("src").join("lib.rs")).expect("lib.rs");
        assert!(lib.contains("#[plugin_fn]"), "{lib}");
        let manifest = std::fs::read_to_string(dir.join("plugin.json")).expect("plugin.json");
        assert!(manifest.contains(r#""name": "route-probe""#), "{manifest}");
        assert!(
            manifest.contains(r#""wasm": "./target/wasm32-unknown-unknown/release/route_probe.wasm""#),
            "{manifest}"
        );
        assert!(!manifest.contains("DELEGATED-MAW"));
    }

    #[test]
    fn plugin_build_rust_wasm_uses_cargo_argv_no_shell_and_golden_output() {
        let root = plugin_temp_root("rust-build");
        let dir = plugin_write_rust(&root, "route-probe");
        let mut runner = FakeBuildRunner::default();
        let out = plugin_build_or_dev_with_runner("build", &plugin_args(&[&dir.display().to_string()]), &mut runner).expect("build");
        assert_eq!(runner.cargo_calls, vec![plugin_cargo_build_args()]);
        assert_eq!(
            out.stdout,
            include_str!("../../tests/fixtures/native-plugin-build/plugin-build-rust.stdout")
        );
        let manifest = std::fs::read_to_string(dir.join("dist/plugin.json")).expect("dist manifest");
        assert!(manifest.contains("\"artifact\""), "{manifest}");
        assert!(manifest.contains("sha256:"), "{manifest}");
        assert!(dir.join("dist/plugin.wasm").is_file());
        assert!(!runner.watched);
    }

    #[test]
    fn plugin_dev_rust_wasm_is_watch_alias_without_ci_hang() {
        let root = plugin_temp_root("rust-dev");
        let dir = plugin_write_rust(&root, "route-probe");
        let mut runner = FakeBuildRunner::default();
        let out = plugin_build_or_dev_with_runner("dev", &plugin_args(&[&dir.display().to_string()]), &mut runner).expect("dev");
        assert_eq!(runner.cargo_calls, vec![plugin_cargo_build_args()]);
        assert!(runner.watched);
        assert!(out.stdout.contains("watch: bounded one-shot"));
    }

    #[test]
    fn plugin_build_rejects_bad_paths_before_runner() {
        let root = plugin_temp_root("rust-guard");
        let dir = plugin_write_rust(&root, "route-probe");
        std::fs::write(dir.join("plugin.json"), r#"{"name":"bad","version":"0.1.0","wasm":"../bad.wasm"}"#).expect("bad manifest");
        let mut runner = FakeBuildRunner::default();
        let err = plugin_build_or_dev_with_runner("build", &plugin_args(&[&dir.display().to_string()]), &mut runner).expect_err("guard");
        assert!(err.contains("wasm path must be relative"));
        assert!(runner.cargo_calls.is_empty(), "guard must reject before cargo runner");
    }

    #[test]
    fn plugin_enable_disable_write_temp_registry() {
        let root = plugin_temp_root("toggle");
        let disable = plugin_run_command(&plugin_args(&["disable", "alpha", "--root", &root.display().to_string()]));
        assert_eq!(disable.code, 0, "{}", disable.stderr);
        let text = std::fs::read_to_string(root.join(".disabled.json")).expect("disabled registry");
        assert!(text.contains("alpha"));
        let enable = plugin_run_command(&plugin_args(&["enable", "alpha", "--root", &root.display().to_string()]));
        assert_eq!(enable.code, 0, "{}", enable.stderr);
        assert_eq!(std::fs::read_to_string(root.join(".disabled.json")).expect("registry"), "[]\n");
    }

    #[test]
    fn plugin_remove_validates_and_archives_without_delete() {
        let root = plugin_temp_root("remove");
        let archive = root.join("archive");
        plugin_write(&root, "alpha");
        let refused = plugin_run_command(&plugin_args(&["remove", "alpha", "--scan-dir", &root.display().to_string()]));
        assert_eq!(refused.code, 2);
        assert!(refused.stderr.contains("refusing without --yes"));
        let removed = plugin_run_command(&plugin_args(&["remove", "alpha", "--yes", "--scan-dir", &root.display().to_string(), "--archive-root", &archive.display().to_string()]));
        assert_eq!(removed.code, 0, "{}", removed.stderr);
        assert!(!root.join("alpha").exists());
        assert!(std::fs::read_dir(&archive).expect("archive root").next().is_some());
    }

    #[test]
    fn plugin_guards_reject_leading_dash_and_separator() {
        let bad = plugin_run_command(&plugin_args(&["--", "ls"]));
        assert_eq!(bad.code, 2);
        let bad_name = plugin_run_command(&plugin_args(&["info", "-bad"]));
        assert_eq!(bad_name.code, 2);
        assert!(bad_name.stderr.contains("unknown argument -bad"));
    }
}
