use std::fs::{create_dir_all, remove_dir_all, write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_cli::{run_cli, CliOutput};

const PEER_KEY: &str = "feedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedfacefeedface";
const FROM: &str = "mawjs:m5";
const NOW: &str = "1700000000";

fn run(args: &[&str]) -> CliOutput {
    run_cli(&args.iter().map(|arg| (*arg).to_owned()).collect::<Vec<_>>())
}

fn assert_ok_contains(args: &[&str], expected: &str) -> CliOutput {
    let output = run(args);
    assert_eq!(output.code, 0, "stderr for {args:?}: {}", output.stderr);
    assert!(
        output.stdout.contains(expected),
        "stdout for {args:?} did not contain {expected:?}: {}",
        output.stdout
    );
    assert!(
        output.stderr.is_empty(),
        "stderr for {args:?}: {}",
        output.stderr
    );
    output
}

fn assert_usage_contains(args: &[&str], expected: &str) {
    let output = run(args);
    assert_eq!(output.code, 2, "stdout for {args:?}: {}", output.stdout);
    assert!(
        output.stderr.contains(expected),
        "stderr for {args:?} did not contain {expected:?}: {}",
        output.stderr
    );
    assert!(
        output.stdout.is_empty(),
        "stdout for {args:?}: {}",
        output.stdout
    );
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-cli-cov-agent-cli-0308-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("create temp dir");
    dir
}

#[test]
#[allow(clippy::too_many_lines)]
fn auth_0308_parser_defaults_and_value_error_edges_are_stable() {
    assert_usage_contains(&["auth"], "auth: expected sign-v1, sign-headers, verify-v1");
    assert_usage_contains(&["auth", "mystery"], "auth: unknown subcommand mystery");
    assert_usage_contains(
        &["auth", "sign-v1", "--token", "t", "--now", "soon"],
        "auth sign-v1: --now must be an integer",
    );
    assert_usage_contains(
        &["auth", "sign-headers", "--token"],
        "auth: missing --token value",
    );
    assert_usage_contains(
        &["auth", "sign-v1", "--now", NOW],
        "auth sign-v1: --token is required",
    );
    assert_usage_contains(
        &["auth", "sign-v1", "--token", "t"],
        "auth sign-v1: --now is required",
    );
    assert_usage_contains(
        &["auth", "sign-headers", "--token", "tok"],
        "auth sign-headers: --now is required",
    );
    assert_usage_contains(
        &[
            "auth",
            "verify-v1",
            "--signature",
            "sig",
            "--signed-at",
            "1",
            "--now",
            NOW,
        ],
        "auth verify-v1: --token is required",
    );
    assert_usage_contains(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            FROM,
            "--timestamp",
            NOW,
            "--now",
            NOW,
        ],
        "auth verify-v3-from: --signature-v3 is required",
    );
    assert_usage_contains(
        &["auth", "from-sign-payload", "--from", FROM],
        "auth from-sign-payload: --timestamp is required",
    );
    assert_usage_contains(
        &[
            "auth",
            "verify-v1",
            "--token",
            "t",
            "--signature",
            "sig",
            "--signed-at",
            "100",
            "--now",
            "bad",
        ],
        "auth verify-v1: --now must be an integer",
    );
    assert_usage_contains(
        &[
            "auth",
            "verify-v3-from",
            "--from",
            FROM,
            "--timestamp",
            "bad",
            "--signature-v3",
            "sig",
            "--now",
            NOW,
        ],
        "auth verify-v3-from: --timestamp must be an integer",
    );
    assert_usage_contains(
        &["auth", "from-sign-payload", "--from", FROM, "--legacy"],
        "auth from-sign-payload: --signed-at is required with --legacy",
    );
    assert_usage_contains(
        &["auth", "verify-request", "--header", "X-Maw-From"],
        "auth verify-request: --header must be key=value",
    );

    assert_ok_contains(
        &[
            "auth",
            "sign-headers",
            "--token",
            "tok",
            "--now",
            NOW,
            "--plan-json",
        ],
        "\"kind\":\"sign-headers\"",
    );
    assert_ok_contains(
        &[
            "auth",
            "sign-v3",
            "--peer-key",
            PEER_KEY,
            "--from",
            FROM,
            "--now",
            NOW,
            "--plan-json",
        ],
        "\"kind\":\"sign-v3\"",
    );
    assert_ok_contains(
        &["auth", "hash-body", "--body", "", "--plan-json"],
        "\"present\":true",
    );
}

