//! Integrity + determinism pin-check for the `fleet-plugins/` artifact home (#72).
//!
//! `fleet-plugins/<name>/` ships a compiled `plugin.wasm` pinned by `artifact.sha256`
//! in its ship manifest. This harness is the CI equivalent of the `examples/wasm-parity`
//! `check:fixtures` golden check: it re-hashes every committed artifact and asserts it
//! still matches its pin, on the default toolchain-free `cargo test` path so it gates
//! narrow runs and CI alike.
//!
//! `fleet_plugins_rebuild_is_deterministic` is `#[ignore]` and gated on the
//! `AssemblyScript` toolchain — same pattern as
//! `plugin_hostfn_probe_acceptance::probe_builds_via_pipeline` — because it shells out to
//! `maw plugin build`. It proves an artifact is reproducible from its `plugin.source.json`
//! source form where one is committed.

use maw_cli::run_cli;
use maw_plugin_manifest::hash_file;
use serde_json::Value;
use std::fs::{self, create_dir_all, read_to_string};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

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

/// The `target` + `artifact` fields of a plugin manifest, as far as the pin check cares.
struct ShipManifest {
    name: String,
    target: Option<String>,
    artifact_path: Option<String>,
    artifact_sha256: Option<String>,
}

fn read_manifest(plugin_dir: &Path) -> Option<ShipManifest> {
    let raw = read_to_string(plugin_dir.join("plugin.json")).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    let name = value.get("name")?.as_str()?.to_owned();
    let artifact = value.get("artifact");
    Some(ShipManifest {
        name,
        target: value
            .get("target")
            .and_then(Value::as_str)
            .map(str::to_owned),
        artifact_path: artifact
            .and_then(|artifact| artifact.get("path"))
            .and_then(Value::as_str)
            .map(str::to_owned),
        artifact_sha256: artifact
            .and_then(|artifact| artifact.get("sha256"))
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// `Ok(true)` = a wasm ship artifact was verified against its pin; `Ok(false)` = skipped
/// (no manifest, or dev-tier source with nothing pinned); `Err` = a wasm ship whose pin
/// is missing, whose artifact is absent, or whose bytes drifted from the pin.
fn verify_ship_artifact(plugin_dir: &Path) -> Result<bool, String> {
    let Some(manifest) = read_manifest(plugin_dir) else {
        return Ok(false);
    };
    let is_wasm_ship = manifest.target.as_deref() == Some("wasm");
    if !is_wasm_ship && manifest.artifact_sha256.is_none() {
        // Dev-tier source alongside is allowed; nothing is pinned, so nothing to check.
        return Ok(false);
    }
    let expected = manifest.artifact_sha256.ok_or_else(|| {
        format!(
            "fleet plugin '{}' declares target=wasm but has no artifact.sha256 — run `maw plugin build`",
            manifest.name
        )
    })?;
    let relative = manifest.artifact_path.ok_or_else(|| {
        format!(
            "fleet plugin '{}' has artifact.sha256 but no artifact.path",
            manifest.name
        )
    })?;
    let artifact = plugin_dir.join(&relative);
    if !artifact.is_file() {
        return Err(format!(
            "fleet plugin '{}' artifact missing: {relative}",
            manifest.name
        ));
    }
    let observed = hash_file(&artifact)?;
    if observed == expected {
        Ok(true)
    } else {
        Err(format!(
            "fleet plugin '{}' artifact hash mismatch:\n  expected: {expected}\n  actual:   {observed}",
            manifest.name
        ))
    }
}

/// The committed `hostfn-probe` fixture is a real ship-form plugin (plugin.json
/// target=wasm + artifact.sha256 + plugin.wasm). Using it here proves the checker works
/// independently of whether any fleet plugin has landed yet — keeping this PR standalone.
fn known_good_ship_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/hostfn-probe")
}

#[test]
fn pin_check_verifies_known_good_ship_fixture() {
    match verify_ship_artifact(&known_good_ship_fixture()) {
        Ok(true) => {}
        other => panic!("known-good ship fixture should verify, got {other:?}"),
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

    let result = verify_ship_artifact(&staged);
    fs::remove_dir_all(&staged).ok();

    assert!(
        matches!(&result, Err(message) if message.contains("hash mismatch")),
        "tampered artifact should fail the pin check, got {result:?}"
    );
}

#[test]
fn fleet_plugins_artifacts_match_manifest_sha256() {
    // The real gate: every committed fleet-plugins/<name> ship artifact must match its
    // pin. Runs on the default toolchain-free path. Before any plugin lands (only
    // README present) this verifies zero artifacts and passes, so the crate PR stands
    // alone; it starts covering squad et al. the moment their dirs land.
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
        match verify_ship_artifact(&path) {
            Ok(true) => checked += 1,
            Ok(false) => {}
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
#[ignore = "requires the AssemblyScript toolchain: run `npm ci` in packages/wasm-sdk first"]
fn fleet_plugins_rebuild_is_deterministic() {
    // Reproduce each fleet artifact from its committed source form and assert the rebuilt
    // bytes reproduce the pin. Gated exactly like probe_builds_via_pipeline. A no-op until
    // a fleet plugin ships a plugin.source.json alongside its artifact.
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
        let Some(manifest) = read_manifest(&plugin) else {
            continue;
        };
        let Some(expected) = manifest.artifact_sha256 else {
            continue;
        };

        let staged = temp_dir("rebuild");
        let work = staged.join(&manifest.name);
        copy_tree(&plugin, &work);
        // Start from the source form so the pipeline actually compiles the .ts.
        let source = read_to_string(work.join("plugin.source.json")).expect("source manifest");
        fs::write(work.join("plugin.json"), source).expect("reset to source manifest");
        fs::remove_file(work.join("plugin.source.json")).ok();
        fs::remove_file(work.join("plugin.wasm")).ok();

        let build = run_cli(&args(&["plugin", "build", &work.display().to_string()]));
        assert_eq!(
            build.code, 0,
            "plugin build failed for '{}': {}\n{}",
            manifest.name, build.stderr, build.stdout
        );

        let observed = hash_file(&work.join("plugin.wasm")).expect("hash rebuilt wasm");
        assert_eq!(
            observed, expected,
            "fleet plugin '{}' is not reproducible from source",
            manifest.name
        );
        fs::remove_dir_all(&staged).ok();
        rebuilt += 1;
    }
    eprintln!("fleet-plugins rebuild determinism: {rebuilt} artifact(s) reproduced");
}
