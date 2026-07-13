use maw_plugin_manifest::{
    build_js_plugin_dir, infer_plugin_capabilities, init_js_plugin_dir, install_built_plugin_dir,
};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn infer_capabilities_matches_bun_ffi_and_ast_like_maw_patterns() {
    let source = r#"
        import { dlopen } from 'bun:ffi';
        import fs from 'node:fs/promises';
        const cp = require("node:child_process");
        const m = maw;
        const { identity, wake: wakeAlias } = maw;
        maw.fetch();
        m["send"]();
        fetch("https://example.test");
    "#;

    assert_eq!(
        infer_plugin_capabilities(source),
        vec![
            "ffi:any",
            "fs:read",
            "net:fetch",
            "proc:spawn",
            "sdk:fetch",
            "sdk:identity",
            "sdk:send",
            "sdk:wake",
        ]
    );
}

#[test]
fn build_js_plugin_dir_writes_dist_manifest_with_inferred_caps_and_dts() {
    let root = temp_dir("build");
    let plugin = root.join("ffi-demo");
    std::fs::create_dir_all(plugin.join("src")).expect("create plugin src");
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"name":"ffi-demo","version":"1.2.3","target":"js","sdk":"^1.0.0","entry":"./src/index.ts","capabilities":["sdk:fetch"]}"#,
    )
    .expect("write manifest");
    std::fs::write(
        plugin.join("src/index.ts"),
        "import { dlopen } from 'bun:ffi';\nmaw.identity();\nfetch('https://example.test');\n",
    )
    .expect("write source");

    let summary = build_js_plugin_dir(&plugin, true).expect("build plugin");

    assert_eq!(summary.name, "ffi-demo");
    assert_eq!(
        summary.capabilities,
        vec!["ffi:any", "net:fetch", "sdk:identity"]
    );
    assert_eq!(
        summary.inferred_only,
        vec!["ffi:any", "net:fetch", "sdk:identity"]
    );
    assert_eq!(summary.declared_only, vec!["sdk:fetch"]);
    assert!(summary.sha256.starts_with("sha256:"));
    assert!(summary.bundle_path.ends_with("dist/index.js"));
    assert!(summary.dts_path.as_ref().is_some_and(|path| path.exists()));

    let dist_manifest: Value = serde_json::from_str(
        &std::fs::read_to_string(plugin.join("dist/plugin.json")).expect("read dist manifest"),
    )
    .expect("valid dist manifest");
    assert_eq!(dist_manifest["entry"], "./index.js");
    assert_eq!(dist_manifest["artifact"]["path"], "./index.js");
    assert_eq!(dist_manifest["artifact"]["sha256"], summary.sha256);
    assert_eq!(
        dist_manifest["capabilities"],
        serde_json::json!(["ffi:any", "net:fetch", "sdk:identity"])
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn build_and_install_preserve_checksum_pinned_bundled_artifacts() {
    let root = temp_dir("bundled-artifact");
    let plugin = root.join("native-plugin");
    let install_root = root.join("installed");
    std::fs::create_dir_all(plugin.join("src")).expect("plugin src");
    std::fs::create_dir_all(plugin.join("bin")).expect("plugin bin");
    std::fs::write(
        plugin.join("plugin.json"),
        r#"{"name":"native-plugin","version":"1.0.0","target":"js","sdk":"^1.0.0","entry":"./src/index.ts","bundledArtifacts":[{"path":"bin/helper","sha256":"sha256:c2d728ea3e369c2e2c93b86cbb1e6cbd61240492bb609b0eefaecec9a15c044c"}]}"#,
    ).expect("manifest");
    std::fs::write(plugin.join("src/index.ts"), "export default {};\n").expect("entry");
    std::fs::write(plugin.join("bin/helper"), b"native helper").expect("helper");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            plugin.join("bin/helper"),
            std::fs::Permissions::from_mode(0o755),
        )
        .expect("helper permissions");
    }

    build_js_plugin_dir(&plugin, false).expect("build plugin");
    assert_eq!(
        std::fs::read(plugin.join("dist/bin/helper")).expect("dist helper"),
        b"native helper"
    );
    let install = install_built_plugin_dir(&plugin, &install_root).expect("install plugin");
    assert_eq!(
        std::fs::read(install.install_dir.join("bin/helper")).expect("installed helper"),
        b"native helper"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(install.install_dir.join("bin/helper"))
            .expect("installed helper metadata")
            .permissions()
            .mode();
        assert_ne!(mode & 0o111, 0, "installed helper must remain executable");
    }
    assert!(install
        .copied_files
        .contains(&std::path::PathBuf::from("bin/helper")));

    std::fs::write(plugin.join("bin/helper"), b"tampered").expect("tamper helper");
    assert!(build_js_plugin_dir(&plugin, false)
        .expect_err("checksum mismatch")
        .contains("sha256 mismatch"));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn init_and_install_plugin_are_host_authoritative_filesystem_operations() {
    let root = temp_dir("init-install");
    let source = root.join("src-plugin");
    let install_root = root.join("installed");
    let init = init_js_plugin_dir("my_ffi", &source).expect("init plugin");
    assert_eq!(init.name, "my-ffi");
    let entry = std::fs::read_to_string(&init.entry_path).expect("entry");
    assert!(entry.contains("export async function handler"), "{entry}");
    assert!(entry.contains("if (import.meta.main)"), "{entry}");
    assert!(entry.contains("process.argv.slice(2)"), "{entry}");
    assert!(entry.contains("hello from my-ffi"), "{entry}");

    build_js_plugin_dir(&source, false).expect("build plugin");
    let install = install_built_plugin_dir(&source, &install_root).expect("install plugin");

    assert_eq!(install.name, "my-ffi");
    assert!(install.install_dir.join("plugin.json").exists());
    assert!(install.install_dir.join("index.js").exists());
    assert!(install
        .copied_files
        .contains(&std::path::PathBuf::from("plugin.json")));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn maw_js_reference_absence_does_not_affect_committed_tier1_logic() {
    std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
    assert_eq!(
        infer_plugin_capabilities("import 'bun:ffi';"),
        vec!["ffi:any"]
    );
    std::env::remove_var("MAW_JS_REF_DIR");
}

fn temp_dir(label: &str) -> std::path::PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("maw-ffi-tier1-{label}-{stamp}"))
}
