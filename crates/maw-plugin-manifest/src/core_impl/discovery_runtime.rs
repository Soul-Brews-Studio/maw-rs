#[derive(Debug, Default)]
pub struct MvpWasmInvokeRuntime;

impl PluginInvokeRuntime for MvpWasmInvokeRuntime {
    fn invoke_ts(&mut self, _plugin: &LoadedPlugin, _ctx: &InvokeContext) -> InvokeResult {
        InvokeResult::error("TS plugin runtime is not available")
    }

    fn invoke_wasm(
        &mut self,
        _plugin: &LoadedPlugin,
        ctx: &InvokeContext,
        wasm_bytes: &[u8],
    ) -> InvokeResult {
        invoke_wasm_mvp(ctx, wasm_bytes)
    }
}

fn parse_invoke_result_stdout(stdout: &[u8]) -> Result<InvokeResult, String> {
    let value: Value = serde_json::from_slice(stdout)
        .map_err(|error| format!("failed to parse InvokeResult JSON: {error}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| "InvokeResult JSON must be an object".to_owned())?;
    let ok = object
        .get("ok")
        .and_then(Value::as_bool)
        .ok_or_else(|| "InvokeResult JSON must contain boolean ok".to_owned())?;
    let output = optional_string_field(object, "output")?;
    let error = optional_string_field(object, "error")?;
    Ok(InvokeResult { ok, output, error })
}

fn optional_string_field(
    object: &serde_json::Map<String, Value>,
    field: &str,
) -> Result<Option<String>, String> {
    object.get(field).map_or(Ok(None), |value| {
        value
            .as_str()
            .map(|text| Some(text.to_owned()))
            .ok_or_else(|| format!("InvokeResult JSON field {field} must be a string"))
    })
}

fn invoke_context_json(ctx: &InvokeContext) -> String {
    serde_json::json!({
        "source": ctx.source.as_str(),
        "args": ctx.args,
    })
    .to_string()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginNameAndTier {
    pub name: String,
    pub tier: PluginTier,
}

#[derive(Debug, Clone)]
pub struct DiscoverPackagesOptions {
    pub scan_dirs: Vec<PathBuf>,
    pub disabled_plugins: Vec<String>,
    pub runtime_version: String,
    pub use_cache: bool,
}

impl Default for DiscoverPackagesOptions {
    fn default() -> Self {
        Self {
            scan_dirs: scan_dirs(),
            disabled_plugins: Vec::new(),
            runtime_version: runtime_sdk_version(),
            use_cache: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoverPackagesReport {
    pub plugins: Vec<LoadedPlugin>,
    pub warnings: Vec<String>,
}

static DISCOVER_CACHE: OnceLock<Mutex<Option<Vec<LoadedPlugin>>>> = OnceLock::new();
static MODULE_SYMBOL_CACHE: OnceLock<Mutex<BTreeMap<String, String>>> = OnceLock::new();

/// Parse and validate a `plugin.json` text.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for malformed manifests.
pub fn parse_manifest(json_text: &str, dir: &Path) -> Result<PluginManifest, String> {
    let raw: Value =
        serde_json::from_str(json_text).map_err(|_| "plugin.json: invalid JSON".to_owned())?;
    let object = parse_manifest_object(&raw)?;
    let name = parse_manifest_name(object)?;
    let version = parse_manifest_version(object)?;
    let weight = parse_manifest_weight(object)?;
    let sdk = parse_manifest_sdk(object)?;
    let wasm = parse_declared_manifest_file(object, "wasm", dir)?;
    let entry = parse_declared_manifest_file(object, "entry", dir)?;
    let entry_export = parse_entry_export(object)?;
    let target = parse_target(&raw)?;
    validate_target_payload(target, wasm.as_deref(), entry.as_deref())?;

    let capability_namespaces = parse_capability_namespaces(&raw)?;
    let extra_namespaces: Vec<&str> = capability_namespaces
        .as_ref()
        .map_or_else(Vec::new, |namespaces| {
            namespaces.iter().map(String::as_str).collect()
        });
    let capabilities = parse_capabilities(&raw, &extra_namespaces)?;
    let (capabilities, capability_warnings) = capabilities.map_or((None, Vec::new()), |parsed| {
        (Some(parsed.capabilities), parsed.warnings)
    });

    Ok(PluginManifest {
        name,
        version,
        weight,
        tier: parse_tier(&raw)?,
        wasm,
        entry,
        entry_export,
        sdk,
        cli: parse_cli(&raw)?,
        api: parse_api(&raw)?,
        description: object
            .get("description")
            .and_then(Value::as_str)
            .map(str::to_owned),
        author: object
            .get("author")
            .and_then(Value::as_str)
            .map(str::to_owned),
        hooks: parse_hooks(&raw)?,
        cron: parse_cron(&raw)?,
        module: parse_module(&raw)?,
        transport: parse_transport(&raw)?,
        engine: parse_engine(&raw)?,
        target,
        capability_namespaces,
        capabilities,
        capability_warnings,
        dependencies: parse_dependencies(&raw)?,
        artifact: parse_artifact(&raw)?,
    })
}

fn validate_target_payload(
    target: Option<PluginTarget>,
    wasm: Option<&str>,
    entry: Option<&str>,
) -> Result<(), String> {
    if target != Some(PluginTarget::Wasm) {
        return Ok(());
    }
    let entry_is_wasm = entry.is_some_and(|entry| {
        Path::new(entry)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
    });
    if wasm.is_some() || entry_is_wasm {
        Ok(())
    } else {
        Err(
            "plugin.json: target \"wasm\" requires a wasm file (Phase C); JS entry plugins must use target \"js\""
                .to_owned(),
        )
    }
}

/// Load and validate `plugin.json` from a plugin directory.
///
/// # Errors
///
/// Returns filesystem read errors or maw-js-compatible manifest validation messages when
/// `plugin.json` exists but cannot be loaded.
pub fn load_manifest_from_dir(dir: &Path) -> Result<Option<LoadedPlugin>, String> {
    let manifest_path = dir.join("plugin.json");
    if !manifest_path.exists() {
        return Ok(None);
    }
    let json_text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("plugin.json: failed to read: {error}"))?;
    let manifest = parse_manifest(&json_text, dir)?;
    let has_entry = manifest.entry.is_some();
    let has_wasm_entry = manifest
        .entry
        .as_ref()
        .is_some_and(|entry| {
            Path::new(entry)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
        });
    let has_artifact_js = manifest.artifact.as_ref().is_some_and(|artifact| {
        !Path::new(&artifact.path)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
    });
    let effective_entry = manifest.entry.as_ref().or_else(|| {
        if has_artifact_js {
            manifest.artifact.as_ref().map(|artifact| &artifact.path)
        } else {
            None
        }
    });

    Ok(Some(LoadedPlugin {
        wasm_path: manifest
            .wasm
            .as_ref()
            .or_else(|| manifest.entry.as_ref().filter(|_| has_wasm_entry))
            .map_or_else(PathBuf::new, |wasm| resolve_dir_path(dir, wasm)),
        entry_path: effective_entry.filter(|_| !has_wasm_entry)
            .map(|entry| resolve_dir_path(dir, entry)),
        wasm_export: manifest
            .entry_export
            .clone()
            .unwrap_or_else(|| "handle".to_owned()),
        kind: if (has_entry && !has_wasm_entry) || has_artifact_js {
            LoadedPluginKind::Ts
        } else {
            LoadedPluginKind::Wasm
        },
        disabled: false,
        dir: dir.to_path_buf(),
        manifest,
    }))
}

/// Scan plugin roots and return packages that pass maw-js Phase A registry gates.
///
/// # Errors
///
/// This function does not expose filesystem errors; unreadable roots, malformed manifests,
/// and refused plugins are skipped like maw-js `discoverPackages`.
#[must_use]
pub fn discover_packages(options: &DiscoverPackagesOptions) -> DiscoverPackagesReport {
    discover_packages_with_profile(options, |_| None)
}

/// Scan plugin roots with an injected active-profile resolver.
///
/// # Errors
///
/// This function does not expose filesystem errors; unreadable roots, malformed manifests,
/// and refused plugins are skipped like maw-js `discoverPackages`.
#[must_use]
pub fn discover_packages_with_profile<F>(
    options: &DiscoverPackagesOptions,
    resolve_active_profile_filter: F,
) -> DiscoverPackagesReport
where
    F: FnOnce(&[PluginNameAndTier]) -> Option<BTreeSet<String>>,
{
    if options.use_cache {
        if let Some(cached) = cached_discover_plugins() {
            return DiscoverPackagesReport {
                plugins: cached,
                warnings: Vec::new(),
            };
        }
    }

    let mut plugins = Vec::new();
    let mut warnings = Vec::new();
    let mut legacy_count = 0usize;
    let mut seen_plugin_names = BTreeSet::new();

    for base_dir in &options.scan_dirs {
        let Ok(entries) = std::fs::read_dir(base_dir) else {
            continue;
        };
        for (entry, file_type) in entries
            .flatten()
            .filter_map(|entry| entry.file_type().ok().map(|file_type| (entry, file_type)))
        {
            if !file_type.is_dir() && !file_type.is_symlink() {
                continue;
            }
            match discover_plugin_dir(&entry.path(), options) {
                PluginDiscovery::Loaded(loaded) => {
                    if seen_plugin_names.insert(loaded.manifest.name.clone()) {
                        plugins.push(loaded);
                    }
                }
                PluginDiscovery::Legacy(loaded) => {
                    if seen_plugin_names.insert(loaded.manifest.name.clone()) {
                        legacy_count += 1;
                        plugins.push(loaded);
                    }
                }
                PluginDiscovery::Warning(warning) => warnings.push(warning),
                PluginDiscovery::Skip => {}
            }
        }
    }

    apply_weight_overrides(options.scan_dirs.first(), &mut plugins);
    plugins.sort_by_key(|plugin| plugin.manifest.weight.unwrap_or(50));

    let filter = resolve_active_profile_filter(
        &plugins
            .iter()
            .map(|plugin| PluginNameAndTier {
                name: plugin.manifest.name.clone(),
                tier: plugin.manifest.tier.unwrap_or(PluginTier::Core),
            })
            .collect::<Vec<_>>(),
    );
    if let Some(filter) = filter {
        plugins.retain(|plugin| filter.contains(&plugin.manifest.name));
    }

    if legacy_count > 0 {
        warnings.push(format!(
            "{legacy_count} legacy plugin{} loaded without artifact hash — build them to enforce integrity.",
            if legacy_count == 1 { "" } else { "s" }
        ));
    }

    if options.use_cache {
        cache_discover_plugins(plugins.clone());
    }

    DiscoverPackagesReport { plugins, warnings }
}

/// Clear registry discovery cache.
pub fn reset_discover_cache() {
    if let Ok(mut cache) = discover_cache().lock() {
        *cache = None;
    }
    if let Ok(mut cache) = module_symbol_cache().lock() {
        cache.clear();
    }
}

/// Import a whitelisted named symbol through an injected module loader.
///
/// This mirrors maw-js `importPluginSymbol` validation and caching, while leaving the
/// language-specific runtime module import to the caller.
///
/// # Errors
///
/// Returns maw-js-compatible errors for missing names, absent or disabled plugins,
/// missing module surfaces, unallowlisted symbols, module paths that escape the plugin
/// directory, loader failures, or runtime modules that omit the allowlisted export.
pub fn import_plugin_symbol<F>(
    plugin_name: &str,
    symbol_name: &str,
    plugins: &[LoadedPlugin],
    load_module_symbols: F,
) -> Result<String, String>
where
    F: FnOnce(&Path) -> Result<BTreeMap<String, String>, String>,
{
    if plugin_name.is_empty() {
        return Err("importPluginSymbol: pluginName is required".to_owned());
    }
    if symbol_name.is_empty() {
        return Err("importPluginSymbol: symbolName is required".to_owned());
    }

    let plugin = plugins
        .iter()
        .find(|plugin| plugin.manifest.name == plugin_name)
        .ok_or_else(|| format!("plugin '{plugin_name}' not found"))?;
    if plugin.disabled {
        return Err(format!("plugin '{plugin_name}' is disabled"));
    }
    let module_surface = plugin
        .manifest
        .module
        .as_ref()
        .ok_or_else(|| format!("plugin '{plugin_name}' does not declare a module surface"))?;
    if !module_surface
        .exports
        .iter()
        .any(|export| export == symbol_name)
    {
        return Err(format!(
            "plugin '{plugin_name}' does not export '{symbol_name}'"
        ));
    }

    let cache_key = format!("{}\0{plugin_name}\0{symbol_name}", plugin.dir.display());
    if let Some(value) = module_symbol_cache()
        .lock()
        .ok()
        .and_then(|cache| cache.get(&cache_key).cloned())
    {
        return Ok(value);
    }

    let module_path = resolve_plugin_module_path(plugin)?;
    let symbols = load_module_symbols(&module_path)?;
    let value = symbols.get(symbol_name).cloned().ok_or_else(|| {
        format!("plugin '{plugin_name}' module did not provide export '{symbol_name}'")
    })?;
    if let Ok(mut cache) = module_symbol_cache().lock() {
        cache.insert(cache_key, value.clone());
    }
    Ok(value)
}

/// Invoke a loaded plugin through maw-js-compatible universal dispatch guards.
///
/// This ports `src/plugin/registry-invoke.ts` universal CLI metadata/help,
/// TS-entry dispatch gating, and WASM file-read handoff while leaving the actual
/// JS/TS and WASM runtime engines injectable to callers.
#[must_use]
pub fn invoke_plugin<R>(plugin: &LoadedPlugin, ctx: &InvokeContext, runtime: &mut R) -> InvokeResult
where
    R: PluginInvokeRuntime,
{
    if let Some(result) = handle_universal_cli_flag(plugin, ctx) {
        return result;
    }

    if plugin.kind == LoadedPluginKind::Ts && plugin.entry_path.is_some() {
        return runtime.invoke_ts(plugin, ctx);
    }

    match std::fs::read(&plugin.wasm_path) {
        Ok(wasm_bytes) => runtime.invoke_wasm(plugin, ctx, &wasm_bytes),
        Err(error) => InvokeResult::error(format!("failed to read wasm: {error}")),
    }
}

/// Default plugin scan roots.
#[must_use]
pub fn scan_dirs() -> Vec<PathBuf> {
    std::env::var_os("MAW_PLUGINS_DIR").map_or_else(
        || {
            let home = std::env::var_os("MAW_HOME")
                .map(PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".maw")))
                .unwrap_or_else(|| PathBuf::from(".maw"));
            vec![home.join("plugins")]
        },
        |path| vec![PathBuf::from(path)],
    )
}

/// Runtime SDK version placeholder for registry gates.
#[must_use]
pub fn runtime_sdk_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

/// Compute a `sha256:<hex>` digest for a file.
///
/// # Errors
///
/// Returns the filesystem read error text if the file cannot be read.
pub fn hash_file(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|error| error.to_string())?;
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut hex, "{byte:02x}");
    }
    Ok(format!("sha256:{hex}"))
}

