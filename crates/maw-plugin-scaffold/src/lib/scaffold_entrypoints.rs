/// Copy a scaffold template tree while skipping build and package artifacts.
///
/// Mirrors maw-js `copyTree`: create the destination directory, recurse into
/// subdirectories, copy files, and skip `target`, `.git`, and `node_modules`
/// entries wherever they appear.
///
/// # Errors
///
/// Returns filesystem errors from reading the source tree, creating
/// directories, or copying files.
pub fn copy_tree(src: impl AsRef<Path>, dest: impl AsRef<Path>) -> io::Result<()> {
    copy_tree_inner(src.as_ref(), dest.as_ref())
}

/// Scaffold a Rust WASM plugin from a template directory.
///
/// Mirrors maw-js `scaffoldRust`: validates the template exists, copies the
/// template tree, rewrites `Cargo.toml` package name and SDK path, writes a
/// README, and emits `plugin.json`.
///
/// # Errors
///
/// Returns filesystem errors from template lookup, tree copy, reading/writing
/// `Cargo.toml`, README, or `plugin.json`.
pub fn scaffold_rust(
    name: &str,
    dest: impl AsRef<Path>,
    template_dir: impl AsRef<Path>,
    sdk_path: &str,
) -> io::Result<()> {
    scaffold_rust_inner(name, dest.as_ref(), template_dir.as_ref(), sdk_path)
}

fn scaffold_rust_inner(
    name: &str,
    dest: &Path,
    template_dir: &Path,
    sdk_path: &str,
) -> io::Result<()> {
    if !template_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Rust template not found at {}", template_dir.display()),
        ));
    }

    copy_tree(template_dir, dest)?;

    let cargo_path = dest.join("Cargo.toml");
    let cargo = fs::read_to_string(&cargo_path)?;
    let cargo = rewrite_rust_cargo_toml(&cargo, name, sdk_path);
    fs::write(&cargo_path, cargo)?;

    fs::write(dest.join("README.md"), rust_readme(name, dest, sdk_path))?;
    fs::write(
        dest.join("plugin.json"),
        build_manifest_json(name, PluginLanguage::Rust),
    )?;
    Ok(())
}

/// Scaffold an `AssemblyScript` WASM plugin from a template directory.
///
/// Mirrors maw-js `scaffoldAs`: validates the template exists, copies the
/// template tree, rewrites package.json name when present, writes a README,
/// and emits `plugin.json`.
///
/// # Errors
///
/// Returns filesystem errors from template lookup, tree copy, reading/writing
/// `package.json`, README, or `plugin.json`, plus invalid package JSON.
pub fn scaffold_as(
    name: &str,
    dest: impl AsRef<Path>,
    template_dir: impl AsRef<Path>,
) -> io::Result<()> {
    scaffold_as_inner(name, dest.as_ref(), template_dir.as_ref())
}

fn scaffold_as_inner(name: &str, dest: &Path, template_dir: &Path) -> io::Result<()> {
    if !template_dir.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "AssemblyScript template not found at {}\n  The AS SDK is still being built — try again after the next maw update,\n  or check: https://github.com/Soul-Brews-Studio/maw-js",
                template_dir.display()
            ),
        ));
    }

    copy_tree(template_dir, dest)?;

    let package_path = dest.join("package.json");
    if package_path.exists() {
        let package = fs::read_to_string(&package_path)?;
        let package = rewrite_package_json_name(&package, name)?;
        fs::write(&package_path, package)?;
    }

    fs::write(dest.join("README.md"), as_readme(name, dest))?;
    fs::write(
        dest.join("plugin.json"),
        build_manifest_json(name, PluginLanguage::AssemblyScript),
    )?;
    Ok(())
}

/// Validate a plugin scaffold name.
///
/// Returns `None` for valid names and the maw-js error text for invalid names.
#[must_use]
pub fn validate_plugin_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("name is required".to_owned());
    }
    if !is_valid_plugin_name(name) {
        return Some(format!(
            "\"{name}\" is invalid — use lowercase letters, digits, - or _ (must start with a letter)"
        ));
    }
    None
}
