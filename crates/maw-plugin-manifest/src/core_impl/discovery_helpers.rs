fn cache_discover_plugins(plugins: Vec<LoadedPlugin>) {
    if let Ok(mut cache) = discover_cache().lock() {
        *cache = Some(plugins);
    }
}

enum PluginDiscovery {
    Loaded(LoadedPlugin),
    Legacy(LoadedPlugin),
    Warning(String),
    Skip,
}

fn discover_plugin_dir(pkg_dir: &Path, options: &DiscoverPackagesOptions) -> PluginDiscovery {
    let Some(mut loaded) = load_manifest_from_dir(pkg_dir).ok().flatten() else {
        return PluginDiscovery::Skip;
    };
    let manifest = &loaded.manifest;

    if !satisfies(&options.runtime_version, &manifest.sdk) {
        return PluginDiscovery::Warning(format_sdk_mismatch_error(
            &manifest.name,
            &manifest.sdk,
            &options.runtime_version,
        ));
    }

    if let Some(warning) = artifact_refusal_warning(pkg_dir, manifest) {
        return PluginDiscovery::Warning(warning);
    }

    if options
        .disabled_plugins
        .iter()
        .any(|disabled| disabled == &loaded.manifest.name)
    {
        loaded.disabled = true;
    }

    let has_artifact = loaded.manifest.artifact.is_some();
    if has_artifact {
        PluginDiscovery::Loaded(loaded)
    } else {
        PluginDiscovery::Legacy(loaded)
    }
}

fn artifact_refusal_warning(pkg_dir: &Path, manifest: &PluginManifest) -> Option<String> {
    let artifact = manifest.artifact.as_ref()?;
    if is_dev_mode_install(pkg_dir) {
        return None;
    }
    let Some(expected_sha) = &artifact.sha256 else {
        return Some(format!(
            "plugin '{}' is unbuilt — run `maw plugin build` in {}",
            manifest.name,
            pkg_dir.display()
        ));
    };
    let artifact_path = pkg_dir.join(&artifact.path);
    if !artifact_path.exists() {
        return Some(format!(
            "plugin '{}' artifact missing: {}",
            manifest.name, artifact.path
        ));
    }
    match hash_file(&artifact_path) {
        Ok(observed) if observed == *expected_sha => None,
        Ok(observed) => Some(format!(
            "plugin '{}' artifact hash mismatch — refusing to load.\n  expected: {}\n  actual:   {}",
            manifest.name, expected_sha, observed
        )),
        Err(error) => Some(format!(
            "plugin '{}' artifact hash failed: {error}",
            manifest.name
        )),
    }
}

fn apply_weight_overrides(primary_plugin_dir: Option<&PathBuf>, plugins: &mut [LoadedPlugin]) {
    let Some(primary_plugin_dir) = primary_plugin_dir else {
        return;
    };
    let overrides_path = primary_plugin_dir.join(".overrides.json");
    let Ok(raw) = std::fs::read_to_string(overrides_path) else {
        return;
    };
    let Ok(overrides) = serde_json::from_str::<BTreeMap<String, u64>>(&raw) else {
        return;
    };
    for plugin in plugins {
        if let Some(weight) = overrides.get(&plugin.manifest.name) {
            plugin.manifest.weight = Some(*weight);
        }
    }
}

fn parse_semver_core(value: &str) -> Option<(u64, u64, u64)> {
    let trimmed = value.trim();
    let without_build = trimmed.split_once('+').map_or(trimmed, |(core, _)| core);
    let core = without_build
        .split_once('-')
        .map_or(without_build, |(core, _)| core);
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn semver_operator(range: &str) -> (Option<&'static str>, &str) {
    for op in [">=", "<=", "^", "~", ">", "<"] {
        if let Some(rest) = range.strip_prefix(op) {
            return (Some(op), rest);
        }
    }
    (None, range)
}

fn compare_semver(left: (u64, u64, u64), right: (u64, u64, u64)) -> std::cmp::Ordering {
    left.cmp(&right)
}

fn caret_satisfies(version: (u64, u64, u64), target: (u64, u64, u64)) -> bool {
    if compare_semver(version, target).is_lt() {
        return false;
    }
    if target.0 > 0 {
        return version.0 == target.0;
    }
    if target.1 > 0 {
        return version.0 == 0 && version.1 == target.1;
    }
    version.0 == 0 && version.1 == 0 && version.2 == target.2
}

fn parse_manifest_object(raw: &Value) -> Result<&Map<String, Value>, String> {
    raw.as_object()
        .ok_or_else(|| "plugin.json: must be a JSON object".to_owned())
}

fn parse_manifest_name(object: &Map<String, Value>) -> Result<String, String> {
    object
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| is_slug(name))
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "plugin.json: name must match /^[a-z0-9-]+$/ (got {})",
                manifest_field_for_error(object, "name")
            )
        })
}

