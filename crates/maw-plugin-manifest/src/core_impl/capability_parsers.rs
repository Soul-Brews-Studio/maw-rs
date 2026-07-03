/// Parse the optional `target` section.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for unsupported targets.
pub fn parse_target(manifest: &Value) -> Result<Option<PluginTarget>, String> {
    let Some(target) = manifest.get("target") else {
        return Ok(None);
    };
    let Some(target_string) = target.as_str() else {
        return Err("plugin.json: target must be a string".to_owned());
    };
    match target_string {
        "js" => Ok(Some(PluginTarget::Js)),
        "wasm" => Ok(Some(PluginTarget::Wasm)),
        _ => Err(format!(
            "plugin.json: unknown target {target} (expected \"js\" or \"wasm\")"
        )),
    }
}

/// Parse optional `capabilityNamespaces`.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for malformed namespace arrays.
pub fn parse_capability_namespaces(manifest: &Value) -> Result<Option<Vec<String>>, String> {
    let Some(namespaces) = manifest.get("capabilityNamespaces") else {
        return Ok(None);
    };
    let namespaces = parse_string_array(
        namespaces,
        "plugin.json: capabilityNamespaces must be an array of slug strings",
        true,
    )?;
    if namespaces.iter().any(|namespace| !is_slug(namespace)) {
        return Err(
            "plugin.json: capabilityNamespaces must be an array of slug strings".to_owned(),
        );
    }

    let mut deduped = Vec::new();
    for namespace in namespaces {
        if !deduped.contains(&namespace) {
            deduped.push(namespace);
        }
    }
    Ok(Some(deduped))
}

/// Parse optional `capabilities` and collect maw-js warning text for unknown namespaces.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for malformed capability arrays.
pub fn parse_capabilities(
    manifest: &Value,
    extra_namespaces: &[&str],
) -> Result<Option<PluginCapabilities>, String> {
    let Some(capabilities) = manifest.get("capabilities") else {
        return Ok(None);
    };
    let capabilities = parse_string_array(
        capabilities,
        "plugin.json: capabilities must be an array of strings",
        false,
    )?;
    let mut warnings = Vec::new();
    for capability in &capabilities {
        let namespace = capability
            .split_once(':')
            .map_or(capability.as_str(), |(namespace, _)| namespace);
        if !is_known_capability_namespace(namespace)
            && !extra_namespaces.iter().any(|extra| extra == &namespace)
        {
            let mut known = known_capability_namespaces();
            known.extend(extra_namespaces.iter().copied());
            warnings.push(format!(
                "plugin.json: unknown capability namespace \"{namespace}\" in \"{capability}\" (known: {})",
                known.join(", ")
            ));
        }
    }
    Ok(Some(PluginCapabilities {
        capabilities,
        warnings,
    }))
}

/// Parse optional `dependencies`.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for malformed dependency shapes.
pub fn parse_dependencies(manifest: &Value) -> Result<Option<PluginDependencies>, String> {
    let Some(dependencies) = manifest.get("dependencies") else {
        return Ok(None);
    };

    let plugins_value = if dependencies.is_array() {
        Some(dependencies)
    } else if let Some(object) = dependencies.as_object() {
        object.get("plugins")
    } else {
        return Err(
            "plugin.json: dependencies must be an object or array of plugin names".to_owned(),
        );
    };

    let Some(plugins_value) = plugins_value else {
        return Ok(Some(PluginDependencies { plugins: None }));
    };
    let plugins = parse_string_array(
        plugins_value,
        "plugin.json: dependencies.plugins must be an array of plugin names",
        true,
    )?;
    if plugins.iter().any(|plugin| !is_slug(plugin)) {
        return Err(
            "plugin.json: dependencies.plugins must be an array of plugin names".to_owned(),
        );
    }
    Ok(Some(PluginDependencies {
        plugins: Some(plugins),
    }))
}

