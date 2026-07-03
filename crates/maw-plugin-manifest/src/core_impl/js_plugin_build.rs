#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginBuildSummary {
    pub name: String,
    pub version: String,
    pub dir: PathBuf,
    pub bundle_path: PathBuf,
    pub size_bytes: u64,
    pub capabilities: Vec<String>,
    pub inferred_only: Vec<String>,
    pub declared_only: Vec<String>,
    pub sha256: String,
    pub manifest_path: PathBuf,
    pub dts_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInstallSummary {
    pub name: String,
    pub version: String,
    pub source_dir: PathBuf,
    pub install_dir: PathBuf,
    pub copied_files: Vec<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginInitSummary {
    pub name: String,
    pub dir: PathBuf,
    pub manifest_path: PathBuf,
    pub entry_path: PathBuf,
}

#[must_use]
pub fn infer_plugin_capabilities(source: &str) -> Vec<String> {
    let mut caps = BTreeSet::new();
    let code = strip_line_and_block_comments(source);

    if has_static_or_dynamic_import(&code, "node:fs") {
        caps.insert("fs:read".to_owned());
    }
    if has_static_or_dynamic_import(&code, "node:child_process") {
        caps.insert("proc:spawn".to_owned());
    }
    if has_static_or_dynamic_import(&code, "bun:ffi") {
        caps.insert("ffi:any".to_owned());
    }
    if contains_global_call(&code, "fetch") {
        caps.insert("net:fetch".to_owned());
    }

    for method in member_methods(&code, "maw") {
        caps.insert(format!("sdk:{method}"));
    }
    for alias in maw_aliases(&code) {
        for method in member_methods(&code, &alias) {
            caps.insert(format!("sdk:{method}"));
        }
    }
    for method in maw_destructured_methods(&code) {
        caps.insert(format!("sdk:{method}"));
    }

    caps.into_iter().collect()
}

/// Build a JS plugin directory using native, side-effect-limited filesystem logic.
///
/// This intentionally does not execute a guest-controlled bundler. It copies the declared entry
/// to `dist/index.js`, infers capabilities from source text, writes `dist/plugin.json` with
/// `artifact.path` and `artifact.sha256`, and optionally emits a minimal declaration stub.
///
/// # Errors
///
/// Returns manifest validation, entry read, output directory creation, bundle write, manifest
/// serialization, or declaration emit errors.
pub fn build_js_plugin_dir(dir: &Path, emit_types: bool) -> Result<PluginBuildSummary, String> {
    let manifest_path = dir.join("plugin.json");
    if !manifest_path.exists() {
        return Err(format!("no plugin.json in {}", dir.display()));
    }
    let text = std::fs::read_to_string(&manifest_path)
        .map_err(|error| format!("invalid plugin.json: {error}"))?;
    let mut raw: Value = serde_json::from_str(&text)
        .map_err(|error| format!("invalid plugin.json: {error}"))?;
    let manifest = parse_manifest(&text, dir)?;
    let target = raw.get("target").and_then(Value::as_str).unwrap_or("js");
    if target == "wasm" {
        return Err("target \"wasm\" is handled by native Rust WASM build route".to_owned());
    }
    if target != "js" {
        return Err(format!("unknown target {} (expected \"js\" or \"wasm\")", json_value_display(raw.get("target"))));
    }

    let entry = manifest.entry.as_deref().unwrap_or("./src/index.ts");
    let entry_path = resolve_dir_path(dir, entry);
    if !entry_path.exists() {
        return Err(format!("entry not found: {}", entry_path.display()));
    }
    let source = std::fs::read_to_string(&entry_path)
        .map_err(|error| format!("entry read failed: {error}"))?;
    let dist_dir = dir.join("dist");
    std::fs::create_dir_all(&dist_dir).map_err(|error| format!("dist create failed: {error}"))?;
    let bundle_path = dist_dir.join("index.js");
    std::fs::write(&bundle_path, &source).map_err(|error| format!("bundle write failed: {error}"))?;
    let size_bytes = source.len() as u64;
    let sha256 = format!("sha256:{}", sha256_hex(source.as_bytes()));

    let capabilities = infer_plugin_capabilities(&source);
    let declared = manifest.capabilities.clone().unwrap_or_default();
    let inferred_only = sorted_difference(&capabilities, &declared);
    let declared_only = sorted_difference(&declared, &capabilities);

    let object = raw
        .as_object_mut()
        .ok_or_else(|| "plugin.json: manifest root must be an object".to_owned())?;
    object.insert("capabilities".to_owned(), string_array_value(&capabilities));
    object.insert("entry".to_owned(), Value::String("./index.js".to_owned()));
    object.insert(
        "artifact".to_owned(),
        serde_json::json!({"path":"./index.js","sha256":sha256}),
    );
    let dist_manifest_path = dist_dir.join("plugin.json");
    std::fs::write(
        &dist_manifest_path,
        serde_json::to_string_pretty(&raw)
            .map_err(|error| format!("dist plugin.json serialize failed: {error}"))?
            + "\n",
    )
    .map_err(|error| format!("dist plugin.json write failed: {error}"))?;

    let dts_path = if emit_types {
        let path = dist_dir.join(format!("{}.d.ts", manifest.name));
        std::fs::write(&path, "export {};\n")
            .map_err(|error| format!("dts-gen: write failed: {error}"))?;
        Some(path)
    } else {
        None
    };

    Ok(PluginBuildSummary {
        name: manifest.name,
        version: manifest.version,
        dir: dir.to_path_buf(),
        bundle_path,
        size_bytes,
        capabilities,
        inferred_only,
        declared_only,
        sha256,
        manifest_path: dist_manifest_path,
        dts_path,
    })
}

/// Initialize a native JS plugin source directory.
///
/// # Errors
///
/// Returns an error when the name is invalid, the destination exists, or scaffold files cannot be
/// created or serialized.
pub fn init_js_plugin_dir(name: &str, dir: &Path) -> Result<PluginInitSummary, String> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        return Err("plugin init: name must be a lowercase slug".to_owned());
    }
    if dir.exists() {
        return Err(format!("plugin init: destination exists: {}", dir.display()));
    }
    std::fs::create_dir_all(dir.join("src"))
        .map_err(|error| format!("plugin init: create failed: {error}"))?;
    let entry_path = dir.join("src").join("index.ts");
    let manifest_path = dir.join("plugin.json");
    let manifest = serde_json::json!({
        "name": name.replace('_', "-"),
        "version": "0.1.0",
        "target": "js",
        "sdk": "^1.0.0",
        "entry": "./src/index.ts",
        "capabilities": [],
        "cli": {"command": name.replace('_', "-")}
    });
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)
            .map_err(|error| format!("plugin init: manifest serialize failed: {error}"))?
            + "\n",
    )
    .map_err(|error| format!("plugin init: manifest write failed: {error}"))?;
    std::fs::write(
        &entry_path,
        "import { maw } from \"maw\";\n\nexport async function main() {\n  return { ok: true };\n}\n",
    )
    .map_err(|error| format!("plugin init: entry write failed: {error}"))?;
    Ok(PluginInitSummary {
        name: name.replace('_', "-"),
        dir: dir.to_path_buf(),
        manifest_path,
        entry_path,
    })
}