#[test]
fn xdg_and_plugin_scaffold_0308_text_and_required_edges_are_stable() {
    assert_ok_contains(
        &["xdg", "constants"],
        "xdg constants modes=legacy,xdg,MAW_HOME",
    );
    assert_usage_contains(
        &["xdg", "constants", "--odd"],
        "xdg constants: unknown arg --odd",
    );
    assert_usage_contains(&["xdg", "paths", "--home"], "xdg: missing --home value");
    assert_usage_contains(
        &["xdg", "core-paths", "--env", "NO_EQUALS"],
        "xdg: --env must be KEY=VALUE",
    );
    assert_usage_contains(
        &["xdg", "validate-instance"],
        "xdg validate-instance: --name is required",
    );
    assert_ok_contains(
        &["xdg", "core-paths", "--home", "/tmp/maw-cov-home"],
        "/tmp/maw-cov-home/.maw",
    );
    assert_ok_contains(
        &[
            "xdg",
            "validate-instance",
            "--name",
            "bad name",
            "--plan-json",
        ],
        "\"valid\":false",
    );

    assert_ok_contains(
        &["plugin-scaffold", "constants"],
        "plugin-scaffold constants actions=validate-name,manifest",
    );
    assert_usage_contains(
        &["plugin-scaffold", "constants", "--odd"],
        "plugin-scaffold constants: unknown argument --odd",
    );
    assert_usage_contains(
        &[
            "plugin-scaffold",
            "manifest",
            "--rust",
            "--as",
            "--name",
            "agent",
        ],
        "Specify --rust or --as, not both",
    );
    assert_usage_contains(
        &["plugin-scaffold", "validate-name"],
        "plugin-scaffold validate-name: --name is required",
    );
    assert_usage_contains(
        &["plugin-scaffold", "manifest", "--rust"],
        "plugin-scaffold manifest: --name is required",
    );
    assert_usage_contains(
        &["plugin-scaffold", "manifest", "--rust", "--name"],
        "plugin-scaffold: missing --name value",
    );
    assert_usage_contains(
        &["plugin-scaffold", "manifest", "--rust", "--name", "Bad"],
        "Invalid plugin name",
    );
    assert_ok_contains(
        &[
            "plugin-scaffold",
            "validate-name",
            "--name",
            "agent",
            "--plan-json",
        ],
        "\"valid\":true",
    );
    assert_ok_contains(
        &["plugin-scaffold", "manifest", "--name", "agent", "--rust"],
        "target/wasm32-unknown-unknown",
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn plugin_manifest_0308_parse_load_discover_and_import_edges_are_stable() {
    let root = temp_dir("manifest");
    let ts_plugin = root.join("ts-plugin");
    let wasm_plugin = root.join("wasm-plugin");
    create_dir_all(&ts_plugin).expect("ts plugin dir");
    create_dir_all(&wasm_plugin).expect("wasm plugin dir");
    write(
        ts_plugin.join("plugin.json"),
        r#"{"name":"ts-plugin","version":"1.0.0","sdk":"*","target":"js","entry":"index.ts","module":{"path":"./lib.ts","exports":["publicSymbol"]},"cli":{"command":"ts-plugin","aliases":["tp"],"help":"help","flags":{"loud":"boolean"}},"api":{"path":"/ts","methods":["GET"]}}"#,
    )
    .expect("ts manifest");
    write(ts_plugin.join("index.ts"), "export default () => null;").expect("ts entry");
    write(
        ts_plugin.join("lib.ts"),
        "export const publicSymbol = 'ok';",
    )
    .expect("ts module");
    write(
        wasm_plugin.join("plugin.json"),
        r#"{"name":"wasm-plugin","version":"1.0.0","sdk":"*","wasm":"plugin.wasm","artifact":{"path":"plugin.wasm","sha256":"abc"}}"#,
    )
    .expect("wasm manifest");
    write(wasm_plugin.join("plugin.wasm"), b"wasm").expect("wasm file");

    let root_arg = root.to_string_lossy();
    assert_usage_contains(
        &["plugin-manifest"],
        "plugin-manifest: expected parse or load",
    );
    assert_usage_contains(
        &["plugin-manifest", "unknown"],
        "plugin-manifest: unknown subcommand unknown",
    );
    assert_usage_contains(
        &["plugin-manifest", "parse", "--dir", root_arg.as_ref()],
        "plugin-manifest parse: --json is required",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "import-symbol",
            "--scan-dir",
            root_arg.as_ref(),
            "--module-symbol",
            "bad",
        ],
        "--module-symbol must be name=value",
    );
    assert_usage_contains(
        &["plugin-manifest", "discover", "--use-cache"],
        "plugin-manifest discover: --scan-dir is required",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "import-symbol",
            "--scan-dir",
            root_arg.as_ref(),
            "--symbol",
            "publicSymbol",
        ],
        "plugin-manifest import-symbol: --plugin is required",
    );
    assert_usage_contains(
        &["plugin-manifest", "import-symbol", "--scan-dir"],
        "plugin-manifest: missing --scan-dir value",
    );
    assert_ok_contains(
        &[
            "plugin-manifest",
            "parse",
            "--dir",
            ts_plugin.to_string_lossy().as_ref(),
            "--json",
            r#"{"name":"inline","version":"1.0.0","sdk":"*","tier":"core","capabilityNamespaces":["fs"],"capabilities":["read"]}"#,
        ],
        "inline",
    );
    assert_ok_contains(
        &[
            "plugin-manifest",
            "parse",
            "--dir",
            ts_plugin.to_string_lossy().as_ref(),
            "--json",
            r#"{"name":"inline-json","version":"1.0.0","sdk":"*","tier":"core","capabilityNamespaces":["fs"],"capabilities":["read"]}"#,
            "--plan-json",
        ],
        "\"tier\":\"core\"",
    );
    assert_ok_contains(
        &["plugin-manifest", "load", "--dir", root_arg.as_ref()],
        "missing",
    );
    assert_ok_contains(
        &[
            "plugin-manifest",
            "load",
            "--dir",
            ts_plugin.to_string_lossy().as_ref(),
        ],
        "ts ts-plugin",
    );
    assert_ok_contains(
        &[
            "plugin-manifest",
            "discover",
            "--scan-dir",
            root_arg.as_ref(),
            "--use-cache",
        ],
        "ts-plugin",
    );
    assert_ok_contains(
        &[
            "plugin-manifest",
            "import-symbol",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "ts-plugin",
            "--symbol",
            "publicSymbol",
            "--module-symbol",
            "publicSymbol=ok",
            "--plan-json",
        ],
        "\"value\":\"ok\"",
    );

    remove_dir_all(root).expect("cleanup");
}

