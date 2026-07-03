use maw_plugin_manifest::hash_file;
use serde_json::json;
use std::{fs, path::Path, process::Command};

fn temp_dir(label: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "maw-rs-plugin-build-{label}-{}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn copy_tree(src: &Path, dest: &Path) {
    fs::create_dir_all(dest).expect("create dest");
    for entry in fs::read_dir(src).expect("read fixture") {
        let entry = entry.expect("entry");
        let name = entry.file_name();
        if name == "target" {
            continue;
        }
        let src_path = entry.path();
        let dest_path = dest.join(name);
        if src_path.is_dir() {
            copy_tree(&src_path, &dest_path);
        } else {
            fs::copy(&src_path, &dest_path).expect("copy fixture file");
        }
    }
}

const WASM_HANDLE_ZERO: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x07, 0x01, 0x60, 0x02, 0x7f, 0x7f, 0x01,
    0x7f, 0x03, 0x02, 0x01, 0x00, 0x05, 0x03, 0x01, 0x00, 0x01, 0x07, 0x13, 0x02, 0x06, 0x6d, 0x65,
    0x6d, 0x6f, 0x72, 0x79, 0x02, 0x00, 0x06, 0x68, 0x61, 0x6e, 0x64, 0x6c, 0x65, 0x00, 0x00, 0x0a,
    0x06, 0x01, 0x04, 0x00, 0x41, 0x00, 0x0b,
];

