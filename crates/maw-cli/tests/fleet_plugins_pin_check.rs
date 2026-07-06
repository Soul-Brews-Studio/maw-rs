//! Integrity + determinism pin-check for the `fleet-plugins/` artifact home (#72).
//!
//! `fleet-plugins/<name>/` ships a compiled `plugin.wasm` pinned by an `artifact.sha256`
//! in one of its committed manifests. This harness is the CI equivalent of the
//! `examples/wasm-parity` `check:fixtures` golden check: it re-hashes every committed
//! artifact and asserts it still matches its pin, on the default toolchain-free
//! `cargo test` path so it gates narrow runs and CI alike, and it refuses an unpinned
//! `plugin.wasm` so no artifact ships unverified.
//!
//! Which manifest carries the pin depends on the plugin's active tier. A ship-tier
//! plugin pins its artifact in `plugin.json` (like the `hostfn-probe` fixture). A plugin
//! whose *active* tier is still `bun-dev` (e.g. squad, gated on a runtime fs-roots grant)
//! keeps `plugin.json` as its bun-dev manifest and pins the pre-staged wasm in
//! `plugin.source.json` instead — so the check reads both.
//!
//! `fleet_plugins_rebuild_is_deterministic` is `#[ignore]` and gated on the
//! `AssemblyScript` toolchain — same pattern as
//! `plugin_hostfn_probe_acceptance::probe_builds_via_pipeline` — because it shells out to
//! `maw plugin build`. It rebuilds from `plugin.source.json` and asserts the artifact
//! reproduces its committed pin.

use maw_cli::run_cli;
use maw_plugin_manifest::hash_file;
use serde_json::Value;
use std::ffi::OsString;
use std::fmt::Write as FmtWrite;
use std::fs::{self, create_dir_all, read_to_string};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex,
};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

type HermesFake = (String, Arc<Mutex<Vec<String>>>, Arc<AtomicBool>, thread::JoinHandle<()>);

/// Manifest filenames a fleet plugin may use to declare its ship artifact. `plugin.json`
/// is the active manifest (possibly a bun-dev dev-tier manifest with no artifact);
/// `plugin.source.json` is the wasm source manifest, and is where a still-bun-dev plugin
/// pins its pre-staged artifact.
const MANIFEST_CANDIDATES: [&str; 2] = ["plugin.json", "plugin.source.json"];

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("repo root two levels above crates/maw-cli")
        .to_path_buf()
}

fn fleet_plugins_dir() -> PathBuf {
    repo_root().join("fleet-plugins")
}

fn args(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-fleet-pin-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}

fn copy_tree(src: &Path, dst: &Path) {
    create_dir_all(dst).expect("dst dir");
    for entry in fs::read_dir(src).expect("read_dir") {
        let entry = entry.expect("entry");
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_tree(&from, &to);
        } else {
            fs::copy(&from, &to).expect("copy file");
        }
    }
}

fn seed_ctq_vault(root: &Path) {
    let atlas = root.join("vault/atlas/inbox");
    let neo = root.join("vault/neo/inbox");
    create_dir_all(&atlas).expect("atlas inbox");
    create_dir_all(&neo).expect("neo inbox");
    fs::write(atlas.join("001-handoff.md"), "---\nfrom: zai\nto: nat\nteam: atlas\ntype: handoff\nsubject: Review vault scanner\n---\nPlease review the vault scanner.\n").expect("atlas msg");
    fs::write(neo.join("002-question.md"), "---\nfrom: morpheus\nto: zai\nteam: neo\ntype: question\nsubject: Need queue answer\n---\nCan you confirm the queue filter?\n").expect("neo msg");
}

fn hermes_args(root: &Path, values: &[&str]) -> Vec<String> {
    let mut out = args(&[
        "plugin-manifest", "invoke", "--scan-dir", &root.display().to_string(), "--plugin", "hermes",
    ]);
    for value in values {
        out.push("--arg".to_owned());
        out.push((*value).to_owned());
    }
    out
}

