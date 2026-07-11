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

fn parse_endpoints(manifest: &Value) -> Result<Option<PluginEndpointPolicies>, String> {
    let Some(value) = manifest.get("endpoints") else {
        return Ok(None);
    };
    let raw = serde_json::from_value::<BTreeMap<String, RawEndpointPolicy>>(value.clone())
        .map_err(|_| "plugin.json: endpoints must be an object of endpoint policies".to_owned())?;
    let mut endpoints = BTreeMap::new();
    for (name, raw) in raw {
        if !is_slug(&name) {
            return Err("plugin.json: endpoints keys must be slug strings".to_owned());
        }
        endpoints.insert(name.clone(), raw.into_policy(&name)?);
    }
    Ok(Some(endpoints))
}

fn parse_secrets(manifest: &Value) -> Result<Option<PluginSecretPolicies>, String> {
    let Some(value) = manifest.get("secrets") else {
        return Ok(None);
    };
    let raw = serde_json::from_value::<PluginSecretPolicies>(value.clone())
        .map_err(|_| "plugin.json: secrets must be an object of secret policies".to_owned())?;
    for (name, policy) in &raw {
        if !is_slug(name) {
            return Err("plugin.json: secrets keys must be slug strings".to_owned());
        }
        if policy
            .env
            .as_deref()
            .is_some_and(|env| env.is_empty() || env.bytes().any(|byte| byte == b'=' || byte == 0))
            || policy.pass.as_deref().is_some_and(str::is_empty)
            || (policy.env.is_none() && policy.pass.is_none())
        {
            return Err(format!(
                "plugin.json: secrets.{name} requires non-empty env or pass"
            ));
        }
    }
    Ok(Some(raw))
}

