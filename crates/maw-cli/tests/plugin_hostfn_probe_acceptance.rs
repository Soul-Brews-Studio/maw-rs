//! Acceptance harness for the host-fn plugin runtime work on #72 (item 6).
//!
//! The `hostfn-probe` fixture imports one maw host function (`maw.exec.run`) from
//! the pinned `@maw-rs/wasm-sdk` and calls it. Its compiled module carries an
//! `extism:host/user` import — the exact shape the toy MVP WASM runtime rejects.
//!
//! Two dispatch paths exist for a compiled plugin:
//!   * `plugin-manifest invoke` -> the real `ExtismWasmInvokeRuntime` (host fns wired)
//!   * `maw <command>`          -> `dispatch_cli_plugin` on the MVP runtime (no imports)
//!
//! `probe_runs_via_manifest_invoke` asserts the internal path already runs the probe
//! today. `probe_runs_via_cli_dispatch` is the acceptance proof for the user-facing
//! path: it is `#[ignore]` until the sibling rt-dispatch change swaps CLI dispatch to
//! the Extism runtime, at which point removing the `#[ignore]` (or running
//! `cargo test -p maw-cli --test plugin_hostfn_probe_acceptance -- --ignored`) turns it
//! into a passing gate. `probe_builds_via_pipeline` re-derives the artifact through the
//! real `maw plugin build` pipeline and needs the `AssemblyScript` toolchain, so it is
//! `#[ignore]` too.

use maw_cli::run_cli;
use std::fs::{create_dir_all, read_to_string, write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostfn-probe")
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-hostfn-probe-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}

fn copy_tree(src: &Path, dst: &Path) {
    create_dir_all(dst).expect("dst dir");
    for entry in std::fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            std::fs::copy(&from, &to).expect("copy file");
        }
    }
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

/// Stage the fixture's ship-tier form (built `plugin.wasm` + wasm manifest) as a
/// single-plugin scan dir: `<root>/plugins/hostfnprobe/`.
fn stage_ship_plugin(root: &Path) -> PathBuf {
    let plugins = root.join("plugins");
    let plugin = plugins.join("hostfnprobe");
    copy_tree(&fixture_dir(), &plugin);
    std::fs::remove_file(plugin.join("plugin.source.json")).expect("drop source manifest");
    plugins
}

#[test]
fn probe_runs_via_manifest_invoke() {
    // Acceptance proof that runs today: a host-fn-importing plugin executes through
    // the real Extism runtime reached by `plugin-manifest invoke`.
    let root = temp_dir("invoke");
    let plugins = stage_ship_plugin(&root);

    let out = run_cli(&args(&[
        "plugin-manifest",
        "invoke",
        "--scan-dir",
        &plugins.display().to_string(),
        "--plugin",
        "hostfnprobe",
        "--plan-json",
    ]));

    assert_eq!(
        out.code, 0,
        "stderr: {}\nstdout: {}",
        out.stderr, out.stdout
    );
    assert!(
        out.stdout.contains(r#""ok":true"#),
        "invoke result not ok: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""mode":"extism-wasm""#),
        "not the extism runtime: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""pluginKind":"wasm""#),
        "not wasm plugin kind: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains(r#""wasmExport":"handle""#),
        "handle export missing: {}",
        out.stdout
    );

    std::fs::remove_dir_all(root).ok();
}

#[test]
fn probe_runs_via_cli_dispatch() {
    // Acceptance proof for the user-facing path: a host-fn-importing WASM plugin must
    // load and run through `dispatch_cli_plugin` on the Extism runtime (#72).
    let _guard = env_lock().lock().expect("env lock");
    let previous = std::env::var_os("MAW_PLUGINS_DIR");
    let root = temp_dir("dispatch");
    let plugins = stage_ship_plugin(&root);
    std::env::set_var("MAW_PLUGINS_DIR", &plugins);

    let out = run_cli(&args(&["hostfnprobe"]));

    match previous {
        Some(value) => std::env::set_var("MAW_PLUGINS_DIR", value),
        None => std::env::remove_var("MAW_PLUGINS_DIR"),
    }

    assert_eq!(
        out.code, 0,
        "CLI dispatch of host-fn plugin failed: {}\n{}",
        out.stderr, out.stdout
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
#[ignore = "requires the AssemblyScript toolchain: run `npm ci` in packages/wasm-sdk first"]
fn probe_builds_via_pipeline() {
    // Re-derive the artifact through the real `maw plugin build` pipeline: proves that
    // asc resolves the pinned @maw-rs/wasm-sdk (and the @extism/as-pdk it re-exports)
    // with no per-plugin node_modules, and that the result runs via the Extism runtime.
    let root = temp_dir("build");
    let plugin = root.join("hostfnprobe");
    copy_tree(&fixture_dir(), &plugin);
    // Start from source form so the build actually compiles the .ts.
    let source_manifest =
        read_to_string(plugin.join("plugin.source.json")).expect("source manifest");
    write(plugin.join("plugin.json"), source_manifest).expect("reset manifest to source form");
    std::fs::remove_file(plugin.join("plugin.source.json")).ok();
    std::fs::remove_file(plugin.join("plugin.wasm")).ok();

    let build = run_cli(&args(&["plugin", "build", &plugin.display().to_string()]));
    assert_eq!(
        build.code, 0,
        "plugin build failed: {}\n{}",
        build.stderr, build.stdout
    );
    assert!(
        build.stdout.contains("ship tier ready: plugin.wasm"),
        "unexpected build output: {}",
        build.stdout
    );
    assert!(
        plugin.join("plugin.wasm").is_file(),
        "pipeline did not emit plugin.wasm"
    );
    let ship_manifest = read_to_string(plugin.join("plugin.json")).expect("built manifest");
    assert!(
        ship_manifest.contains(r#""target": "wasm""#),
        "manifest not ship tier: {ship_manifest}"
    );
    assert!(
        ship_manifest.contains(r#""sha256": "sha256:"#),
        "manifest missing artifact sha256: {ship_manifest}"
    );

    let invoke = run_cli(&args(&[
        "plugin-manifest",
        "invoke",
        "--scan-dir",
        &root.display().to_string(),
        "--plugin",
        "hostfnprobe",
        "--plan-json",
    ]));
    assert_eq!(
        invoke.code, 0,
        "invoke of freshly built probe failed: {}\n{}",
        invoke.stderr, invoke.stdout
    );
    assert!(
        invoke.stdout.contains(r#""ok":true"#),
        "built probe did not run: {}",
        invoke.stdout
    );
    assert!(
        invoke.stdout.contains(r#""mode":"extism-wasm""#),
        "not the extism runtime: {}",
        invoke.stdout
    );

    std::fs::remove_dir_all(root).ok();
}