#[test]
fn consent_store_part17_null_and_dash_render_branches_are_stable() {
    const TRUST_ENTRY: &str =
        "from=neo,to=mawjs,action=hey,approved_at=2026-01-02T00:00:00.000Z,approved_by=human";
    const PENDING_REQ: &str = "id=req-1,from=neo,to=mawjs,action=plugin-install,summary=install,pin_hash=hash,created_at=2026-01-02T00:00:00.000Z,expires_at=2026-01-02T00:01:00.000Z,status=pending";

    assert_ok_contains(
        &[
            "consent-store",
            "trust",
            "--entry",
            TRUST_ENTRY,
            "--plan-json",
        ],
        "\"trusted\":null",
    );
    assert_ok_contains(
        &["consent-store", "trust", "--entry", TRUST_ENTRY],
        "consent-store trust trusted=- trustKey=-",
    );
    assert_ok_contains(
        &[
            "consent-store",
            "pending",
            "--request",
            PENDING_REQ,
            "--plan-json",
        ],
        "\"updated\":null",
    );
    assert_ok_contains(
        &["consent-store", "pending", "--request", PENDING_REQ],
        "consent-store pending updated=-",
    );
    assert_usage_contains(
        &["consent-store", "trust", "--entry", "=bad"],
        "consent-store: expected non-empty field name",
    );
    assert_usage_contains(
        &[
            "consent-store",
            "trust",
            "--entry",
            "from=neo,to=mawjs,action=hey,approved_at=now,approved_by=robot",
        ],
        "consent-store: invalid approved_by",
    );
    assert_usage_contains(
        &["consent-store", "pending", "--set-status", ":approved"],
        "consent-store: --set-status missing id",
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn plugin_manifest_0308_invoke_source_and_runtime_edges_are_stable() {
    let root = temp_dir("invoke");
    let ts_plugin = root.join("ts-plugin");
    let broken_wasm = root.join("broken-wasm");
    create_dir_all(&ts_plugin).expect("ts plugin dir");
    create_dir_all(&broken_wasm).expect("broken wasm dir");
    write(
        ts_plugin.join("plugin.json"),
        r#"{"name":"ts-plugin","version":"1.0.0","sdk":"*","entry":"index.ts"}"#,
    )
    .expect("ts manifest");
    write(ts_plugin.join("index.ts"), "export default () => null;").expect("ts entry");
    create_dir_all(broken_wasm.join("plugin.wasm")).expect("wasm path dir");
    write(
        broken_wasm.join("plugin.json"),
        r#"{"name":"broken-wasm","version":"1.0.0","sdk":"*","wasm":"plugin.wasm"}"#,
    )
    .expect("broken manifest");

    let root_arg = root.to_string_lossy();
    assert_usage_contains(
        &["plugin-manifest", "invoke", "--scan-dir", root_arg.as_ref()],
        "plugin-manifest invoke: --plugin is required",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "ts-plugin",
            "--source",
            "socket",
        ],
        "unknown --source socket",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
        ],
        "plugin-manifest: missing --plugin value",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "ts-plugin",
            "--arg",
        ],
        "plugin-manifest: missing --arg value",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "ts-plugin",
            "--source",
            "api",
        ],
        "TS source plugin 'ts-plugin' is not executable",
    );
    assert_usage_contains(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "ts-plugin",
            "--source",
            "peer",
            "--arg",
            "one",
            "--plan-json",
        ],
        "No Bun/JS subprocess fallback is available",
    );
    assert_ok_contains(
        &[
            "plugin-manifest",
            "invoke",
            "--scan-dir",
            root_arg.as_ref(),
            "--plugin",
            "broken-wasm",
        ],
        "failed to read wasm:",
    );

    assert_usage_contains(
        &["bind-host", "--config-named-peers-len", "many"],
        "bind-host: --config-named-peers-len must be a non-negative integer",
    );
    assert_usage_contains(
        &["bind-host", "--peers-store-len", "many"],
        "bind-host: --peers-store-len must be a non-negative integer",
    );

    remove_dir_all(root).expect("cleanup");
}