/// Parse optional `artifact`.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for malformed artifact shapes.
pub fn parse_artifact(manifest: &Value) -> Result<Option<PluginArtifact>, String> {
    let Some(artifact) = manifest.get("artifact") else {
        return Ok(None);
    };
    let Some(artifact) = artifact.as_object() else {
        return Err("plugin.json: artifact must be an object".to_owned());
    };

    let Some(path) = artifact
        .get("path")
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
    else {
        return Err("plugin.json: artifact.path must be a non-empty string".to_owned());
    };

    let sha256_value = artifact
        .get("sha256")
        .ok_or_else(|| "plugin.json: artifact.sha256 must be a string or null".to_owned())?;
    let sha256 = if sha256_value.is_null() {
        None
    } else {
        Some(
            sha256_value
                .as_str()
                .ok_or_else(|| "plugin.json: artifact.sha256 must be a string or null".to_owned())?
                .to_owned(),
        )
    };

    Ok(Some(PluginArtifact {
        path: path.to_owned(),
        sha256,
    }))
}

/// Parse optional `tier`.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for unknown tiers.
pub fn parse_tier(manifest: &Value) -> Result<Option<PluginTier>, String> {
    let Some(tier) = manifest.get("tier") else {
        return Ok(None);
    };
    let Some(tier_string) = tier.as_str() else {
        return Err(format!(
            "plugin.json: tier must be \"core\", \"standard\", or \"extra\" (got {tier})"
        ));
    };
    let tier = match tier_string {
        "core" => PluginTier::Core,
        "standard" => PluginTier::Standard,
        "extra" => PluginTier::Extra,
        _ => {
            return Err(format!(
                "plugin.json: tier must be \"core\", \"standard\", or \"extra\" (got {tier})"
            ));
        }
    };
    Ok(Some(tier))
}

/// Parse the optional `hooks` section.
///
/// # Errors
///
/// Returns maw-js-compatible validation messages for malformed hook shapes.
pub fn parse_hooks(manifest: &Value) -> Result<Option<PluginHooks>, String> {
    let Some(hooks) = manifest.get("hooks") else {
        return Ok(None);
    };
    let Some(hooks) = hooks.as_object() else {
        return Err("plugin.json: hooks must be an object".to_owned());
    };

    let gate = parse_optional_string_array(
        hooks,
        "gate",
        "plugin.json: hooks.gate must be an array of strings",
        false,
    )?;
    let filter = parse_optional_string_array(
        hooks,
        "filter",
        "plugin.json: hooks.filter must be an array of strings",
        false,
    )?;
    let on = parse_optional_string_array(
        hooks,
        "on",
        "plugin.json: hooks.on must be an array of strings",
        false,
    )?;
    let late = parse_optional_string_array(
        hooks,
        "late",
        "plugin.json: hooks.late must be an array of strings",
        false,
    )?;

    Ok(Some(PluginHooks {
        gate,
        filter,
        on,
        late,
        wake: parse_lifecycle_hook(hooks, "wake")?,
        sleep: parse_lifecycle_hook(hooks, "sleep")?,
        serve: parse_lifecycle_hook(hooks, "serve")?,
    }))
}

fn parse_lifecycle_hook(
    hooks: &Map<String, Value>,
    key: &'static str,
) -> Result<Option<PluginLifecycleHook>, String> {
    let Some(raw) = hooks.get(key) else {
        return Ok(None);
    };
    let Some(hook) = raw.as_object() else {
        return Err(format!("plugin.json: hooks.{key} must be an object"));
    };

    let script = match hook.get("script") {
        Some(value) => Some(
            value
                .as_str()
                .filter(|script| !script.is_empty())
                .ok_or_else(|| {
                    format!("plugin.json: hooks.{key}.script must be a non-empty string")
                })?
                .to_owned(),
        ),
        None => None,
    };

    let handler = match hook.get("handler") {
        Some(value) => Some(
            value
                .as_str()
                .filter(|handler| !handler.is_empty())
                .ok_or_else(|| {
                    format!("plugin.json: hooks.{key}.handler must be a non-empty string")
                })?
                .to_owned(),
        ),
        None => None,
    };

    let ensures = parse_optional_string_array(
        hook,
        "ensures",
        &format!("plugin.json: hooks.{key}.ensures must be an array of non-empty strings"),
        true,
    )?;

    let policy = match hook.get("policy") {
        Some(Value::String(value)) if value == "best-effort" => Some(HookPolicy::BestEffort),
        Some(Value::String(value)) if value == "fail-fast" => Some(HookPolicy::FailFast),
        Some(_) => {
            return Err(format!(
                "plugin.json: hooks.{key}.policy must be \"best-effort\" or \"fail-fast\""
            ));
        }
        None => None,
    };

    Ok(Some(PluginLifecycleHook {
        script,
        handler,
        ensures,
        policy,
    }))
}