/// Install a built plugin directory into a host-selected install root.
///
/// # Errors
///
/// Returns manifest load, destination creation, source tree read, or file copy errors. The install
/// root is supplied by the host; this function never accepts a guest-selected dlopen path.
pub fn install_built_plugin_dir(source_dir: &Path, install_root: &Path) -> Result<PluginInstallSummary, String> {
    let plugin = load_manifest_from_dir(source_dir)?
        .ok_or_else(|| format!("no plugin.json in {}", source_dir.display()))?;
    let dist_dir = source_dir.join("dist");
    let package_dir = if dist_dir.join("plugin.json").exists() { &dist_dir } else { source_dir };
    let install_dir = install_root.join(&plugin.manifest.name);
    std::fs::create_dir_all(&install_dir)
        .map_err(|error| format!("plugin install: create failed: {error}"))?;
    let mut copied_files = Vec::new();
    copy_plugin_tree(package_dir, &install_dir, package_dir, &mut copied_files)?;
    copied_files.sort();
    Ok(PluginInstallSummary {
        name: plugin.manifest.name,
        version: plugin.manifest.version,
        source_dir: package_dir.to_path_buf(),
        install_dir,
        copied_files,
    })
}

fn copy_plugin_tree(src: &Path, dest: &Path, root: &Path, copied: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in std::fs::read_dir(src).map_err(|error| format!("plugin install: read failed: {error}"))? {
        let entry = entry.map_err(|error| format!("plugin install: read failed: {error}"))?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if matches!(name.as_ref(), ".git" | "node_modules" | "target") {
            continue;
        }
        let from = entry.path();
        let to = dest.join(name.as_ref());
        if from.is_dir() {
            std::fs::create_dir_all(&to).map_err(|error| format!("plugin install: mkdir failed: {error}"))?;
            copy_plugin_tree(&from, &to, root, copied)?;
        } else if from.is_file() {
            std::fs::copy(&from, &to).map_err(|error| format!("plugin install: copy failed: {error}"))?;
            copied.push(from.strip_prefix(root).unwrap_or(&from).to_path_buf());
        }
    }
    Ok(())
}