/// True when the top-level plugin install path is a symlink/dev install.
#[must_use]
pub fn is_dev_mode_install(plugin_dir: &Path) -> bool {
    std::fs::symlink_metadata(plugin_dir).is_ok_and(|metadata| metadata.file_type().is_symlink())
}

/// Minimal maw-js-compatible semver range satisfaction.
#[must_use]
pub fn satisfies(version: &str, range: &str) -> bool {
    let Some(version) = parse_semver_core(version) else {
        return false;
    };
    let range = range.trim();
    if range == "*" {
        return true;
    }

    let (op, rest) = semver_operator(range);
    let Some(target) = parse_semver_core(rest) else {
        return false;
    };

    match op {
        Some("^") => caret_satisfies(version, target),
        Some("~") => {
            compare_semver(version, target).is_ge()
                && version.0 == target.0
                && version.1 == target.1
        }
        Some(">=") => compare_semver(version, target).is_ge(),
        Some("<=") => compare_semver(version, target).is_le(),
        Some(">") => compare_semver(version, target).is_gt(),
        Some("<") => compare_semver(version, target).is_lt(),
        _ => compare_semver(version, target).is_eq(),
    }
}

/// Format maw-js SDK mismatch warning text.
#[must_use]
pub fn format_sdk_mismatch_error(name: &str, manifest_sdk: &str, runtime_version: &str) -> String {
    [
        format!("✗ plugin '{name}' requires maw SDK {manifest_sdk}"),
        format!("  your maw: {runtime_version}  (SDK {runtime_version})"),
        String::new(),
        "  fix:".to_owned(),
        "    • maw update                                    (upgrade maw)".to_owned(),
        format!("    • maw plugin install {name}@<old-version>      (older compat release)"),
        "    • (manual) edit plugin.json \"sdk\" to accept this version and rebuild".to_owned(),
    ]
    .join("\n")
}