fn set_hermes_base_url(plugin: &Path, base_url: &str) {
    let manifest = plugin.join("plugin.json");
    let mut value: Value = serde_json::from_str(&read_to_string(&manifest).expect("manifest json")).expect("json");
    value["endpoints"]["discord-rest"]["baseUrl"] = Value::String(base_url.to_owned());
    fs::write(&manifest, serde_json::to_string_pretty(&value).expect("serialize manifest")).expect("write manifest");
}

fn spawn_hermes_discord_fake() -> HermesFake {
    let listener = TcpListener::bind("127.0.0.1:0").expect("fake discord bind");
    listener.set_nonblocking(true).expect("nonblocking");
    let base = format!("http://{}", listener.local_addr().expect("local addr"));
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    let stop = Arc::new(AtomicBool::new(false));
    let stop_loop = Arc::clone(&stop);
    let handle = thread::spawn(move || {
        while !stop_loop.load(Ordering::SeqCst) {
            let Ok((mut stream, _)) = listener.accept() else {
                thread::sleep(std::time::Duration::from_millis(10));
                continue;
            };
            let mut buf = [0_u8; 8192];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]).to_string();
            let path = req.lines().next().and_then(|line| line.split_whitespace().nth(1)).unwrap_or("/");
            seen.lock().expect("seen").push(req.clone());
            let body = hermes_fake_body(path.split('?').next().unwrap_or(path));
            let resp = format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}", body.len(), body);
            stream.write_all(resp.as_bytes()).ok();
        }
    });
    (base, requests, stop, handle)
}

fn hermes_fake_body(path: &str) -> &'static str {
    match path {
        "/users/@me" => r#"{"username":"hermes-bot","id":"bot-42","bot":true}"#,
        "/users/@me/guilds" => r#"[{"name":"Nous","id":"guild1"}]"#,
        "/channels/ch1" => r#"{"id":"ch1","guild_id":"guild1"}"#,
        "/channels/ch1/messages" => r#"[{"id":"m2","author":{"username":"trinity","bot":true},"content":"ack"},{"id":"m1","author":{"username":"neo","bot":false},"content":"hello"}]"#,
        "/guilds/guild1/threads/active" => r#"{"threads":[{"id":"th1","name":"Alpha","parent_id":"ch1","message_count":2,"last_message_id":"9"}]}"#,
        "/channels/ch1/threads/archived/public" => r#"{"threads":[{"id":"th2","name":"Old","parent_id":"ch1","message_count":1,"last_message_id":"7"}]}"#,
        "/channels/th1/messages" => r#"[{"id":"tm1","author":{"username":"morpheus","bot":false},"content":"thread hi"}]"#,
        "/channels/th2/messages" => r#"[{"id":"tm0","author":{"username":"archive-bot","bot":true},"content":"old note"}]"#,
        _ => r#"{"error":"unexpected fake path"}"#,
    }
}

fn restore_env(key: &str, value: Option<OsString>) {
    match value {
        Some(value) => std::env::set_var(key, value),
        None => std::env::remove_var(key),
    }
}

fn normalize_root(root: &Path, output: &str) -> String {
    let raw = root.display().to_string();
    let mut normalized = output.replace(&format!("/private{raw}"), "$ROOT");
    normalized = normalized.replace(&raw, "$ROOT");
    if let Ok(canonical) = root.canonicalize() {
        normalized = normalized.replace(&canonical.display().to_string(), "$ROOT");
    }
    normalized
}

/// An `artifact` pin declared by one manifest: the relative artifact path + its sha256.
struct ArtifactPin {
    plugin_name: String,
    manifest: String,
    path: String,
    sha256: String,
}

/// Read the `artifact` pin (path + sha256) from a single manifest file, if it declares one.
fn read_artifact_pin(manifest_path: &Path) -> Option<ArtifactPin> {
    let raw = read_to_string(manifest_path).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let plugin_name = value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("?")
        .to_owned();
    let artifact = value.get("artifact")?;
    let path = artifact.get("path").and_then(Value::as_str)?.to_owned();
    let sha256 = artifact.get("sha256").and_then(Value::as_str)?.to_owned();
    Some(ArtifactPin {
        plugin_name,
        manifest: manifest_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path,
        sha256,
    })
}