fn parse_manifest_version(object: &Map<String, Value>) -> Result<String, String> {
    object
        .get("version")
        .and_then(Value::as_str)
        .filter(|version| is_semver(version))
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "plugin.json: version must be semver N.N.N (got {})",
                manifest_field_for_error(object, "version")
            )
        })
}

fn parse_manifest_weight(object: &Map<String, Value>) -> Result<Option<u64>, String> {
    let Some(value) = object.get("weight") else {
        return Ok(None);
    };
    let valid_weight = value
        .as_u64()
        .filter(|weight| *weight <= 99)
        .ok_or_else(weight_error)?;
    Ok(Some(valid_weight))
}

fn parse_manifest_sdk(object: &Map<String, Value>) -> Result<String, String> {
    object
        .get("sdk")
        .and_then(Value::as_str)
        .filter(|sdk| is_semver_range(sdk))
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "plugin.json: sdk must be a semver range (got {})",
                manifest_field_for_error(object, "sdk")
            )
        })
}

fn parse_declared_manifest_file(
    object: &Map<String, Value>,
    key: &str,
    dir: &Path,
) -> Result<Option<String>, String> {
    let Some(value) = object.get(key) else {
        return Ok(None);
    };
    let path = if key == "entry" {
        if let Some(path) = value.as_str().filter(|path| !path.is_empty()) {
            path
        } else if let Some(entry) = value.as_object() {
            let kind = entry.get("kind").and_then(Value::as_str);
            if kind != Some("wasm") {
                return Err("plugin.json: entry.kind must be \"wasm\" when entry is an object".to_owned());
            }
            entry
                .get("path")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .ok_or_else(|| "plugin.json: entry.path must be a non-empty string".to_owned())?
        } else {
            return Err("plugin.json: entry must be a string or wasm entry object".to_owned());
        }
    } else if let Some(path) = value.as_str().filter(|path| !path.is_empty()) {
        path
    } else {
        return Ok(None);
    };
    let declared_path = dir.join(path);
    if declared_path.exists() {
        Ok(Some(path.to_owned()))
    } else {
        Err(format!(
            "plugin.json: {key} file not found: {}",
            declared_path.display()
        ))
    }
}

fn parse_entry_export(object: &Map<String, Value>) -> Result<Option<String>, String> {
    let Some(entry) = object.get("entry").and_then(Value::as_object) else {
        return Ok(None);
    };
    let Some(raw) = entry.get("export") else {
        return Ok(Some("handle".to_owned()));
    };
    raw.as_str()
        .filter(|value| !value.is_empty())
        .map(|value| Some(value.to_owned()))
        .ok_or_else(|| "plugin.json: entry.export must be a non-empty string".to_owned())
}

fn manifest_field_for_error(object: &Map<String, Value>, key: &str) -> String {
    object
        .get(key)
        .map_or("undefined".to_owned(), Value::to_string)
}

fn weight_error() -> String {
    "plugin.json: weight must be a number 0-99 (lower = runs first, default 50)".to_owned()
}

fn is_semver(value: &str) -> bool {
    let (core_and_pre, build_ok) = split_once_optional(value, '+');
    if !build_ok || core_and_pre.is_empty() {
        return false;
    }
    let (core, pre_ok) = split_once_optional(core_and_pre, '-');
    pre_ok && is_semver_core(core)
}

fn is_semver_range(value: &str) -> bool {
    if value == "*" {
        return true;
    }
    for op in [">=", "<=", "^", "~", ">", "<"] {
        if let Some(rest) = value.strip_prefix(op) {
            return is_semver(rest);
        }
    }
    is_semver(value)
}

fn split_once_optional(value: &str, separator: char) -> (&str, bool) {
    let mut parts = value.split(separator);
    let first = parts.next().unwrap_or(value);
    if parts.next().is_some_and(str::is_empty) {
        return (first, false);
    }
    if parts.next().is_some() {
        return (first, false);
    }
    (first, true)
}