fn validate_endpoint_capabilities(
    capabilities: Option<&[String]>,
    endpoints: Option<&PluginEndpointPolicies>,
    secrets: Option<&PluginSecretPolicies>,
) -> Result<(), String> {
    for capability in capabilities.unwrap_or(&[]) {
        if let Some(name) = capability.strip_prefix("net:fetch:") {
            if endpoints.is_none_or(|endpoints| !endpoints.contains_key(name)) {
                return Err(format!(
                    "plugin.json: capability {capability:?} references missing endpoint {name:?}"
                ));
            }
        }
        if let Some(name) = capability.strip_prefix("secret:use:") {
            if secrets.is_none_or(|secrets| !secrets.contains_key(name)) {
                return Err(format!(
                    "plugin.json: capability {capability:?} references missing secret {name:?}"
                ));
            }
            if endpoints.is_none_or(|endpoints| {
                !endpoints.values().any(|endpoint| {
                    endpoint
                        .auth
                        .as_ref()
                        .is_some_and(|auth| auth.secret == name)
                })
            }) {
                return Err(format!(
                    "plugin.json: capability {capability:?} references unbound secret {name:?}"
                ));
            }
        }
    }
    if let Some(endpoints) = endpoints {
        for (endpoint, policy) in endpoints {
            if let Some(auth) = &policy.auth {
                if secrets.is_none_or(|secrets| !secrets.contains_key(&auth.secret)) {
                    return Err(format!(
                        "plugin.json: endpoints.{endpoint}.auth references missing secret {:?}",
                        auth.secret
                    ));
                }
            }
        }
    }
    Ok(())
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

fn is_http_header_name(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn is_known_capability_namespace(namespace: &str) -> bool {
    known_capability_namespaces().contains(&namespace)
}

fn known_capability_namespaces() -> Vec<&'static str> {
    vec![
        "net", "fs", "peer", "sdk", "proc", "ffi", "tmux", "shell", "attach", "secret",
        "cli",
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

pub type PluginEndpointPolicies = BTreeMap<String, PluginEndpointPolicy>;
pub type PluginSecretPolicies = BTreeMap<String, PluginSecretPolicy>;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PluginSecretPolicy {
    pub env: Option<String>,
    pub pass: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginEndpointPolicy {
    pub base_url: Option<String>,
    pub base_url_ref: Option<String>,
    pub default_base_url: Option<String>,
    pub methods: Vec<String>,
    pub paths: Vec<EndpointPathPattern>,
    pub auth: Option<PluginEndpointAuth>,
    pub loopback_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct PluginEndpointAuth {
    pub kind: String,
    pub secret: String,
    pub header: Option<String>,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct RawEndpointPolicy {
    base_url: Option<String>,
    base_url_ref: Option<String>,
    default_base_url: Option<String>,
    methods: Option<Vec<String>>,
    paths: Vec<String>,
    auth: Option<PluginEndpointAuth>,
    #[serde(default)]
    loopback_only: bool,
}

impl RawEndpointPolicy {
    fn into_policy(self, name: &str) -> Result<PluginEndpointPolicy, String> {
        if self.base_url.as_deref().is_some_and(str::is_empty)
            || self.base_url_ref.as_deref().is_some_and(str::is_empty)
            || self.default_base_url.as_deref().is_some_and(str::is_empty)
        {
            return Err(format!(
                "plugin.json: endpoints.{name} URLs must be non-empty strings"
            ));
        }
        if self.base_url.is_none() && self.base_url_ref.is_none() {
            return Err(format!(
                "plugin.json: endpoints.{name} requires baseUrl or baseUrlRef"
            ));
        }
        let mut methods = self.methods.unwrap_or_else(|| vec!["GET".to_owned()]);
        if methods.is_empty() || methods.iter().any(String::is_empty) {
            return Err(format!(
                "plugin.json: endpoints.{name}.methods must be non-empty"
            ));
        }
        for method in &mut methods {
            *method = method.to_ascii_uppercase();
            if method.bytes().any(|byte| !byte.is_ascii_uppercase()) {
                return Err(format!(
                    "plugin.json: endpoints.{name}.methods must be HTTP tokens"
                ));
            }
        }
        if self.paths.is_empty() {
            return Err(format!(
                "plugin.json: endpoints.{name}.paths must be non-empty"
            ));
        }
        let paths = self
            .paths
            .iter()
            .map(|path| EndpointPathPattern::parse(path))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("plugin.json: endpoints.{name}.paths: {error}"))?;
        if let Some(auth) = &self.auth {
            if auth.kind.is_empty() || auth.secret.is_empty() {
                return Err(format!(
                    "plugin.json: endpoints.{name}.auth fields must be non-empty"
                ));
            }
            match auth.kind.as_str() {
                "bearer" | "discord-bot" => {}
                "api-key-header" if auth.header.as_deref().is_some_and(is_http_header_name) => {}
                "api-key-header" => {
                    return Err(format!(
                        "plugin.json: endpoints.{name}.auth.header must be an HTTP header name"
                    ));
                }
                _ => {
                    return Err(format!(
                        "plugin.json: endpoints.{name}.auth.kind must be bearer, discord-bot, or api-key-header"
                    ));
                }
            }
        }
        Ok(PluginEndpointPolicy {
            base_url: self.base_url,
            base_url_ref: self.base_url_ref,
            default_base_url: self.default_base_url,
            methods,
            paths,
            auth: self.auth,
            loopback_only: self.loopback_only,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointPathPattern {
    segments: Vec<String>,
}

impl EndpointPathPattern {
    fn parse(pattern: &str) -> Result<Self, String> {
        Ok(Self {
            segments: endpoint_path_segments(pattern)?,
        })
    }

    #[must_use]
    pub fn matches(&self, path: &str) -> bool {
        endpoint_path_segments(path).is_ok_and(|segments| {
            self.segments.len() == segments.len()
                && self
                    .segments
                    .iter()
                    .zip(segments.iter())
                    .all(|(pattern, actual)| pattern == "*" || pattern == actual)
        })
    }
}

// V1 grammar: absolute literal path segments plus full-segment `*`; no `**`, scheme/host, query, fragment, or `..`.
fn endpoint_path_segments(path: &str) -> Result<Vec<String>, String> {
    if !path.starts_with('/') || path.starts_with("//") || path.contains("://") {
        return Err("path must be absolute and must not include scheme or host".to_owned());
    }
    if path.contains('?') || path.contains('#') {
        return Err("path must not include query or fragment".to_owned());
    }
    let rest = &path[1..];
    if rest.is_empty() {
        return Ok(Vec::new());
    }
    rest.split('/')
        .map(|segment| {
            if segment.is_empty() {
                return Err("path segments must be non-empty".to_owned());
            }
            if segment == ".."
                || segment.eq_ignore_ascii_case("%2e%2e")
                || segment.eq_ignore_ascii_case(".%2e")
                || segment.eq_ignore_ascii_case("%2e.")
            {
                return Err("path must not contain .. segments".to_owned());
            }
            if segment == "**" || (segment.contains('*') && segment != "*") {
                return Err("only full-segment * wildcards are supported".to_owned());
            }
            Ok(segment.to_owned())
        })
        .collect()
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
    pub endpoints: Option<PluginEndpointPolicies>,
    pub secrets: Option<PluginSecretPolicies>,
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
    /// Host process working directory at invoke time, so plugins can resolve
    /// relative paths. `None` when it could not be determined.
    pub cwd: Option<String>,
    /// User home directory (`$HOME`) at invoke time, so plugins can locate
    /// user paths such as `~/.claude/teams`. `None` when unset.
    pub home: Option<String>,
}

impl InvokeContext {
    /// Build an invoke context, capturing the host process's cwd and `$HOME`
    /// so the plugin can learn where it is running and where the user's home
    /// is. This is the constructor invoke sites should use.
    #[must_use]
    pub fn new(source: InvokeSource, args: Vec<String>) -> Self {
        Self {
            source,
            args,
            cwd: std::env::current_dir()
                .ok()
                .map(|dir| dir.to_string_lossy().into_owned()),
            home: std::env::var_os("HOME").map(|home| home.to_string_lossy().into_owned()),
        }
    }
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