/// Verify every committed artifact under `plugin_dir` against its manifest pin, and refuse
/// an unpinned `plugin.wasm`. Returns the number of artifacts verified (0 = a source-only
/// or manifest-less dir with no committed artifact — nothing pinned to check).
fn verify_pins(plugin_dir: &Path) -> Result<usize, String> {
    let mut verified_files = Vec::new();
    let mut checked = 0_usize;
    for candidate in MANIFEST_CANDIDATES {
        let Some(pin) = read_artifact_pin(&plugin_dir.join(candidate)) else {
            continue;
        };
        let artifact = plugin_dir.join(&pin.path);
        if !artifact.is_file() {
            return Err(format!(
                "fleet plugin '{}' ({}) pins artifact '{}' but the file is missing",
                pin.plugin_name, pin.manifest, pin.path
            ));
        }
        let observed = hash_file(&artifact)?;
        if observed != pin.sha256 {
            return Err(format!(
                "fleet plugin '{}' ({}) artifact hash mismatch:\n  expected: {}\n  actual:   {}",
                pin.plugin_name, pin.manifest, pin.sha256, observed
            ));
        }
        checked += 1;
        if let Ok(canonical) = artifact.canonicalize() {
            verified_files.push(canonical);
        }
    }
    // Coverage: a committed plugin.wasm must be pinned by one of the manifests above, so an
    // artifact can never ship unverified.
    let wasm = plugin_dir.join("plugin.wasm");
    if wasm.is_file() {
        let covered = wasm
            .canonicalize()
            .is_ok_and(|canonical| verified_files.contains(&canonical));
        if !covered {
            return Err(format!(
                "fleet plugin dir '{}' commits plugin.wasm but no manifest pins it — add \
                 \"target\":\"wasm\" + \"artifact\":{{\"path\":\"./plugin.wasm\",\"sha256\":\"sha256:<hex>\"}} \
                 to plugin.source.json (or plugin.json)",
                plugin_dir.display()
            ));
        }
    }
    Ok(checked)
}

/// The committed `hostfn-probe` fixture is a real ship-form plugin (its `plugin.json` has
/// `target=wasm`, an `artifact.sha256`, and a `plugin.wasm`). Using it here proves the
/// checker works independently of whether any fleet plugin has landed yet — keeping this
/// PR standalone.
fn known_good_ship_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostfn-probe")
}

#[test]
fn pin_check_verifies_known_good_ship_fixture() {
    match verify_pins(&known_good_ship_fixture()) {
        Ok(count) if count >= 1 => {}
        other => panic!("known-good ship fixture should verify >=1 artifact, got {other:?}"),
    }
}

#[test]
fn pin_check_detects_tampered_artifact() {
    let staged = temp_dir("tamper");
    copy_tree(&known_good_ship_fixture(), &staged);
    let wasm = staged.join("plugin.wasm");
    let mut bytes = fs::read(&wasm).expect("read wasm");
    let last = bytes.len() - 1;
    bytes[last] ^= 0xff;
    fs::write(&wasm, &bytes).expect("write tampered wasm");

    let result = verify_pins(&staged);
    fs::remove_dir_all(&staged).ok();

    assert!(
        matches!(&result, Err(message) if message.contains("hash mismatch")),
        "tampered artifact should fail the pin check, got {result:?}"
    );
}

