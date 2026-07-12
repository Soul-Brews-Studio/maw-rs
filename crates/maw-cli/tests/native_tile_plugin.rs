use maw_cli::{dispatcher_status, DispatchKind};
use maw_plugin_manifest::{
    invoke_plugin, load_manifest_from_dir, ExtismWasmInvokeRuntime, InvokeContext, InvokeSource,
    MawWasmHost,
};
use serde_json::json;
use std::path::PathBuf;

const PANE_FMT: &str = "#{pane_id}|||#{pane_title}|||#{@maw_tile}";
const SWAP_FMT: &str = "#{pane_index}|||#{pane_id}|||#{pane_title}|||#{pane_top}";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/native-tile/tile-plugin")
}

fn request(command: &str, args: &[&str]) -> String {
    json!({"command":command,"args":args}).to_string()
}

fn response(command: &str, args: &[&str], stdout: &str) -> String {
    json!({"ok":true,"value":{"command":command,"args":args,"stdout":stdout}}).to_string()
}

fn fake(host: MawWasmHost, command: &str, args: &[&str], stdout: &str) -> MawWasmHost {
    host.with_fake_response(
        "maw.tmux.command",
        request(command, args),
        response(command, args, stdout),
    )
}

fn plugin_host() -> MawWasmHost {
    let plugin = load_manifest_from_dir(&fixture())
        .expect("load tile fixture")
        .expect("tile fixture");
    MawWasmHost::new(&plugin)
}

fn invoke(args: &[&str], host: MawWasmHost) -> maw_plugin_manifest::InvokeResult {
    let plugin = load_manifest_from_dir(&fixture())
        .expect("load tile fixture")
        .expect("tile fixture");
    let mut runtime = ExtismWasmInvokeRuntime::default().with_host("tile", host);
    let context = InvokeContext {
        source: InvokeSource::Cli,
        args: args.iter().map(|arg| (*arg).to_owned()).collect(),
        cwd: Some("/repo".to_owned()),
        home: Some("/home/test".to_owned()),
    };
    invoke_plugin(&plugin, &context, &mut runtime)
}

fn common_host() -> MawWasmHost {
    let host = plugin_host();
    fake(host, "display-message", &["-p", "#{pane_id}"], "%1\n")
}

#[test]
fn tile_plugin_spawn_uses_managed_split_plan_and_layout() {
    let shell = "cd '.' || exit $?; export MAW_TILE_PARENT='alpha-main:1.0' MAW_TILE_ROLE='alpha-main-tile-1' MAW_TILE_INDEX='1' MAW_TILE_TOTAL='1' MAW_TILE_WINDOW='alpha-main:1' MAW_SESSION_ID='solo'; exec zsh -ic 'echo ok; exec zsh'";
    let mut host = common_host();
    for (command, args, stdout) in [
        ("display-message", vec!["-p", "#{window_id}"], "@7\n"),
        ("display-message", vec!["-t", "%1", "-p", "#{session_name}:#{window_index}.#{pane_index}"], "alpha-main:1.0\n"),
        ("display-message", vec!["-t", "%1", "-p", "#{session_name}:#{window_index}"], "alpha-main:1\n"),
        ("list-panes", vec!["-t", "@7", "-F", PANE_FMT], "%1|||lead|||\n"),
        ("split-window", vec!["-t", "%1", "-h", "-P", "-F", "#{pane_id}", shell], "%2\n"),
        ("select-pane", vec!["-t", "%2", "-T", "alpha-main-tile-1"], ""),
        ("set-option", vec!["-p", "-t", "%2", "pane-border-format", "#[fg=blue,bold] #{pane_title}"], ""),
        ("set-option", vec!["-p", "-t", "%2", "pane-active-border-style", "fg=blue"], ""),
        ("set-option", vec!["-p", "-t", "%2", "@maw_tile", "1"], ""),
        ("set-option", vec!["-p", "-t", "%2", "@maw_tile_parent", "alpha-main:1.0"], ""),
        ("set-option", vec!["-p", "-t", "%2", "@maw_tile_role", "alpha-main-tile-1"], ""),
        ("list-panes", vec!["-t", "@7", "-F", "#{pane_id}"], "%1\n%2\n"),
        ("select-layout", vec!["-t", "@7", "even-horizontal"], ""),
    ] {
        host = fake(host, command, &args, stdout);
    }

    let result = invoke(
        &["1", "--path", ".", "--cmd", "echo ok", "--session-id", "solo"],
        host,
    );

    assert!(result.ok, "{:?}", result.error);
    let output = result.output.as_deref().expect("output");
    assert!(output.contains("alpha-main-tile-1 → %2"), "{output}");
    assert!(output.contains("1 panes tiled (path, cmd)"), "{output}");
}

#[test]
fn tile_plugin_swap_and_clean_match_native_outputs() {
    let host = fake(
        fake(
            fake(common_host(), "display-message", &["-p", "#{window_id}"], "@7\n"),
            "list-panes",
            &["-t", "@7", "-F", SWAP_FMT],
            "0|||%1|||lead|||20\n1|||%2|||tile-1|||40\n2|||%3|||tile-2|||10\n",
        ),
        "swap-pane",
        &["-s", "%3", "-t", "%2"],
        "",
    );
    let swapped = invoke(&["swap", "top", "bottom"], host);
    assert!(swapped.ok, "{:?}", swapped.error);
    assert_eq!(
        swapped.output.as_deref(),
        Some("\x1b[32m✓\x1b[0m swapped tile-2 ↔ tile-1\n")
    );

    let mut host = fake(common_host(), "display-message", &["-p", "#{window_id}"], "@7\n");
    host = fake(
        host,
        "list-panes",
        &["-t", "@7", "-F", PANE_FMT],
        "%1|||lead|||\n%2|||alpha-main-tile-1|||1\n%3|||worker|||\n%4|||tile-2|||\n",
    );
    host = fake(host, "kill-pane", &["-t", "%2"], "");
    host = fake(host, "kill-pane", &["-t", "%4"], "");
    let cleaned = invoke(&["clean"], host);
    assert!(cleaned.ok, "{:?}", cleaned.error);
    let output = cleaned.output.as_deref().expect("clean output");
    assert!(output.contains("alpha-main-tile-1 (%2)"), "{output}");
    assert!(output.contains("tile-2 (%4)"), "{output}");
    assert!(output.contains("cleaned 2 tiles"), "{output}");
}

#[test]
fn tile_plugin_guard_and_dispatcher_fallthrough_are_preserved() {
    let result = invoke(&["--", "1"], common_host());
    assert!(!result.ok);
    assert!(result
        .error
        .as_deref()
        .is_some_and(|error| error.contains("-- separator is not supported")));
    assert_eq!(dispatcher_status("tile"), DispatchKind::NativeError);
}
