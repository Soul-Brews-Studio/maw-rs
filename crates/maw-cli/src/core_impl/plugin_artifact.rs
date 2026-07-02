const DISPATCH_288: &[DispatcherEntry] = &[DispatcherEntry {
    command: "plugin-artifact",
    handler: Handler::Sync(pluginartifact_run_command),
}];

const PLUGINARTIFACT_USAGE: &str = "usage: maw plugin-artifact <contract|plan> [dir]\n  contract        print the ZERO-BUN plugin artifact contract\n  plan [dir]      classify one plugin project without spawning bun/maw\n";

fn pluginartifact_run_command(argv: &[String]) -> CliOutput {
    match pluginartifact_parse_args(argv).and_then(pluginartifact_dispatch) {
        Ok(output) => output,
        Err(message) if message.is_empty() => pluginartifact_ok(PLUGINARTIFACT_USAGE),
        Err(message) => CliOutput {
            code: 2,
            stdout: String::new(),
            stderr: format!("{message}\n{PLUGINARTIFACT_USAGE}"),
        },
    }
}

enum PluginArtifactAction {
    Contract,
    Plan(std::path::PathBuf),
}

fn pluginartifact_parse_args(argv: &[String]) -> Result<PluginArtifactAction, String> {
    let Some(kind) = argv.first().map(String::as_str) else {
        return Ok(PluginArtifactAction::Contract);
    };
    if matches!(kind, "--help" | "-h" | "help") {
        return Err(String::new());
    }
    if kind == "--" || kind.starts_with('-') || kind.chars().any(char::is_control) {
        return Err("plugin-artifact: subcommand must be contract or plan".to_owned());
    }
    match kind {
        "contract" => {
            if argv.len() != 1 {
                return Err("plugin-artifact contract: unexpected arguments".to_owned());
            }
            Ok(PluginArtifactAction::Contract)
        }
        "plan" => pluginartifact_parse_plan_args(&argv[1..]),
        other => Err(format!("plugin-artifact: unknown subcommand {other}")),
    }
}

fn pluginartifact_parse_plan_args(argv: &[String]) -> Result<PluginArtifactAction, String> {
    let mut dir = None;
    for value in argv {
        if value == "--" || value.starts_with('-') {
            return Err("plugin-artifact plan: dir must not start with '-' or use --".to_owned());
        }
        if dir.is_some() {
            return Err(format!("plugin-artifact plan: unexpected argument {value}"));
        }
        dir = Some(pluginartifact_validate_dir(value)?);
    }
    Ok(PluginArtifactAction::Plan(
        dir.unwrap_or_else(|| std::path::PathBuf::from(".")),
    ))
}

fn pluginartifact_dispatch(action: PluginArtifactAction) -> Result<CliOutput, String> {
    match action {
        PluginArtifactAction::Contract => Ok(pluginartifact_ok(pluginartifact_contract_text())),
        PluginArtifactAction::Plan(dir) => pluginartifact_plan_project(&dir).map(|text| pluginartifact_ok(&text)),
    }
}

fn pluginartifact_plan_project(dir: &std::path::Path) -> Result<String, String> {
    let root = pluginartifact_canonical_dir(dir)?;
    let loaded = load_manifest_from_dir(&root)
        .map_err(|message| format!("plugin-artifact plan: {message}"))?
        .ok_or_else(|| format!("plugin-artifact plan: no plugin.json in {}", root.display()))?;
    let manifest = &loaded.manifest;
    pluginartifact_validate_manifest_paths(manifest)?;
    let capabilities = pluginartifact_capabilities(manifest);
    let export = loaded.wasm_export.as_str();
    let artifact = manifest
        .artifact
        .as_ref()
        .map(|artifact| artifact.path.as_str())
        .or(manifest.wasm.as_deref())
        .or(manifest.entry.as_deref())
        .unwrap_or("-");
    let sha = manifest
        .artifact
        .as_ref()
        .and_then(|artifact| artifact.sha256.as_deref())
        .unwrap_or("required-before-install");

    if pluginartifact_is_rust_wasm(&root, &loaded) {
        return Ok(format!(
            "plugin artifact plan: rust-wasm\n  status: supported\n  project: {}\n  manifest: plugin.json\n  artifact: {artifact}\n  sha256: {sha}\n  capabilities: {capabilities}\n  runtime: extism-wasm\n  export: {export}\n  build: cargo build --release --target wasm32-unknown-unknown\n  no-bun: true\n",
            manifest.name
        ));
    }

    if pluginartifact_is_prebuilt_wasm(manifest) {
        return Ok(format!(
            "plugin artifact plan: prebuilt-wasm\n  status: supported\n  project: {}\n  manifest: plugin.json\n  artifact: {artifact}\n  sha256: {sha}\n  capabilities: {capabilities}\n  runtime: extism-wasm\n  export: {export}\n  build: not required (prebuilt artifact)\n  no-bun: true\n",
            manifest.name
        ));
    }

    if pluginartifact_is_assemblyscript_like(&root, manifest) {
        return Ok(format!(
            "plugin artifact plan: assemblyscript-ts\n  status: refused\n  project: {}\n  reason: AssemblyScript/TS builds require a pinned WASM compiler or prebuilt WASM artifact with sha256\n  action: publish plugin.json with target=wasm, entry.kind=wasm, artifact.path, artifact.sha256, capabilities, and entry.export\n  no-bun: true\n",
            manifest.name
        ));
    }

    Ok(format!(
        "plugin artifact plan: js-bun\n  status: refused\n  project: {}\n  reason: JS/TS Bun plugins are refused at ZERO-BUN cutover unless they provide a prebuilt WASM artifact\n  action: rewrite as Rust WASM or install a prebuilt WASM artifact with sha256; no Bun subprocess fallback is available\n  no-bun: true\n",
        manifest.name
    ))
}