fn normalize_plugin_build_stdout(stdout: &str) -> String {
    let mut normalized = stdout
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("sha256: sha256:") {
                "  sha256: sha256:cd5d4935a48c0672cb06407bb443bc0087aff947c6b864bac886982c73b3027f"
                    .to_owned()
            } else {
                line.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    normalized.push('\n');
    normalized
}

#[cfg(unix)]
fn write_executable(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;

    fs::write(path, body).expect("write executable");
    let mut permissions = fs::metadata(path).expect("metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("chmod executable");
}

#[cfg(unix)]
fn write_fake_asc(sdk_dir: &Path) {
    fs::create_dir_all(sdk_dir).expect("sdk dir");
    fs::write(sdk_dir.join("package.json"), r#"{"name":"fake-wasm-sdk"}"#).expect("sdk package");
    let bin = sdk_dir.join("node_modules/.bin");
    fs::create_dir_all(&bin).expect("asc bin");
    write_executable(
        &bin.join("asc"),
        "#!/bin/sh\nout=\nwhile [ \"$#\" -gt 0 ]; do\n  if [ \"$1\" = \"--outFile\" ]; then\n    shift\n    out=$1\n  fi\n  shift\ndone\nif [ -z \"$out\" ]; then\n  echo 'missing --outFile' >&2\n  exit 64\nfi\nmkdir -p \"$(dirname \"$out\")\"\ncp \"$MAW_TEST_ASC_WASM\" \"$out\"\n",
    );
}

#[test]
fn plugin_build_route_a_builds_dist_and_extism_loads_fixture() {
    let root = temp_dir("route-a");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/native-plugin-build/plugin-build-rust");
    let plugin_dir = root.join("plugin-build-rust");
    copy_tree(&fixture, &plugin_dir);

    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["plugin", "build", plugin_dir.to_str().expect("utf8 path")])
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .expect("run maw-rs plugin build");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        normalize_plugin_build_stdout(&stdout),
        include_str!("fixtures/native-plugin-build/plugin-build-rust.stdout")
    );
    assert!(stdout.contains("sha256: sha256:"));
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let wasm_path = plugin_dir.join("target/wasm32-unknown-unknown/release/route_probe.wasm");
    assert!(
        wasm_path.is_file(),
        "wasm should be produced at manifest wasm path"
    );
    let dist_wasm = plugin_dir.join("dist/plugin.wasm");
    assert!(dist_wasm.is_file(), "dist wasm should be emitted");
    let dist_manifest =
        fs::read_to_string(plugin_dir.join("dist/plugin.json")).expect("dist manifest");
    assert!(dist_manifest.contains(r#""artifact""#), "{dist_manifest}");
    assert!(dist_manifest.contains("sha256:"), "{dist_manifest}");
    assert!(
        dist_manifest.contains(r#""cli""#),
        "caps should be preserved: {dist_manifest}"
    );

    let plugin = maw_plugin_manifest::load_manifest_from_dir(&plugin_dir.join("dist"))
        .expect("load dist manifest")
        .expect("plugin loaded");
    assert_eq!(
        plugin
            .manifest
            .target
            .map(maw_plugin_manifest::PluginTarget::as_str),
        Some("wasm")
    );
    assert_eq!(plugin.kind.as_str(), "wasm");
    let mut runtime = maw_plugin_manifest::ExtismWasmInvokeRuntime::default();
    let result = maw_plugin_manifest::invoke_plugin(
        &plugin,
        &maw_plugin_manifest::InvokeContext {
            source: maw_plugin_manifest::InvokeSource::Cli,
            args: vec!["probe".to_owned()],
            cwd: None,
            home: None,
        },
        &mut runtime,
    );
    assert!(result.ok, "invoke error: {:?}", result.error);
    assert_eq!(result.output.as_deref(), Some("route-probe:called"));
}

#[test]
#[cfg(unix)]
fn plugin_build_ts_missing_local_toolchain_does_not_delegate_to_maw_or_bun() {
    let root = temp_dir("fake-maw-proof");
    let bin = root.join("bin");
    let sdk_dir = root.join("wasm-sdk");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(&sdk_dir).expect("sdk dir");
    fs::write(sdk_dir.join("package.json"), r#"{"name":"fake-wasm-sdk"}"#).expect("sdk package");
    write_executable(&bin.join("maw"), "#!/bin/sh\necho DELEGATED-MAW\nexit 37\n");
    write_executable(&bin.join("bun"), "#!/bin/sh\necho DELEGATED-BUN\nexit 38\n");

    let plugin_dir = root.join("ts-plugin");
    fs::create_dir_all(&plugin_dir).expect("plugin dir");
    fs::write(plugin_dir.join("index.ts"), "export function handle() {}\n").expect("entry");
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{"name":"ts-plugin","version":"1.0.0","sdk":"*","target":"js","entry":"index.ts"}"#,
    )
    .expect("manifest");

    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["plugin", "build", plugin_dir.to_str().expect("utf8 path")])
        .env("PATH", path)
        .env("MAW_WASM_SDK_DIR", &sdk_dir)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs plugin build");
    assert_eq!(output.status.code(), Some(2));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.is_empty(), "stdout: {stdout}");
    assert!(
        stderr.contains("AssemblyScript compiler not found"),
        "{stderr}"
    );
    assert!(stderr.contains("npm ci --prefix"), "{stderr}");
    assert!(!stdout.contains("DELEGATED-MAW"), "{stdout}");
    assert!(!stderr.contains("DELEGATED-MAW"), "{stderr}");
    assert!(!stdout.contains("DELEGATED-BUN"), "{stdout}");
    assert!(!stderr.contains("DELEGATED-BUN"), "{stderr}");
}

#[test]
#[cfg(unix)]
fn plugin_build_ts_assemblyscript_fixture_graduates_to_wasm_and_dispatch_avoids_bun() {
    let root = temp_dir("ts-ship");
    let sdk_dir = root.join("wasm-sdk");
    let plugins_root = root.join("plugins");
    let plugin_dir = plugins_root.join("ship-demo");
    let bin_dir = root.join("bin");
    fs::create_dir_all(plugin_dir.join("src")).expect("plugin src");
    fs::create_dir_all(&bin_dir).expect("bin dir");
    write_fake_asc(&sdk_dir);
    let wasm_fixture = root.join("fixture.wasm");
    fs::write(&wasm_fixture, WASM_HANDLE_ZERO).expect("wasm fixture");
    fs::write(
        plugin_dir.join("src/plugin.ts"),
        "export function myAbort(message: string | null, fileName: string | null, lineNumber: u32, columnNumber: u32): void {}\nexport function handle(): i32 { return 0; }\n",
    )
    .expect("entry");
    fs::write(
        plugin_dir.join("plugin.json"),
        json!({
            "name": "ship-demo",
            "version": "1.0.0",
            "sdk": "*",
            "runtime": "bun-dev",
            "target": "js",
            "entry": "src/plugin.ts",
            "cli": {
                "command": "ship-demo",
                "help": "maw ship-demo"
            }
        })
        .to_string(),
    )
    .expect("manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["plugin", "build", plugin_dir.to_str().expect("utf8 path")])
        .env("MAW_WASM_SDK_DIR", &sdk_dir)
        .env("MAW_TEST_ASC_WASM", &wasm_fixture)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs plugin build");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.starts_with("ship tier ready: plugin.wasm (sha256 "),
        "{stdout}"
    );
    assert!(
        stdout.contains("remove \"runtime\": \"bun-dev\" or leave it as dev fallback"),
        "{stdout}"
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());

    let wasm_path = plugin_dir.join("plugin.wasm");
    assert!(wasm_path.is_file(), "WASM artifact should be emitted");
    let sha256 = hash_file(&wasm_path).expect("wasm hash");
    let manifest: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(plugin_dir.join("plugin.json")).expect("manifest"),
    )
    .expect("manifest json");
    assert_eq!(manifest["runtime"], "bun-dev");
    assert_eq!(manifest["target"], "wasm");
    assert_eq!(
        manifest["entry"],
        json!({"kind":"wasm","path":"plugin.wasm","export":"handle"})
    );
    assert_eq!(
        manifest["artifact"],
        json!({"path":"./plugin.wasm","sha256":sha256})
    );

    let bun_args = root.join("bun-args");
    write_executable(
        &bin_dir.join("bun"),
        "#!/bin/sh\nprintf invoked > \"$BUN_SHIM_ARGS\"\nexit 39\n",
    );
    let path = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let dispatched = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["ship-demo", "hello"])
        .env("PATH", path)
        .env("MAW_PLUGINS_DIR", &plugins_root)
        .env("BUN_SHIM_ARGS", &bun_args)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs plugin dispatch");
    assert!(
        dispatched.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&dispatched.stderr)
    );
    assert!(String::from_utf8_lossy(&dispatched.stdout).is_empty());
    assert!(String::from_utf8_lossy(&dispatched.stderr).is_empty());
    assert!(
        !bun_args.exists(),
        "fake PATH bun was invoked after manifest graduated to WASM"
    );
}