fn is_slug(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

fn is_known_capability_namespace(namespace: &str) -> bool {
    known_capability_namespaces().contains(&namespace)
}

fn known_capability_namespaces() -> Vec<&'static str> {
    vec![
        "net", "fs", "peer", "sdk", "proc", "ffi", "tmux", "shell", "attach",
    ]
}

fn parse_optional_string_array(
    object: &Map<String, Value>,
    key: &str,
    error: &str,
    reject_empty: bool,
) -> Result<Option<Vec<String>>, String> {
    object
        .get(key)
        .map(|value| parse_string_array(value, error, reject_empty))
        .transpose()
}

fn parse_string_array(
    value: &Value,
    error: &str,
    reject_empty: bool,
) -> Result<Vec<String>, String> {
    let Some(values) = value.as_array() else {
        return Err(error.to_owned());
    };
    let mut parsed = Vec::with_capacity(values.len());
    for value in values {
        let Some(item) = value.as_str() else {
            return Err(error.to_owned());
        };
        if reject_empty && item.is_empty() {
            return Err(error.to_owned());
        }
        parsed.push(item.to_owned());
    }
    Ok(parsed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub weight: Option<u64>,
    pub tier: Option<PluginTier>,
    pub wasm: Option<String>,
    pub entry: Option<String>,
    pub entry_export: Option<String>,
    pub sdk: String,
    pub cli: Option<PluginCli>,
    pub api: Option<PluginApi>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub hooks: Option<PluginHooks>,
    pub cron: Option<PluginCron>,
    pub module: Option<PluginModule>,
    pub transport: Option<PluginTransport>,
    pub engine: Option<PluginEngine>,
    pub target: Option<PluginTarget>,
    pub capability_namespaces: Option<Vec<String>>,
    pub capabilities: Option<Vec<String>>,
    pub capability_warnings: Vec<String>,
    pub dependencies: Option<PluginDependencies>,
    pub artifact: Option<PluginArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedPlugin {
    pub manifest: PluginManifest,
    pub dir: PathBuf,
    pub wasm_path: PathBuf,
    pub entry_path: Option<PathBuf>,
    pub wasm_export: String,
    pub kind: LoadedPluginKind,
    pub disabled: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadedPluginKind {
    Ts,
    Wasm,
}

impl LoadedPluginKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ts => "ts",
            Self::Wasm => "wasm",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvokeSource {
    Cli,
    Api,
    Peer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeContext {
    pub source: InvokeSource,
    pub args: Vec<String>,
}

impl InvokeSource {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cli => "cli",
            Self::Api => "api",
            Self::Peer => "peer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvokeResult {
    pub ok: bool,
    pub output: Option<String>,
    pub error: Option<String>,
}

impl InvokeResult {
    #[must_use]
    pub const fn ok() -> Self {
        Self {
            ok: true,
            output: None,
            error: None,
        }
    }

    #[must_use]
    pub fn output(output: impl Into<String>) -> Self {
        Self {
            ok: true,
            output: Some(output.into()),
            error: None,
        }
    }

    #[must_use]
    pub fn error(error: impl Into<String>) -> Self {
        Self {
            ok: false,
            output: None,
            error: Some(error.into()),
        }
    }
}

pub trait PluginInvokeRuntime {
    fn invoke_ts(&mut self, plugin: &LoadedPlugin, ctx: &InvokeContext) -> InvokeResult;

    fn invoke_wasm(
        &mut self,
        plugin: &LoadedPlugin,
        ctx: &InvokeContext,
        wasm_bytes: &[u8],
    ) -> InvokeResult;
}
