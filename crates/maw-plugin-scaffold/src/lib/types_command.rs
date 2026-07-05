#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginCreateRequest {
    pub name: Option<String>,
    pub rust: bool,
    pub assembly_script: bool,
    pub dest: std::path::PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PluginCreateError {
    MissingType,
    ConflictingTypes,
    MissingName,
    InvalidName(String),
    DestinationExists(std::path::PathBuf),
    Scaffold(String),
}

impl std::fmt::Display for PluginCreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingType => write!(
                f,
                "usage: maw plugin create [--rust | --as] <name> [--here]\n  Specify either --rust or --as"
            ),
            Self::ConflictingTypes => write!(f, "  Specify --rust or --as, not both"),
            Self::MissingName => write!(f, "usage: maw plugin create [--rust | --as] <name> [--here]"),
            Self::InvalidName(error) => write!(f, "✗ Invalid plugin name: {error}"),
            Self::DestinationExists(dest) => write!(f, "✗ Destination already exists: {}", dest.display()),
            Self::Scaffold(error) => write!(f, "✗ {error}"),
        }
    }
}

impl std::error::Error for PluginCreateError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginLanguage {
    Rust,
    AssemblyScript,
}

/// Execute the pure command guard and dispatch for `maw plugin create`.
///
/// This mirrors the command-boundary checks in maw-js `cmdPluginCreate`, but
/// returns typed errors instead of calling `process.exit(1)`.
///
/// # Errors
///
/// Returns validation, destination-exists, or scaffold filesystem errors.
pub fn cmd_plugin_create(
    request: &PluginCreateRequest,
    rust_template_dir: impl AsRef<Path>,
    as_template_dir: impl AsRef<Path>,
    sdk_path: &str,
) -> Result<(), PluginCreateError> {
    cmd_plugin_create_inner(
        request,
        rust_template_dir.as_ref(),
        as_template_dir.as_ref(),
        sdk_path,
    )
}

fn cmd_plugin_create_inner(
    request: &PluginCreateRequest,
    rust_template_dir: &Path,
    as_template_dir: &Path,
    sdk_path: &str,
) -> Result<(), PluginCreateError> {
    if !request.rust && !request.assembly_script {
        return Err(PluginCreateError::MissingType);
    }
    if request.rust && request.assembly_script {
        return Err(PluginCreateError::ConflictingTypes);
    }
    let Some(name) = request.name.as_deref() else {
        return Err(PluginCreateError::MissingName);
    };
    if let Some(error) = validate_plugin_name(name) {
        return Err(PluginCreateError::InvalidName(error));
    }
    if request.dest.exists() {
        return Err(PluginCreateError::DestinationExists(request.dest.clone()));
    }

    let result = if request.rust {
        scaffold_rust(name, &request.dest, rust_template_dir, sdk_path)
    } else {
        scaffold_as(name, &request.dest, as_template_dir)
    };
    result.map_err(|error| PluginCreateError::Scaffold(error.to_string()))
}