fn resolve_dir_path(dir: &Path, path: &str) -> PathBuf {
    let base = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        std::env::current_dir().map_or_else(|_| PathBuf::from(".").join(dir), |cwd| cwd.join(dir))
    };
    base.join(path)
}

fn discover_cache() -> &'static Mutex<Option<Vec<LoadedPlugin>>> {
    DISCOVER_CACHE.get_or_init(|| Mutex::new(None))
}

fn module_symbol_cache() -> &'static Mutex<BTreeMap<String, String>> {
    MODULE_SYMBOL_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()))
}

fn resolve_plugin_module_path(plugin: &LoadedPlugin) -> Result<PathBuf, String> {
    let module_path = plugin
        .manifest
        .module
        .as_ref()
        .map(|module| module.path.as_str())
        .ok_or_else(|| {
            format!(
                "plugin '{}' does not declare module.path",
                plugin.manifest.name
            )
        })?;
    let resolved = plugin.dir.join(module_path);
    let plugin_root = std::fs::canonicalize(&plugin.dir).map_err(|error| error.to_string())?;
    let real_path = std::fs::canonicalize(&resolved).map_err(|error| error.to_string())?;
    if real_path != plugin_root && !real_path.starts_with(&plugin_root) {
        return Err(format!(
            "plugin '{}' module.path escapes plugin dir: {module_path}",
            plugin.manifest.name
        ));
    }
    Ok(real_path)
}

