use std::collections::BTreeMap;
use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{
    invoke_plugin, ApiMethod, CliFlagKind, InvokeContext, InvokeResult, InvokeSource, LoadedPlugin,
    LoadedPluginKind, PluginApi, PluginCli, PluginHooks, PluginInvokeRuntime, PluginManifest,
    PluginTransport,
};

fn make_temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-registry-invoke-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
fn invoke_plugin_version_reports_effective_surfaces() {
    let root = make_temp_dir("version");
    let entry = root.join("index.ts");
    write(&entry, b"export default () => ({ ok: true });\n").expect("entry");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Ts);
    plugin.entry_path = Some(entry);
    plugin.manifest.name = "surface".to_owned();
    plugin.manifest.version = "2.3.4".to_owned();
    plugin.manifest.description = Some("surface reporter".to_owned());
    plugin.manifest.weight = Some(7);
    plugin.manifest.api = Some(PluginApi {
        path: "/api/surface".to_owned(),
        methods: vec![ApiMethod::Get],
    });
    plugin.manifest.hooks = Some(PluginHooks {
        gate: None,
        filter: None,
        on: Some(vec!["message:send".to_owned()]),
        late: None,
        wake: None,
        sleep: None,
        serve: None,
    });
    plugin.manifest.transport = Some(PluginTransport { peer: Some(true) });

    let result = invoke_plugin(&plugin, &cli(&["--version"]), &mut FakeRuntime::default());

    assert!(result.ok);
    let output = result.output.expect("output");
    assert!(output.contains("surface v2.3.4 (ts, weight:7)"));
    assert!(output.contains("surface reporter"));
    assert!(output.contains("cli:surface"));
    assert!(output.contains("api:/api/surface"));
    assert!(output.contains("hooks"));
    assert!(output.contains("peer"));
    assert!(output.contains(&format!("dir: {}", root.display())));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_help_matches_anywhere_and_renders_declared_metadata() {
    let root = make_temp_dir("help");
    let entry = root.join("index.ts");
    write(&entry, b"export const handler = () => ({ ok: true });\n").expect("entry");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Ts);
    plugin.entry_path = Some(entry);
    plugin.manifest.name = "helper".to_owned();
    plugin.manifest.version = "1.0.1".to_owned();
    plugin.manifest.description = Some("helpful plugin".to_owned());
    plugin.manifest.cli = Some(PluginCli {
        command: "helper".to_owned(),
        help: Some("maw helper <thing>".to_owned()),
        aliases: Some(vec!["hp".to_owned()]),
        flags: Some(BTreeMap::from([("--name".to_owned(), CliFlagKind::String)])),
    });
    plugin.manifest.api = Some(PluginApi {
        path: "/api/helper".to_owned(),
        methods: vec![ApiMethod::Get, ApiMethod::Post],
    });
    plugin.manifest.hooks = Some(PluginHooks {
        gate: Some(vec!["cmd:before".to_owned()]),
        filter: None,
        on: None,
        late: None,
        wake: None,
        sleep: None,
        serve: None,
    });

    let result = invoke_plugin(
        &plugin,
        &cli(&["sub", "--help"]),
        &mut FakeRuntime::default(),
    );

    assert!(result.ok);
    let output = result.output.expect("output");
    assert!(output.contains("helper v1.0.1"));
    assert!(output.contains("helpful plugin"));
    assert!(output.contains("usage: maw helper <thing>"));
    assert!(output.contains("aliases: hp"));
    assert!(output.contains("--name"));
    assert!(output.contains("string"));
    assert!(output.contains("api: GET/POST /api/helper"));
    assert!(output.contains("hooks: gate"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_version_only_matches_first_arg_and_non_cli_skips_flags() {
    let root = make_temp_dir("flag-skip");
    let plugin = make_plugin(&root, LoadedPluginKind::Wasm);

    let later_version = invoke_plugin(&plugin, &cli(&["run", "-v"]), &mut FakeRuntime::default());
    assert!(!later_version.ok);
    assert_error_contains(&later_version, "failed to read wasm");

    let non_cli = invoke_plugin(
        &plugin,
        &InvokeContext {
            source: InvokeSource::Api,
            args: vec!["-v".to_owned()],
            cwd: None,
            home: None,
        },
        &mut FakeRuntime::default(),
    );
    assert!(!non_cli.ok);
    assert_error_contains(&non_cli, "failed to read wasm");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_default_name_help_for_dispatchable_plugins() {
    let root = make_temp_dir("default-help");
    let wasm = root.join("plugin.wasm");
    write(&wasm, b"wasm bytes").expect("wasm");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Wasm);
    plugin.manifest.name = "park".to_owned();
    plugin.wasm_path = wasm;

    let result = invoke_plugin(&plugin, &cli(&["-help"]), &mut FakeRuntime::default());

    assert!(result.ok);
    let output = result.output.expect("output");
    assert!(output.contains("usage: maw park"));
    assert!(output.contains("cli: maw park"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_help_for_non_dispatchable_plugin_has_no_cli_surface() {
    let root = make_temp_dir("help-no-surface");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Ts);
    plugin.manifest.name = "dormant".to_owned();

    let result = invoke_plugin(&plugin, &cli(&["--help"]), &mut FakeRuntime::default());

    assert!(result.ok);
    let output = result.output.expect("output");
    assert!(output.contains("dormant v1.0.0"));
    assert!(!output.contains("usage:"));
    assert!(!output.contains("cli: maw dormant"));
    assert!(output.contains("surfaces:"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_version_for_non_dispatchable_plugin_reports_empty_surfaces() {
    let root = make_temp_dir("version-no-surface");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Ts);
    plugin.manifest.name = "quiet".to_owned();

    let result = invoke_plugin(&plugin, &cli(&["-v"]), &mut FakeRuntime::default());

    assert!(result.ok);
    let output = result.output.expect("output");
    assert!(output.contains("quiet v1.0.0 (ts, weight:50)"));
    assert!(output.contains("surfaces: \n"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_ts_dispatches_through_injected_runtime() {
    let root = make_temp_dir("ts-runtime");
    let entry = root.join("index.ts");
    write(&entry, b"export default () => ({ ok: true });\n").expect("entry");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Ts);
    plugin.entry_path = Some(entry);
    let mut runtime = FakeRuntime {
        ts_result: InvokeResult::output("args=a|b"),
        ..FakeRuntime::default()
    };

    let result = invoke_plugin(
        &plugin,
        &InvokeContext {
            source: InvokeSource::Api,
            args: vec!["a".to_owned(), "b".to_owned()],
            cwd: None,
            home: None,
        },
        &mut runtime,
    );

    assert_eq!(result, InvokeResult::output("args=a|b"));
    assert_eq!(runtime.ts_calls, 1);
    assert_eq!(runtime.wasm_calls, 0);
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_ts_without_entry_falls_through_to_wasm_read() {
    let root = make_temp_dir("ts-no-entry");
    let plugin = make_plugin(&root, LoadedPluginKind::Ts);

    let result = invoke_plugin(
        &plugin,
        &InvokeContext {
            source: InvokeSource::Api,
            args: Vec::new(),
            cwd: None,
            home: None,
        },
        &mut FakeRuntime::default(),
    );

    assert!(!result.ok);
    assert_error_contains(&result, "failed to read wasm");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_wasm_reads_bytes_and_hands_off_to_runtime() {
    let root = make_temp_dir("wasm-runtime");
    let wasm = root.join("plugin.wasm");
    write(&wasm, b"wasm bytes").expect("wasm");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Wasm);
    plugin.wasm_path = wasm;
    let mut runtime = FakeRuntime {
        wasm_result: InvokeResult::output("HELLO"),
        ..FakeRuntime::default()
    };

    let result = invoke_plugin(&plugin, &cli(&[]), &mut runtime);

    assert_eq!(result, InvokeResult::output("HELLO"));
    assert_eq!(runtime.ts_calls, 0);
    assert_eq!(runtime.wasm_calls, 1);
    assert_eq!(runtime.last_wasm_bytes, b"wasm bytes");
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn invoke_plugin_help_renders_peer_and_all_hook_keys() {
    let root = make_temp_dir("help-all-hooks");
    let entry = root.join("index.ts");
    write(&entry, b"export default () => ({ ok: true });\n").expect("entry");
    let mut plugin = make_plugin(&root, LoadedPluginKind::Ts);
    plugin.entry_path = Some(entry);
    plugin.manifest.name = "fullhelp".to_owned();
    plugin.manifest.transport = Some(PluginTransport { peer: Some(true) });
    plugin.manifest.hooks = Some(PluginHooks {
        gate: Some(vec!["cmd:before".to_owned()]),
        filter: Some(vec!["message".to_owned()]),
        on: Some(vec!["message:send".to_owned()]),
        late: Some(vec!["after".to_owned()]),
        wake: Some(maw_plugin_manifest::PluginLifecycleHook {
            script: None,
            handler: None,
            ensures: None,
            policy: None,
        }),
        sleep: Some(maw_plugin_manifest::PluginLifecycleHook {
            script: None,
            handler: None,
            ensures: None,
            policy: None,
        }),
        serve: Some(maw_plugin_manifest::PluginLifecycleHook {
            script: None,
            handler: None,
            ensures: None,
            policy: None,
        }),
    });

    let result = invoke_plugin(&plugin, &cli(&["--help"]), &mut FakeRuntime::default());

    assert!(result.ok);
    let output = result.output.expect("output");
    assert!(output.contains("peer: maw hey plugin:fullhelp"));
    assert!(output.contains("hooks: gate, filter, on, late, wake, sleep, serve"));
    remove_dir_all(root).expect("cleanup");
}

#[test]
fn loaded_plugin_kind_as_str_covers_wasm_variant() {
    assert_eq!(LoadedPluginKind::Ts.as_str(), "ts");
    assert_eq!(LoadedPluginKind::Wasm.as_str(), "wasm");
}

fn make_plugin(dir: &Path, kind: LoadedPluginKind) -> LoadedPlugin {
    LoadedPlugin {
        manifest: PluginManifest {
            name: "plug".to_owned(),
            version: "1.0.0".to_owned(),
            weight: None,
            tier: None,
            wasm: None,
            entry: None,
            entry_export: None,
            sdk: "*".to_owned(),
            cli: None,
            api: None,
            description: None,
            author: None,
            hooks: None,
            cron: None,
            module: None,
            transport: None,
            engine: None,
            target: None,
            capability_namespaces: None,
            capabilities: None,
            endpoints: None,
            secrets: None,
            capability_warnings: Vec::new(),
            dependencies: None,
            artifact: None,
        },
        dir: dir.to_path_buf(),
        wasm_path: dir.join("missing.wasm"),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind,
        disabled: false,
    }
}

fn cli(args: &[&str]) -> InvokeContext {
    InvokeContext {
        source: InvokeSource::Cli,
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: None,
        home: None,
    }
}

fn assert_error_contains(result: &InvokeResult, expected: &str) {
    let error = result.error.as_deref().unwrap_or_default();
    assert!(
        error.contains(expected),
        "{error:?} did not contain {expected:?}"
    );
}

struct FakeRuntime {
    ts_calls: usize,
    wasm_calls: usize,
    last_wasm_bytes: Vec<u8>,
    ts_result: InvokeResult,
    wasm_result: InvokeResult,
}

impl Default for FakeRuntime {
    fn default() -> Self {
        Self {
            ts_calls: 0,
            wasm_calls: 0,
            last_wasm_bytes: Vec::new(),
            ts_result: InvokeResult::ok(),
            wasm_result: InvokeResult::ok(),
        }
    }
}

impl PluginInvokeRuntime for FakeRuntime {
    fn invoke_ts(&mut self, _plugin: &LoadedPlugin, _ctx: &InvokeContext) -> InvokeResult {
        self.ts_calls += 1;
        self.ts_result.clone()
    }

    fn invoke_wasm(
        &mut self,
        _plugin: &LoadedPlugin,
        _ctx: &InvokeContext,
        wasm_bytes: &[u8],
    ) -> InvokeResult {
        self.wasm_calls += 1;
        self.last_wasm_bytes = wasm_bytes.to_vec();
        self.wasm_result.clone()
    }
}