fn pluginartifact_contract_text() -> &'static str {
    "plugin artifact contract v1\n  manifest: plugin.json\n  wasm: target=wasm plus wasm=<relative .wasm> or entry={kind:wasm,path,export}\n  artifact: artifact.path=<relative .wasm> and artifact.sha256=sha256:<hex> before install/load\n  capabilities: plugin.json capabilities array is preserved and consumed by runtime/registry gates\n  runtime: extism-wasm, export defaults to handle when entry.export is absent\n  supported: Rust-WASM first-class via cargo wasm32-unknown-unknown\n  supported: prebuilt WASM artifact with sha256 and declared capabilities/export\n  refused: AssemblyScript/TS unless a pinned compiler/prebuilt WASM artifact is supplied\n  refused: JS/TS Bun plugins; no bun subprocess or maw-js fallback is allowed\n  seam: part287 plugin-manifest consumes the same plugin.json + wasm/artifact + sha256 + capabilities + export contract\n"
}

fn pluginartifact_validate_dir(value: &str) -> Result<std::path::PathBuf, String> {
    if value.trim() != value
        || value.is_empty()
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err("plugin-artifact plan: dir must be non-empty, unpadded, not start with '-', and contain no control characters".to_owned());
    }
    let path = std::path::Path::new(value);
    if path
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err("plugin-artifact plan: dir must not contain .. segments".to_owned());
    }
    Ok(path.to_path_buf())
}

fn pluginartifact_canonical_dir(dir: &std::path::Path) -> Result<std::path::PathBuf, String> {
    dir.canonicalize()
        .map_err(|error| format!("plugin-artifact plan: invalid dir: {error}"))
}

fn pluginartifact_validate_manifest_paths(manifest: &PluginManifest) -> Result<(), String> {
    for (label, maybe_path) in [
        ("wasm", manifest.wasm.as_deref()),
        ("entry", manifest.entry.as_deref()),
        (
            "artifact.path",
            manifest.artifact.as_ref().map(|artifact| artifact.path.as_str()),
        ),
    ] {
        if let Some(value) = maybe_path {
            pluginartifact_validate_relative_path(label, value)?;
        }
    }
    Ok(())
}

fn pluginartifact_validate_relative_path(label: &str, value: &str) -> Result<(), String> {
    if value.trim() != value
        || value.is_empty()
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(format!("plugin-artifact plan: {label} must be non-empty, unpadded, not start with '-', and contain no control characters"));
    }
    let path = std::path::Path::new(value);
    if path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!(
            "plugin-artifact plan: {label} must be relative and stay inside plugin dir"
        ));
    }
    Ok(())
}

fn pluginartifact_is_rust_wasm(root: &std::path::Path, plugin: &LoadedPlugin) -> bool {
    root.join("Cargo.toml").is_file() && plugin.kind == LoadedPluginKind::Wasm
}

fn pluginartifact_is_prebuilt_wasm(manifest: &PluginManifest) -> bool {
    let Some(artifact) = manifest.artifact.as_ref() else {
        return false;
    };
    artifact.sha256.is_some()
        && pluginartifact_path_has_wasm_extension(&artifact.path)
        && (manifest.target == Some(maw_plugin_manifest::PluginTarget::Wasm)
            || manifest.wasm.is_some()
            || manifest.entry.as_ref().is_some_and(|entry| pluginartifact_path_has_wasm_extension(entry)))
}

fn pluginartifact_is_assemblyscript_like(root: &std::path::Path, _manifest: &PluginManifest) -> bool {
    root.join("asconfig.json").is_file() || root.join("assembly").is_dir()
}

fn pluginartifact_path_has_wasm_extension(value: &str) -> bool {
    std::path::Path::new(value)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("wasm"))
}