#[test]
#[cfg(unix)]
fn plugin_build_ts_missing_assemblyscript_toolchain_reports_install_command() {
    let root = temp_dir("ts-missing-asc");
    let sdk_dir = root.join("wasm-sdk");
    let plugin_dir = root.join("plugin");
    fs::create_dir_all(plugin_dir.join("src")).expect("plugin src");
    fs::create_dir_all(&sdk_dir).expect("sdk dir");
    fs::write(sdk_dir.join("package.json"), r#"{"name":"fake-wasm-sdk"}"#).expect("sdk package");
    fs::write(
        plugin_dir.join("src/plugin.ts"),
        "export function handle(): i32 { return 0; }\n",
    )
    .expect("entry");
    fs::write(
        plugin_dir.join("plugin.json"),
        r#"{"name":"ship-demo","version":"1.0.0","sdk":"*","target":"js","entry":"src/plugin.ts"}"#,
    )
    .expect("manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_maw-rs"))
        .args(["plugin", "build", plugin_dir.to_str().expect("utf8 path")])
        .env("MAW_WASM_SDK_DIR", &sdk_dir)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs plugin build");
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stdout).is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("AssemblyScript compiler not found"),
        "{stderr}"
    );
    assert!(stderr.contains("npm ci --prefix"), "{stderr}");
    assert!(!plugin_dir.join("plugin.wasm").exists());
}
