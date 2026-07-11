use maw_cli::{dispatcher_status, DispatchKind};
use maw_plugin_manifest::{
    invoke_plugin, load_manifest_from_dir, ExtismWasmInvokeRuntime, InvokeContext, InvokeSource,
    MawWasmHost,
};
use serde_json::json;
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-layout/layout-plugin")
}

fn invoke(args: &[&str]) -> maw_plugin_manifest::InvokeResult {
    let plugin = load_manifest_from_dir(&fixture())
        .expect("load layout fixture")
        .expect("layout fixture");
    let request = json!({"command": "select-layout", "args": tmux_args(args)}).to_string();
    let response = json!({
        "ok": true,
        "value": {"command": "select-layout", "args": tmux_args(args), "stdout": ""}
    })
    .to_string();
    let host = MawWasmHost::new(&plugin).with_fake_response("maw.tmux.command", request, response);
    let mut runtime = ExtismWasmInvokeRuntime::default().with_host("layout", host);
    let context = InvokeContext::new(
        InvokeSource::Cli,
        args.iter().map(|arg| (*arg).to_owned()).collect(),
    );
    invoke_plugin(&plugin, &context, &mut runtime)
}

fn tmux_args(args: &[&str]) -> Vec<String> {
    if args.len() == 1 {
        vec![args[0].to_owned()]
    } else {
        vec!["-t".to_owned(), "team:work".to_owned(), args[0].to_owned()]
    }
}

#[test]
fn layout_plugin_matches_current_window_and_targeted_output() {
    let current = invoke(&["main-vertical"]);
    assert!(current.ok, "{:?}", current.error);
    assert_eq!(
        current.output.as_deref(),
        Some("layout main-vertical applied to current window\n")
    );

    let targeted = invoke(&["tiled", "--to", "team:work.2"]);
    assert!(targeted.ok, "{:?}", targeted.error);
    assert_eq!(
        targeted.output.as_deref(),
        Some("layout tiled applied to team:work\n")
    );
}

#[test]
fn layout_plugin_rejects_invalid_preset_before_host_call() {
    let result = invoke(&["broken"]);
    assert!(!result.ok);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("invalid layout")));
}

#[test]
fn layout_top_level_falls_through_while_tmux_subdispatcher_stays_native() {
    assert_eq!(dispatcher_status("layout"), DispatchKind::NativeError);
    assert_eq!(dispatcher_status("tmux"), DispatchKind::Native);
}
