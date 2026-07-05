/// Build plugin.json content for a scaffolded plugin.
///
/// Underscores are normalized to hyphens for slug fields, while Rust wasm crate
/// artifacts normalize hyphens to underscores like maw-js.
///
/// # Panics
///
/// Panics only if `serde_json` cannot serialize the statically constructed manifest.
#[must_use]
pub fn build_manifest_json(name: &str, lang: PluginLanguage) -> String {
    let slug = name.replace('_', "-");
    let wasm_path = match lang {
        PluginLanguage::Rust => format!(
            "./target/wasm32-unknown-unknown/release/{}.wasm",
            name.replace('-', "_")
        ),
        PluginLanguage::AssemblyScript => "./build/release.wasm".to_owned(),
    };
    let type_name = match lang {
        PluginLanguage::Rust => "Rust",
        PluginLanguage::AssemblyScript => "AssemblyScript",
    };

    let mut manifest = Map::new();
    manifest.insert("name".to_owned(), json!(slug));
    manifest.insert("version".to_owned(), json!("0.1.0"));
    manifest.insert("wasm".to_owned(), json!(wasm_path));
    manifest.insert("sdk".to_owned(), json!("^1.0.0"));
    manifest.insert(
        "description".to_owned(),
        json!(format!("{type_name} plugin: {name}")),
    );
    manifest.insert("author".to_owned(), json!(""));
    manifest.insert(
        "cli".to_owned(),
        json!({ "command": slug, "help": format!("Invoke {name}") }),
    );
    manifest.insert(
        "api".to_owned(),
        json!({ "path": format!("/api/plugins/{slug}"), "methods": ["GET", "POST"] }),
    );

    let text = serde_json::to_string_pretty(&Value::Object(manifest))
        .expect("plugin manifest JSON serialization should be infallible");
    format!("{text}\n")
}

fn rewrite_package_json_name(package: &str, name: &str) -> io::Result<String> {
    let mut value: Value = serde_json::from_str(package).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("package.json: invalid JSON: {error}"),
        )
    })?;
    match &mut value {
        Value::Object(object) => {
            object.insert("name".to_owned(), Value::String(name.to_owned()));
            let text = serde_json::to_string_pretty(&value)
                .expect("package.json serialization should be infallible");
            Ok(format!("{text}\n"))
        }
        _ => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "package.json: must be a JSON object",
        )),
    }
}

fn as_readme(name: &str, dest: &Path) -> String {
    format!(
        r#"# {name}

A maw WASM command plugin (AssemblyScript).

## Build

```bash
cd "{}"
npm install
npm run build
```

Output: `build/{name}.wasm`

## Install

```bash
maw plugin install "{}"
```
"#,
        dest.display(),
        dest.display()
    )
}

fn rewrite_rust_cargo_toml(cargo: &str, name: &str, sdk_path: &str) -> String {
    let mut rewritten = cargo
        .lines()
        .map(|line| {
            if line.starts_with("name = ") {
                format!(r#"name = "{name}""#)
            } else if line.trim_start().starts_with("maw-plugin-sdk = { path = ") {
                format!(r#"maw-plugin-sdk = {{ path = "{sdk_path}" }}"#)
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    if cargo.ends_with('\n') {
        rewritten.push('\n');
    }
    rewritten
}

fn rust_readme(name: &str, dest: &Path, sdk_path: &str) -> String {
    let crate_name = name.replace('-', "_");
    format!(
        r#"# {name}

A maw WASM command plugin (Rust).

## Build

```bash
cd "{}"
cargo build --release --target wasm32-unknown-unknown
```

Output: `target/wasm32-unknown-unknown/release/{crate_name}.wasm`

## Install

```bash
maw plugin install "{}"
```

## SDK docs

See the SDK at `{sdk_path}` for available host functions:
`maw::print`, `maw::identity`, `maw::federation`, `maw::send`, `maw::fetch`.
"#,
        dest.display(),
        dest.display()
    )
}