fn pluginartifact_capabilities(manifest: &PluginManifest) -> String {
    manifest
        .capabilities
        .as_ref()
        .filter(|values| !values.is_empty())
        .map_or_else(|| "[]".to_owned(), |values| values.join(","))
}

fn pluginartifact_ok(message: &str) -> CliOutput {
    CliOutput {
        code: 0,
        stdout: if message.ends_with('\n') {
            message.to_owned()
        } else {
            format!("{message}\n")
        },
        stderr: String::new(),
    }
}

#[cfg(test)]
mod pluginartifact_tests {
    use super::{pluginartifact_run_command, DISPATCH_288};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn pluginartifact_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn pluginartifact_temp(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "maw-plugin-artifact-{label}-{}-{nonce}",
            std::process::id()
        ));
        std::fs::create_dir_all(&root).expect("temp");
        root
    }

    fn pluginartifact_write_rust(root: &Path) -> PathBuf {
        let dir = root.join("route-probe");
        std::fs::create_dir_all(dir.join("target/wasm32-unknown-unknown/release"))
            .expect("target");
        std::fs::write(
            dir.join("Cargo.toml"),
            "[package]\nname=\"route_probe\"\nversion=\"0.1.0\"\nedition=\"2021\"\n",
        )
        .expect("cargo");
        std::fs::write(
            dir.join("target/wasm32-unknown-unknown/release/route_probe.wasm"),
            b"\0asm",
        )
        .expect("wasm");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"route-probe","version":"0.1.0","sdk":"*","target":"wasm","wasm":"target/wasm32-unknown-unknown/release/route_probe.wasm","capabilities":["messages:ledger"]}"#,
        )
        .expect("manifest");
        dir
    }

    fn pluginartifact_write_js(root: &Path) -> PathBuf {
        let dir = root.join("legacy-js");
        std::fs::create_dir_all(&dir).expect("plugin");
        std::fs::write(dir.join("index.ts"), "export default function handle() {}\n")
            .expect("entry");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"legacy-js","version":"1.0.0","sdk":"*","target":"js","entry":"index.ts","capabilities":["sdk:identity"]}"#,
        )
        .expect("manifest");
        dir
    }

    #[test]
    fn pluginartifact_dispatch_288_matches_part_file() {
        assert_eq!(DISPATCH_288.len(), 1);
        assert_eq!(DISPATCH_288[0].command, "plugin-artifact");
    }

    #[test]
    fn pluginartifact_contract_documents_shared_runtime_contract() {
        let out = pluginartifact_run_command(&pluginartifact_args(&["contract"]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("plugin artifact contract v1"));
        assert!(out.stdout.contains("part287 plugin-manifest consumes"));
        assert!(out.stdout.contains("no bun subprocess"));
    }

    #[test]
    fn pluginartifact_plan_rust_wasm_matches_golden() {
        let root = pluginartifact_temp("rust-golden");
        let dir = pluginartifact_write_rust(&root);
        let out = pluginartifact_run_command(&pluginartifact_args(&[
            "plan",
            dir.to_str().expect("dir"),
        ]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(
            out.stdout,
            include_str!("../../tests/fixtures/native-plugin-artifact/plan-rust-wasm.stdout")
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn pluginartifact_plan_js_bun_refusal_matches_golden() {
        let root = pluginartifact_temp("js-golden");
        let dir = pluginartifact_write_js(&root);
        let out = pluginartifact_run_command(&pluginartifact_args(&[
            "plan",
            dir.to_str().expect("dir"),
        ]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(
            out.stdout,
            include_str!("../../tests/fixtures/native-plugin-artifact/plan-js-refused.stdout")
        );
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn pluginartifact_rejects_injection_shapes_before_manifest_load() {
        for bad in ["--target", "../outside", "bad\npath"] {
            let out = pluginartifact_run_command(&pluginartifact_args(&["plan", bad]));
            assert_eq!(out.code, 2, "{bad}: {}", out.stdout);
        }
    }

    #[test]
    fn pluginartifact_rejects_manifest_paths_that_escape_plugin_dir() {
        let root = pluginartifact_temp("bad-path");
        let dir = root.join("bad");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("bad.wasm"), b"\0asm").expect("wasm");
        std::fs::write(
            dir.join("plugin.json"),
            r#"{"name":"bad","version":"1.0.0","sdk":"*","target":"wasm","entry":{"kind":"wasm","path":"bad.wasm","export":"handle"},"artifact":{"path":"../bad.wasm","sha256":"sha256:abc"}}"#,
        )
        .expect("manifest");
        let out = pluginartifact_run_command(&pluginartifact_args(&[
            "plan",
            dir.to_str().expect("dir"),
        ]));
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("artifact.path must be relative"), "{}", out.stderr);
        std::fs::remove_dir_all(root).expect("cleanup");
    }
}