fn handle_universal_cli_flag(plugin: &LoadedPlugin, ctx: &InvokeContext) -> Option<InvokeResult> {
    if ctx.source != InvokeSource::Cli {
        return None;
    }
    let first = ctx.args.first()?;
    if matches!(first.as_str(), "-v" | "--version" | "-version") {
        return Some(InvokeResult::output(render_version_output(plugin)));
    }
    if ctx
        .args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help" | "-help"))
    {
        return Some(InvokeResult::output(render_help_output(plugin)));
    }
    None
}

#[cfg(test)]
mod part03_coverage_tests {
    use super::*;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn reset_discover_cache_clears_discovery_and_symbol_caches() {
        cache_discover_plugins(Vec::new());
        module_symbol_cache()
            .lock()
            .expect("symbol cache lock")
            .insert("dir\0plugin\0symbol".to_owned(), "cached".to_owned());

        assert_eq!(cached_discover_plugins(), Some(Vec::new()));
        assert!(!module_symbol_cache()
            .lock()
            .expect("symbol cache lock")
            .is_empty());

        reset_discover_cache();

        assert_eq!(cached_discover_plugins(), None);
        assert!(module_symbol_cache()
            .lock()
            .expect("symbol cache lock")
            .is_empty());
    }

    #[test]
    fn resolve_dir_path_handles_absolute_relative_and_missing_cwd_fallback() {
        let _guard = CWD_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let original = std::env::current_dir().expect("cwd");
        let root = std::env::temp_dir().join(format!(
            "maw-rs-resolve-dir-path-{}",
            std::process::id()
        ));
        let vanished = root.join("vanished");
        std::fs::create_dir_all(&vanished).expect("vanished dir");

        let absolute = resolve_dir_path(&root, "plugin.wasm");
        assert_eq!(absolute, root.join("plugin.wasm"));

        std::env::set_current_dir(&root).expect("set cwd");
        let cwd_root = std::env::current_dir().expect("canonical cwd");
        let relative = resolve_dir_path(Path::new("plugins/helper"), "index.ts");
        assert_eq!(relative, cwd_root.join("plugins/helper/index.ts"));

        std::env::set_current_dir(&vanished).expect("set vanished cwd");
        std::fs::remove_dir_all(&vanished).expect("remove cwd");
        let fallback = resolve_dir_path(Path::new("plugins/helper"), "index.ts");
        assert_eq!(fallback, PathBuf::from(".").join("plugins/helper/index.ts"));

        std::env::set_current_dir(original).expect("restore cwd");
        let _ = std::fs::remove_dir_all(root);
    }
}
