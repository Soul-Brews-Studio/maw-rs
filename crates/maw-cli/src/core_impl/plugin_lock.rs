// What was installed, from where, at which commit and hash.
//
// The lock is what makes an install reproducible and a drift detectable: it
// records the resolved source and pin per package, so a later verify can say not
// just "this is installed" but "this is the same thing that was installed".

struct PluginLockEntry { version: String, sha256: String, source: Option<String> }

fn verify_plugin_install_pin(
    name: &str,
    version: &str,
    observed_sha256: Option<&str>,
    expected_sha256: Option<&str>,
    warn_unpinned: bool,
    force: bool,
) -> Result<PluginInstallVerification, String> {
    let observed = observed_sha256.ok_or_else(|| "plugin install: sha256 unavailable after build".to_owned())?;
    let locked = read_plugin_lock_entry_full(name)?;
    let mut lock_override = false;
    if let Some(entry) = &locked {
        if entry.version != version || entry.sha256 != observed {
            // --force is the sanctioned upgrade path now that installs write
            // the lock: it replaces the recorded pin instead of refusing.
            if force {
                lock_override = true;
            } else if entry.version != version {
                return Err(format!("plugin '{name}' version mismatch: plugins.lock={} install={version} (use --force to update the pin)", entry.version));
            } else {
                return Err(format!("plugin '{name}' sha256 mismatch — refusing to install.\n  plugins.lock: {}\n  install:      {observed}\n(use --force to update the pin)", entry.sha256));
            }
        }
    }
    if let Some(expected) = expected_sha256 {
        if expected != observed {
            return Err(format!("plugin '{name}' sha256 mismatch — refusing to install.\n  expected: {expected}\n  install:  {observed}"));
        }
    }
    let warning = if lock_override {
        Some(format!("warning: plugin '{name}' plugins.lock pin replaced (--force): {version} {observed}"))
    } else {
        (locked.is_none() && expected_sha256.is_none() && warn_unpinned).then(|| {
            format!("warning: plugin install {name} is unpinned; use owner/repo@ref and --sha256 {observed}")
        })
    };
    Ok(PluginInstallVerification { warning, resolved_sha256: Some(observed.to_owned()) })
}

/// Resolve the consumer-side lock file: `MAW_PLUGINS_LOCK` override, else
/// `<maw data dir>/plugins.lock` (sibling of the `plugins/` install root).
fn plugin_lock_path() -> std::path::PathBuf {
    std::env::var_os("MAW_PLUGINS_LOCK").map_or_else(
        || maw_data_path(&real_xdg_env(), &["plugins.lock"]),
        std::path::PathBuf::from,
    )
}

/// plugins.lock format (schema 1), written by `maw plugin install` (git and
/// local routes) and read back as the consumer-side pin gate:
///
/// ```json
/// {
///   "schema": 1,
///   "plugins": {
///     "<name>": {
///       "version": "1.1.0",
///       "sha256": "sha256:<64 lowercase hex>",
///       "source": "github:owner/repo@ref/sub/path | path:/abs/dir | <git url>"
///     }
///   }
/// }
/// ```
///
/// A subsequent install of `<name>` must match the pinned version + sha256 or
/// it refuses; `--force` replaces the pin (upgrade path). Entries for plugins
/// installed before this file existed are simply absent — the file is created
/// on the next successful install and existing entries are preserved.
fn read_plugin_lock_entry_full(name: &str) -> Result<Option<PluginLockEntry>, String> {
    let path = plugin_lock_path();
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("plugins.lock: read {}: {error}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|error| format!("plugins.lock: invalid JSON at {}: {error}", path.display()))?;
    let plugins = json.get("plugins").and_then(serde_json::Value::as_object)
        .ok_or_else(|| "plugins.lock: 'plugins' must be an object".to_owned())?;
    let Some(entry) = plugins.get(name) else { return Ok(None); };
    let version = entry.get("version").and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("plugins.lock: entry '{name}' missing version"))?;
    let sha256 = normalize_plugin_install_sha256(entry.get("sha256").and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("plugins.lock: entry '{name}' missing sha256"))?)?;
    let source = entry.get("source").and_then(serde_json::Value::as_str).map(str::to_owned);
    Ok(Some(PluginLockEntry { version: version.to_owned(), sha256, source }))
}

/// All lock-pinned plugin names (empty when the lock file is absent or
/// malformed) — consumed by the missing-plugin dispatcher fallthrough and
/// `maw doctor`.
fn plugin_lock_pinned_names() -> Vec<String> {
    let path = plugin_lock_path();
    let Ok(text) = std::fs::read_to_string(&path) else { return Vec::new() };
    let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) else { return Vec::new() };
    json.get("plugins")
        .and_then(serde_json::Value::as_object)
        .map_or_else(Vec::new, |plugins| plugins.keys().cloned().collect())
}

/// Record the resolved pin of a successful install into plugins.lock —
/// creates the file (schema 1) if absent, preserves existing entries, and
/// replaces the entry for this plugin. No-op when the install produced no
/// verifiable sha256 (unpinned local dev dirs).
fn record_plugin_install_pin(
    summary: &maw_plugin_manifest::PluginInstallSummary,
    resolved_sha256: Option<&str>,
    lock_source: &str,
) -> Result<(), String> {
    let Some(sha256) = resolved_sha256 else { return Ok(()) };
    let path = plugin_lock_path();
    let mut root = if path.exists() {
        let text = std::fs::read_to_string(&path)
            .map_err(|error| format!("plugins.lock: read {}: {error}", path.display()))?;
        serde_json::from_str::<serde_json::Value>(&text)
            .map_err(|error| format!("plugins.lock: invalid JSON at {}: {error}", path.display()))?
    } else {
        serde_json::json!({ "schema": 1, "plugins": {} })
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| "plugins.lock: top level must be an object".to_owned())?;
    let plugins = object
        .entry("plugins")
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .ok_or_else(|| "plugins.lock: 'plugins' must be an object".to_owned())?;
    plugins.insert(
        summary.name.clone(),
        serde_json::json!({
            "version": summary.version,
            "sha256": sha256,
            "source": lock_source,
        }),
    );
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("plugins.lock: create dir {}: {error}", parent.display()))?;
    }
    let mut text = serde_json::to_string_pretty(&root)
        .map_err(|error| format!("plugins.lock: serialize failed: {error}"))?;
    text.push('\n');
    std::fs::write(&path, text)
        .map_err(|error| format!("plugins.lock: write {}: {error}", path.display()))
}

/// Lock `source` string for a git install. GitHub URLs collapse to the
/// re-installable shorthand `github:owner/repo[@ref][/subpath]`; other URLs
/// stay verbatim with `@ref`/`#subpath` suffixes.
fn git_install_lock_source(
    url: &str,
    subpath: Option<&std::path::Path>,
    reference: Option<&str>,
) -> String {
    let subpath = subpath.map(|path| path.display().to_string());
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let mut source = format!("github:{}", rest.trim_end_matches(".git"));
        if let Some(reference) = reference {
            source.push('@');
            source.push_str(reference);
        }
        if let Some(subpath) = &subpath {
            source.push('/');
            source.push_str(subpath);
        }
        return source;
    }
    let mut source = url.to_owned();
    if let Some(reference) = reference {
        source.push('@');
        source.push_str(reference);
    }
    if let Some(subpath) = &subpath {
        source.push('#');
        source.push_str(subpath);
    }
    source
}

/// Lock `source` string for a local-directory install.
fn local_install_lock_source(source: &std::path::Path) -> String {
    let resolved = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    format!("path:{}", resolved.display())
}