fn is_semver_core(core: &str) -> bool {
    let mut parts = core.split('.');
    let major = parts.next().unwrap_or_default();
    let Some(minor) = parts.next() else {
        return false;
    };
    let Some(patch) = parts.next() else {
        return false;
    };
    parts.next().is_none()
        && [major, minor, patch]
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("maw-plugin-manifest-{name}-{nonce}"))
    }

    fn manifest(name: &str) -> PluginManifest {
        PluginManifest {
            name: name.to_owned(),
            version: "1.2.3".to_owned(),
            weight: None,
            tier: None,
            wasm: None,
            entry: None,
            entry_export: None,
            sdk: "*".to_owned(),
            cli: None,
            api: None,
            description: Some("demo plugin".to_owned()),
            author: None,
            hooks: None,
            cron: None,
            module: None,
            transport: None,
            engine: None,
            target: None,
            capability_namespaces: None,
            capabilities: None,
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        }
    }

    fn loaded(name: &str, dir: PathBuf) -> LoadedPlugin {
        LoadedPlugin {
            manifest: manifest(name),
            dir,
            wasm_path: PathBuf::new(),
            entry_path: None,
            wasm_export: "handle".to_owned(),
            kind: LoadedPluginKind::Wasm,
            disabled: false,
        }
    }

    #[test]
    fn module_exports_reject_missing_or_empty_exports() {
        let missing = serde_json::json!({"module": {"path": "./mod.js"}});
        assert_eq!(
            parse_module(&missing).expect_err("missing exports"),
            "plugin.json: module.exports must be a non-empty array of strings"
        );

        let empty = serde_json::json!({"module": {"exports": [], "path": "./mod.js"}});
        assert_eq!(
            parse_module(&empty).expect_err("empty exports"),
            "plugin.json: module.exports must be a non-empty array of strings"
        );

        let non_string = serde_json::json!({"module": {"exports": ["ok", 1], "path": "./mod.js"}});
        assert_eq!(
            parse_module(&non_string).expect_err("non-string exports"),
            "plugin.json: module.exports must be a non-empty array of strings"
        );
    }

    #[test]
    fn import_symbol_reports_missing_module_path_before_loading() {
        let plugin = loaded("demo", temp_dir("missing-module"));
        let err = resolve_plugin_module_path(&plugin).expect_err("missing module path");
        assert_eq!(err, "plugin 'demo' does not declare module.path");
    }

    #[test]
    fn wasm_parser_reads_handle_body_and_errors_when_body_is_absent() {
        let body = [0x00, 0x41, 0x2a, 0x0b];
        let section = [
            0x01,
            u8::try_from(body.len()).expect("small wasm body"),
            body[0],
            body[1],
            body[2],
            body[3],
        ];
        assert_eq!(
            parse_handle_result(&section, 0, 0, 1).expect("const handle result"),
            42
        );
        assert_eq!(
            parse_handle_result(&[0x00], 0, 0, 1).expect_err("missing body"),
            "failed to parse WebAssembly module"
        );
    }

    #[test]
    fn wasm_helpers_cover_limits_memory_bounds_and_result_fallbacks() {
        let mut limits = WasmCursor::new(&[0x01, 0x01, 0x02]);
        skip_limits(&mut limits).expect("limits with max");

        let mut memory = [0_u8; 4];
        write_linear_memory(&mut memory, 10, b"ignored");
        assert_eq!(memory, [0, 0, 0, 0]);

        assert_eq!(read_wasm_result_from_memory(&memory, 0), InvokeResult::ok());
        assert_eq!(
            read_wasm_result_from_memory(&memory, 99),
            InvokeResult::ok()
        );
    }

    #[test]
    fn semver_helpers_reject_empty_optional_segments_and_missing_core_parts() {
        assert!(!is_semver("1.2.3+"));
        assert!(!is_semver("1.2.3-alpha-extra-more"));
        assert!(!is_semver_core("1.2"));
        assert!(!is_semver_core("1.2."));
    }

    #[test]
    fn wasm_parser_covers_second_defined_body_and_helper_edges() {
        let first_body = [0x00, 0x41, 0x01, 0x0b];
        let second_body = [0x00, 0x41, 0x2a, 0x0b];
        let section = [
            0x02,
            u8::try_from(first_body.len()).expect("small first body"),
            first_body[0],
            first_body[1],
            first_body[2],
            first_body[3],
            u8::try_from(second_body.len()).expect("small second body"),
            second_body[0],
            second_body[1],
            second_body[2],
            second_body[3],
        ];

        assert_eq!(
            parse_handle_result(&section, 1, 0, 2).expect("second body result"),
            42
        );

        let mut limits_without_max = WasmCursor::new(&[0x00, 0x01]);
        skip_limits(&mut limits_without_max).expect("limits without max");

        let mut partial = [0_u8; 4];
        write_linear_memory(&mut partial, 2, b"abcd");
        assert_eq!(partial, [0, 0, b'a', b'b']);

        assert_eq!(
            read_wasm_result_from_memory(&[0, 0, 0, 0], 1),
            InvokeResult::ok()
        );
        assert_eq!(read_length_prefixed_wasm_output(&[0, 0, 0], 0), None);
        assert_eq!(
            split_once_optional("1.2.3+build+again", '+'),
            ("1.2.3", false)
        );
        assert!(!is_semver_core("1..3"));
    }

    #[test]
    fn wasm_module_parse_records_handle_result_from_code_section() {
        let mut wasm = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        wasm.extend([0x03, 0x02, 0x01, 0x00]);
        wasm.extend([
            0x07, 0x13, 0x02, 0x06, b'm', b'e', b'm', b'o', b'r', b'y', 0x02, 0x00, 0x06, b'h',
            b'a', b'n', b'd', b'l', b'e', 0x00, 0x00,
        ]);
        wasm.extend([0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b]);

        let module = MvpWasmModule::parse(&wasm).expect("valid tiny module");

        assert!(module.exports_handle);
        assert!(module.exports_memory);
        assert_eq!(module.handle_result, 42);
    }

    #[test]
    fn optional_code_section_without_handle_export_is_noop() {
        let mut module = MvpWasmModule::default();
        parse_optional_code_section(&[0x00], None, 0, 0, &mut module)
            .expect("missing exported handle skips code parse");
        assert_eq!(module.handle_result, 0);
    }

    #[test]
    fn discover_applies_weight_overrides() {
        let root = temp_dir("discover");
        let plugin_dir = root.join("demo");
        std::fs::create_dir_all(&plugin_dir).expect("create plugin dir");
        std::fs::write(
            plugin_dir.join("plugin.json"),
            r#"{"name":"demo","version":"1.0.0","sdk":"*"}"#,
        )
        .expect("write manifest");
        std::fs::write(root.join(".overrides.json"), r#"{"demo":7}"#).expect("write overrides");

        let report = discover_packages(&DiscoverPackagesOptions {
            scan_dirs: vec![root.clone()],
            disabled_plugins: Vec::new(),
            runtime_version: runtime_sdk_version(),
            use_cache: false,
        });

        assert_eq!(report.plugins.len(), 1);
        assert_eq!(report.plugins[0].manifest.weight, Some(7));
        assert!(report
            .warnings
            .iter()
            .any(|warning| warning.contains("legacy plugin")));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn manifest_error_helpers_cover_validation_tail_edges() {
        let root = temp_dir("manifest-errors");

        assert_eq!(
            parse_manifest(
                &serde_json::json!({"name":"Bad","version":"1.0.0","sdk":"*"}).to_string(),
                &root,
            )
            .expect_err("invalid slug"),
            "plugin.json: name must match /^[a-z0-9-]+$/ (got \"Bad\")"
        );

        assert_eq!(
            parse_manifest(
                &serde_json::json!({"name":"heavy","version":"1.0.0","sdk":"*","weight":100})
                    .to_string(),
                &root,
            )
            .expect_err("invalid weight"),
            "plugin.json: weight must be a number 0-99 (lower = runs first, default 50)"
        );

        let missing_entry = parse_manifest(
            &serde_json::json!({
                "name":"missing-entry",
                "version":"1.0.0",
                "sdk":"*",
                "entry":"missing.ts"
            })
            .to_string(),
            &root,
        )
        .expect_err("missing declared entry");
        assert!(missing_entry.contains("plugin.json: entry file not found:"));

        assert_eq!(
            split_once_optional("1.2.3-alpha-extra", '-'),
            ("1.2.3", false)
        );
        assert!(!is_semver_core("1"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn wasm_private_helpers_cover_remaining_control_edges() {
        let mut module = MvpWasmModule::default();
        parse_optional_code_section(&[0x00], None, 0, 1, &mut module)
            .expect("missing handle export skips code parsing");
        assert_eq!(module.handle_result, 0);

        let err = parse_optional_code_section(&[0x00], Some(0), 0, 1, &mut module)
            .expect_err("empty code section cannot contain exported handle body");
        assert_eq!(err, "failed to parse WebAssembly module");

        let mut memory = [1_u8, 2, 3, 4];
        write_linear_memory(&mut memory, 4, b"ignored");
        assert_eq!(memory, [1, 2, 3, 4]);
    }
}
