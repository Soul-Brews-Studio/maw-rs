const DISPATCH_102: &[DispatcherEntry] = &[DispatcherEntry {
    command: "plugin",
    handler: Handler::Sync(plugin_run_command),
}];

const PLUGIN_USAGE: &str = "usage: maw plugin <ls|info|install|remove|enable|disable|init|create|build|dev> [args]\n  ls/list                  list installed plugins\n  info <name>              show manifest and resolved paths\n  install <dir> --root R   install a built plugin directory\n  remove <name> --yes      archive installed plugin directory (Nothing Deleted)\n  enable <name...>         enable plugins in the local disabled registry\n  disable <name>           disable one plugin in the local disabled registry\n  init|create <name> [--rust] create file-only JS or Rust WASM plugin scaffold\n  build [dir] [--watch]    build Rust WASM plugins with cargo; JS/TS refused unless prebuilt WASM\n  dev [dir]                bounded one-build dev alias for Rust WASM plugins";
const PLUGIN_TS_REFUSAL: &str = "plugin build/dev: JS/TS source builds are intentionally deferred in maw-rs to preserve ZERO-BUN (#59); no Bun/JS compiler is vendored and no Bun subprocess fallback is available. Supported path: Rust-WASM plugins via `maw plugin create --rust <name>` then `maw plugin build`, or provide a prebuilt WASM artifact with target=wasm and a relative wasm path in plugin.json.";

fn plugin_run_command(argv: &[String]) -> CliOutput {
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
    TsRefused,
    UnsupportedWasm(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PluginCargoOutput { status: i32, stdout: String, stderr: String }

trait PluginBuildRunner {
    fn plugin_run_cargo(&mut self, dir: &std::path::Path, args: &[String]) -> Result<PluginCargoOutput, String>;
    fn plugin_after_build_watch(&mut self, _dir: &std::path::Path) -> Result<(), String> {
        Ok(())
    }
}

#[derive(Debug, Default)]
struct PluginRealBuildRunner;

impl PluginBuildRunner for PluginRealBuildRunner {
    fn plugin_run_cargo(&mut self, dir: &std::path::Path, args: &[String]) -> Result<PluginCargoOutput, String> {
        let mut child = std::process::Command::new("cargo")
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("plugin build: failed to run cargo: {error}"))?;
        let timeout = plugin_build_timeout();
        let start = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_)) => {
                    let output = child
                        .wait_with_output()
                        .map_err(|error| format!("plugin build: failed to collect cargo output: {error}"))?;
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
                        stderr: format!("cargo build timed out after {}ms", timeout.as_millis()),
                    });
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(50)),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("plugin build: failed to wait for cargo: {error}"));
                }
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
        PluginProjectKind::TsRefused => Err(PLUGIN_TS_REFUSAL.to_owned()),
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
    let has_js_entry = raw.get("entry").and_then(serde_json::Value::as_str).is_some_and(|entry| !plugin_path_has_wasm_extension(entry));
    let has_js_artifact = raw
        .get("artifact")
        .and_then(|value| value.get("path"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|path| !plugin_path_has_wasm_extension(path));
    let wasm = raw.get("wasm").and_then(serde_json::Value::as_str);
    if target == Some("js") || has_js_entry || has_js_artifact { return Ok(PluginProjectKind::TsRefused); }
    let Some(wasm) = wasm else { return Ok(PluginProjectKind::TsRefused); };
    if !dir.join("Cargo.toml").exists() {
        return Ok(PluginProjectKind::UnsupportedWasm("plugin build: wasm project is not a Rust cargo plugin yet (#70-out)".to_owned()));
    }
    plugin_validate_wasm_manifest_path(wasm)?;
    Ok(PluginProjectKind::RustWasm { name, wasm: wasm.to_owned() })
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

fn plugin_cargo_failure_detail(output: &PluginCargoOutput) -> String {
    let detail = if output.stderr.trim().is_empty() { output.stdout.trim() } else { output.stderr.trim() };
    if detail.is_empty() { String::new() } else { format!(": {detail}") }
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
        path_string, plugin_build_or_dev_with_runner, plugin_cargo_build_args, plugin_run_command,
        PluginBuildRunner, PluginCargoOutput, DISPATCH_102,
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
    struct FakeBuildRunner { calls: Vec<Vec<String>>, watched: bool }

    impl PluginBuildRunner for FakeBuildRunner {
        fn plugin_run_cargo(&mut self, dir: &Path, args: &[String]) -> Result<PluginCargoOutput, String> {
            self.calls.push(args.to_vec());
            std::fs::write(dir.join("target/wasm32-unknown-unknown/release/route_probe.wasm"), b"\0asm").expect("fake wasm");
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
    fn plugin_build_and_dev_refuse_js_ts_without_bun_or_delegation() {
        for sub in ["build", "dev"] {
            let root = plugin_temp_root(sub);
            plugin_write(&root, "alpha");
            let out = plugin_run_command(&plugin_args(&[sub, &root.join("alpha").display().to_string()]));
            assert_eq!(out.code, 2);
            assert!(out.stdout.is_empty());
            assert!(out.stderr.contains("JS/TS source builds are intentionally deferred"));
            assert!(!out.stderr.contains("DELEGATED-MAW"));
        }
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
        assert_eq!(runner.calls, vec![plugin_cargo_build_args()]);
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
        assert_eq!(runner.calls, vec![plugin_cargo_build_args()]);
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
        assert!(runner.calls.is_empty(), "guard must reject before cargo runner");
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