fn sorted_difference(left: &[String], right: &[String]) -> Vec<String> {
    left.iter()
        .filter(|value| !right.contains(value))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hasher.finalize().iter().fold(String::new(), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

fn string_array_value(values: &[String]) -> Value {
    Value::Array(values.iter().cloned().map(Value::String).collect())
}

fn json_value_display(value: Option<&Value>) -> String {
    value.map_or_else(|| "null".to_owned(), ToString::to_string)
}

fn has_static_or_dynamic_import(code: &str, module: &str) -> bool {
    let quoted_single = format!("'{module}'");
    let quoted_double = format!("\"{module}\"");
    let prefix_single = format!("'{module}/");
    let prefix_double = format!("\"{module}/");
    code.contains(&quoted_single)
        || code.contains(&quoted_double)
        || code.contains(&prefix_single)
        || code.contains(&prefix_double)
}

fn contains_global_call(code: &str, name: &str) -> bool {
    let bytes = code.as_bytes();
    let needle = name.as_bytes();
    let mut index = 0;
    while let Some(pos) = code[index..].find(name) {
        let start = index + pos;
        let end = start + needle.len();
        let before = start.checked_sub(1).and_then(|i| bytes.get(i).copied());
        let after = bytes.get(end).copied();
        let ident_before = before.is_some_and(is_ident_byte) || before == Some(b'.');
        let ident_after = after.is_some_and(is_ident_byte);
        let rest = &code[end..];
        if !ident_before && !ident_after && rest.trim_start().starts_with('(') {
            return true;
        }
        index = end;
    }
    false
}

fn member_methods(code: &str, object: &str) -> Vec<String> {
    let mut found = BTreeSet::new();
    let dot = format!("{object}.");
    let bracket_single = format!("{object}['");
    let bracket_double = format!("{object}[\"");
    for (prefix, term) in [(dot.as_str(), None), (bracket_single.as_str(), Some('\'')), (bracket_double.as_str(), Some('"'))] {
        let mut index = 0;
        while let Some(pos) = code[index..].find(prefix) {
            let start = index + pos + prefix.len();
            if let Some(term) = term {
                if let Some(end) = code[start..].find(term) {
                    let candidate = &code[start..start + end];
                    if is_identifier(candidate) {
                        found.insert(candidate.to_owned());
                    }
                    index = start + end + 1;
                } else {
                    break;
                }
            } else {
                let candidate: String = code[start..]
                    .chars()
                    .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
                    .collect();
                if is_identifier(&candidate) {
                    found.insert(candidate);
                }
                index = start + 1;
            }
        }
    }
    found.into_iter().collect()
}

fn maw_aliases(code: &str) -> Vec<String> {
    let mut aliases = BTreeSet::new();
    for marker in ["= maw", "=maw"] {
        let mut index = 0;
        while let Some(pos) = code[index..].find(marker) {
            let eq = index + pos;
            let before = code[..eq].trim_end();
            let alias: String = before
                .chars()
                .rev()
                .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == '$')
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if is_identifier(&alias) && alias != "maw" {
                aliases.insert(alias);
            }
            index = eq + marker.len();
        }
    }
    aliases.into_iter().collect()
}

fn maw_destructured_methods(code: &str) -> Vec<String> {
    let mut methods = BTreeSet::new();
    let mut index = 0;
    while let Some(pos) = code[index..].find("} = maw") {
        let close = index + pos;
        if let Some(open) = code[..close].rfind('{') {
            for raw in code[open + 1..close].split(',') {
                let name = raw.trim().split(':').next().unwrap_or_default().trim();
                if is_identifier(name) {
                    methods.insert(name.to_owned());
                }
            }
        }
        index = close + 7;
    }
    methods.into_iter().collect()
}

fn is_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else { return false; };
    (first.is_ascii_alphabetic() || first == '_' || first == '$')
        && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '$')
}

fn is_ident_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn strip_line_and_block_comments(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    let mut in_string: Option<char> = None;
    while let Some(ch) = chars.next() {
        if let Some(quote) = in_string {
            out.push(ch);
            if ch == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if ch == quote {
                in_string = None;
            }
            continue;
        }
        if matches!(ch, '\'' | '"' | '`') {
            in_string = Some(ch);
            out.push(ch);
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'/') {
            for next in chars.by_ref() {
                if next == '\n' {
                    out.push('\n');
                    break;
                }
            }
            continue;
        }
        if ch == '/' && chars.peek() == Some(&'*') {
            let _ = chars.next();
            let mut prev = '\0';
            for next in chars.by_ref() {
                if prev == '*' && next == '/' {
                    break;
                }
                prev = next;
            }
            continue;
        }
        out.push(ch);
    }
    out
}