#[test]
fn pin_check_refuses_unpinned_wasm() {
    // Squad's exact shape: a bun-dev plugin.json (no artifact) sitting next to a committed
    // plugin.wasm. Without a pin in some manifest the artifact would ship unverified, so
    // the check must refuse it.
    let staged = temp_dir("unpinned");
    fs::write(
        staged.join("plugin.json"),
        r#"{"name":"unpinned","version":"0.1.0","sdk":"*","runtime":"bun-dev","target":"js","entry":"impl.ts"}"#,
    )
    .expect("write bun-dev manifest");
    fs::write(staged.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00").expect("write wasm");

    let result = verify_pins(&staged);
    fs::remove_dir_all(&staged).ok();

    assert!(
        matches!(&result, Err(message) if message.contains("no manifest pins it")),
        "unpinned plugin.wasm should be refused, got {result:?}"
    );
}

#[test]
fn fleet_plugins_artifacts_match_manifest_sha256() {
    // The real gate: every committed fleet-plugins/<name> artifact must match its pin and
    // no plugin.wasm may ship unpinned. Runs on the default toolchain-free path. Before any
    // plugin lands (only README present) this verifies zero artifacts and passes, so the
    // crate PR stands alone; it starts covering squad et al. the moment their dirs land.
    let dir = fleet_plugins_dir();
    if !dir.is_dir() {
        return;
    }
    let mut checked = 0_usize;
    let mut failures = Vec::new();
    for entry in fs::read_dir(&dir).expect("read fleet-plugins") {
        let path = entry.expect("entry").path();
        if !path.is_dir() {
            continue;
        }
        match verify_pins(&path) {
            Ok(count) => checked += count,
            Err(message) => failures.push(message),
        }
    }
    assert!(
        failures.is_empty(),
        "fleet-plugins pin check failed:\n{}",
        failures.join("\n")
    );
    eprintln!("fleet-plugins pin check: {checked} artifact(s) verified");
}

#[test]
fn cross_team_queue_fleet_artifact_installs_and_scans_vault() {
    let source = fleet_plugins_dir().join("cross-team-queue");
    assert!(source.join("plugin.json").is_file(), "missing cross-team-queue fleet plugin");
    let root = temp_dir("ctq-invoke");
    seed_ctq_vault(&root);
    let install_root = root.join("plugins");
    let install = run_cli(&args(&[
        "plugin",
        "install",
        &source.display().to_string(),
        "--root",
        &install_root.display().to_string(),
    ]));
    assert_eq!(install.code, 0, "plugin install failed: {}\n{}", install.stderr, install.stdout);

    let saved_home = std::env::var_os("HOME");
    let saved_vault = std::env::var_os("MAW_VAULT_ROOT");
    std::env::set_var("HOME", &root);
    std::env::set_var("MAW_VAULT_ROOT", root.join("vault"));
    let invoke = run_cli(&args(&[
        "plugin-manifest",
        "invoke",
        "--scan-dir",
        &install_root.display().to_string(),
        "--plugin",
        "cross-team-queue",
        "--arg",
        "--json",
    ]));
    let filtered = run_cli(&args(&[
        "plugin-manifest", "invoke", "--scan-dir", &install_root.display().to_string(),
        "--plugin", "cross-team-queue", "--arg", "--json", "--arg", "--recipient", "--arg", "nat",
    ]));
    restore_env("MAW_VAULT_ROOT", saved_vault);
    restore_env("HOME", saved_home);
    fs::remove_dir_all(&root).ok();

    assert_eq!(invoke.code, 0, "plugin invoke failed: {}\n{}", invoke.stderr, invoke.stdout);
    assert_eq!(
        normalize_root(&root, &invoke.stdout),
        include_str!("fixtures/zerobun/cross-team-queue-scan.stdout")
    );
    assert!(invoke.stderr.is_empty(), "{}", invoke.stderr);
    assert_eq!(filtered.code, 0, "filtered invoke failed: {}\n{}", filtered.stderr, filtered.stdout);
    let filtered = normalize_root(&root, &filtered.stdout);
    assert!(filtered.contains("\"totalItems\":1") && filtered.contains("\"recipient\":\"nat\"") && !filtered.contains("\"recipient\":\"zai\""), "{filtered}");
}

#[test]
fn hermes_fleet_artifact_invokes_discord_read_only_verbs() {
    let source = fleet_plugins_dir().join("hermes");
    let manifest: Value = serde_json::from_str(&read_to_string(source.join("plugin.json")).expect("hermes manifest")).expect("json");
    assert_eq!(
        manifest["capabilities"],
        serde_json::json!(["net:fetch:discord-rest", "secret:use:discord-bot-token"])
    );
    let root = temp_dir("hermes-invoke");
    let staged = root.join("hermes");
    copy_tree(&source, &staged);
    let (base_url, requests, stop, server) = spawn_hermes_discord_fake();
    set_hermes_base_url(&staged, &base_url);
    let install_root = root.join("plugins");
    let install = run_cli(&args(&[
        "plugin", "install", &staged.display().to_string(), "--root", &install_root.display().to_string(),
    ]));
    assert_eq!(install.code, 0, "plugin install failed: {}\n{}", install.stderr, install.stdout);

    let saved_token = std::env::var_os("DISCORD_BOT_TOKEN");
    std::env::set_var("DISCORD_BOT_TOKEN", "TEST_TOKEN_245");
    let mut observed = String::new();
    for (label, argv) in [
        ("whoami", vec!["whoami"]),
        ("channels", vec!["channels"]),
        ("read", vec!["read", "ch1", "2"]),
        ("threads-list", vec!["threads", "list", "ch1", "--all"]),
        ("threads-read", vec!["threads", "read", "ch1", "--all"]),
    ] {
        let invoke = run_cli(&hermes_args(&install_root, &argv));
        assert_eq!(invoke.code, 0, "hermes {label} failed: {}\n{}", invoke.stderr, invoke.stdout);
        write!(&mut observed, "## {label}\n{}", invoke.stdout).expect("append observed");
        assert!(!invoke.stdout.contains("TEST_TOKEN_245"), "token leaked in stdout");
        assert!(invoke.stderr.is_empty(), "{}", invoke.stderr);
    }
    restore_env("DISCORD_BOT_TOKEN", saved_token);
    stop.store(true, Ordering::SeqCst);
    server.join().expect("fake server join");
    fs::remove_dir_all(&root).ok();

    assert_eq!(observed, include_str!("fixtures/zerobun/hermes-wasm-read-only.stdout"));
    let requests = requests.lock().expect("requests");
    assert!(requests.iter().all(|req| req.contains("authorization: Bot TEST_TOKEN_245")), "{requests:#?}");
}

#[test]
#[ignore = "requires the AssemblyScript toolchain: run `npm ci` in packages/wasm-sdk first"]
fn fleet_plugins_rebuild_is_deterministic() {
    // Reproduce each fleet artifact from its committed source form and assert the rebuilt
    // bytes reproduce the pin. Gated exactly like probe_builds_via_pipeline. A no-op until
    // a fleet plugin ships a plugin.source.json alongside a pinned artifact.
    let dir = fleet_plugins_dir();
    if !dir.is_dir() {
        return;
    }
    let mut rebuilt = 0_usize;
    for entry in fs::read_dir(&dir).expect("read fleet-plugins") {
        let plugin = entry.expect("entry").path();
        if !plugin.is_dir() || !plugin.join("plugin.source.json").is_file() {
            continue;
        }
        // The committed pin to reproduce, from whichever manifest declares it.
        let Some(expected) = MANIFEST_CANDIDATES
            .iter()
            .find_map(|candidate| read_artifact_pin(&plugin.join(candidate)))
            .map(|pin| pin.sha256)
        else {
            continue;
        };

        let staged = temp_dir("rebuild");
        let name = plugin.file_name().expect("plugin name").to_owned();
        let work = staged.join(&name);
        copy_tree(&plugin, &work);
        // Start from the source form so the pipeline actually compiles the .ts.
        let source = read_to_string(work.join("plugin.source.json")).expect("source manifest");
        fs::write(work.join("plugin.json"), source).expect("reset to source manifest");
        fs::remove_file(work.join("plugin.source.json")).ok();
        fs::remove_file(work.join("plugin.wasm")).ok();

        let build = run_cli(&args(&["plugin", "build", &work.display().to_string()]));
        assert_eq!(
            build.code,
            0,
            "plugin build failed for '{}': {}\n{}",
            name.to_string_lossy(),
            build.stderr,
            build.stdout
        );

        let observed = hash_file(&work.join("plugin.wasm")).expect("hash rebuilt wasm");
        assert_eq!(
            observed,
            expected,
            "fleet plugin '{}' is not reproducible from source",
            name.to_string_lossy()
        );
        fs::remove_dir_all(&staged).ok();
        rebuilt += 1;
    }
    eprintln!("fleet-plugins rebuild determinism: {rebuilt} artifact(s) reproduced");
}
