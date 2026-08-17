// The plugin_plan suite, kept whole.
//
// Moved out of plugin_plan.rs so the module reads as the thing it implements rather than
// the thing plus its scaffolding. Not split further: these tests share one
// fixture harness, and a private item in one `mod` is invisible to a sibling,
// so carving them up would mean exporting the harness for no real gain.

#[cfg(test)]
mod plugin_install_tests {
    use super::{
        classify_plugin_install_source, verify_package_dir, verify_plugin_install_pin,
        InstallSource, ResolvedPackage,
    };
    fn temp_existing_dir(label: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "maw-rs-plugin-install-classifier-{label}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn classifier_accepts_explicit_git_url_forms() {
        assert_eq!(
            classify_plugin_install_source("https://github.com/owner/repo", None, None).expect("https"),
            InstallSource::Git {
                url: "https://github.com/owner/repo".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: false,
                subpath: None,
            }
        );
        assert_eq!(
            classify_plugin_install_source(
                "git@github.com:owner/repo.git",
                Some("main".to_owned()),
                None,
            )
            .expect("ssh"),
            InstallSource::Git {
                url: "git@github.com:owner/repo.git".to_owned(),
                reference: Some("main".to_owned()),
                sha256: None,
                warn_unpinned: false,
                subpath: None,
            }
        );
        assert_eq!(
            classify_plugin_install_source("file:///tmp/plugin-fixture", None, None).expect("file"),
            InstallSource::Git {
                url: "file:///tmp/plugin-fixture".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: false,
                subpath: None,
            }
        );
        assert_eq!(
            classify_plugin_install_source("owner/repo.git", None, None).expect("suffix"),
            InstallSource::Git {
                url: "owner/repo.git".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: false,
                subpath: None,
            }
        );
    }

    #[test]
    fn classifier_maps_owner_repo_shorthand_to_github_when_not_local() {
        assert_eq!(
            classify_plugin_install_source("Soul-Brews-Studio/maw-js", Some("alpha".to_owned()), None)
                .expect("shorthand"),
            InstallSource::Git {
                url: "https://github.com/Soul-Brews-Studio/maw-js".to_owned(),
                reference: Some("alpha".to_owned()),
                sha256: None,
                warn_unpinned: false,
                subpath: None,
            }
        );
        assert_eq!(
            classify_plugin_install_source("Soul-Brews-Studio/maw-js@v1", None, None)
                .expect("inline ref"),
            InstallSource::Git {
                url: "https://github.com/Soul-Brews-Studio/maw-js".to_owned(),
                reference: Some("v1".to_owned()),
                sha256: None,
                warn_unpinned: false,
                subpath: None,
            }
        );
        assert_eq!(
            classify_plugin_install_source("Soul-Brews-Studio/maw-plugins/packages/costs", None, None)
                .expect("monorepo shorthand"),
            InstallSource::Git {
                url: "https://github.com/Soul-Brews-Studio/maw-plugins".to_owned(),
                reference: None,
                sha256: None,
                warn_unpinned: true,
                subpath: Some(std::path::PathBuf::from("packages/costs")),
            }
        );
    }

    #[test]
    fn classifier_keeps_local_paths_local() {
        assert_eq!(
            classify_plugin_install_source("local-plugin", None, None).expect("plain local"),
            InstallSource::Local { dir: std::path::PathBuf::from("local-plugin"), sha256: None }
        );

        let dir = temp_existing_dir("existing");
        assert_eq!(
            classify_plugin_install_source(&dir.display().to_string(), None, None).expect("existing"),
            InstallSource::Local { dir: dir.clone(), sha256: None }
        );

        let pathish = "./missing-plugin";
        assert_eq!(
            classify_plugin_install_source(pathish, None, None).expect("pathish"),
            InstallSource::Local { dir: std::path::PathBuf::from(pathish), sha256: None }
        );

        let pin = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            classify_plugin_install_source("./missing-plugin", None, Some(pin.to_owned()))
                .expect("local with sha256"),
            InstallSource::Local {
                dir: std::path::PathBuf::from("./missing-plugin"),
                sha256: Some(pin.to_owned()),
            }
        );

        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn classifier_rejects_ref_for_local_source() {
        let error = classify_plugin_install_source("local-plugin", Some("main".to_owned()), None)
            .expect_err("local ref rejected");
        assert!(error.contains("--ref is only supported for git sources"));
    }

    #[test]
    fn pin_verifier_matches_mismatches_warns_and_checks_lock() {
        let path = temp_existing_dir("lock").join("plugins.lock");
        let _guard = crate::test_env::EnvVarGuard::set("MAW_PLUGINS_LOCK", &path);
        let sha = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let matched = verify_plugin_install_pin("demo", "0.1.0", Some(sha), Some(sha), true, false).expect("match");
        assert_eq!(matched.warning, None);
        assert_eq!(matched.resolved_sha256.as_deref(), Some(sha));
        let err = verify_plugin_install_pin("demo", "0.1.0", Some(sha), Some("sha256:1111111111111111111111111111111111111111111111111111111111111111"), false, false).expect_err("mismatch");
        assert!(err.contains("sha256 mismatch"), "{err}");
        assert!(verify_plugin_install_pin("demo", "0.1.0", Some(sha), None, true, false).expect("warn").warning.expect("warning").contains("unpinned"));
        let locked = "sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        std::fs::write(&path, format!(r#"{{"schema":1,"plugins":{{"demo":{{"version":"0.1.0","sha256":"{locked}","source":"github:o/r@v1"}}}}}}"#)).expect("lock");
        assert_eq!(verify_plugin_install_pin("demo", "0.1.0", Some(locked), None, true, false).expect("lock match").warning, None);
        let err = verify_plugin_install_pin("demo", "0.1.0", Some(sha), None, false, false).expect_err("lock mismatch");
        assert!(err.contains("plugins.lock"), "{err}");
        // --force replaces a mismatching lock pin instead of refusing.
        let forced = verify_plugin_install_pin("demo", "0.2.0", Some(sha), None, false, true).expect("forced");
        assert!(forced.warning.expect("force warning").contains("pin replaced"));
        // explicit --sha256 contradiction stays fatal even with --force.
        let err = verify_plugin_install_pin("demo", "0.1.0", Some(locked), Some(sha), false, true).expect_err("explicit pin mismatch");
        assert!(err.contains("sha256 mismatch"), "{err}");
    }

    #[test]
    fn lock_writer_creates_file_preserves_entries_and_lists_names() {
        let path = temp_existing_dir("lock-write").join("nested").join("plugins.lock");
        let _guard = crate::test_env::EnvVarGuard::set("MAW_PLUGINS_LOCK", &path);
        let summary = maw_plugin_manifest::PluginInstallSummary {
            name: "demo".to_owned(),
            version: "0.1.0".to_owned(),
            source_dir: std::path::PathBuf::from("/tmp/demo-src"),
            install_dir: std::path::PathBuf::from("/tmp/demo-dst"),
            copied_files: Vec::new(),
        };
        let sha = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        // no resolved sha → no file created
        super::record_plugin_install_pin(&summary, None, "path:/tmp/demo-src").expect("noop");
        assert!(!path.exists());

        super::record_plugin_install_pin(&summary, Some(sha), "path:/tmp/demo-src").expect("create");
        let entry = super::read_plugin_lock_entry_full("demo").expect("read").expect("entry");
        assert_eq!(entry.version, "0.1.0");
        assert_eq!(entry.sha256, sha);
        assert_eq!(entry.source.as_deref(), Some("path:/tmp/demo-src"));

        // second entry preserves the first
        let other = maw_plugin_manifest::PluginInstallSummary {
            name: "other".to_owned(),
            version: "1.0.0".to_owned(),
            source_dir: std::path::PathBuf::from("/tmp/other-src"),
            install_dir: std::path::PathBuf::from("/tmp/other-dst"),
            copied_files: Vec::new(),
        };
        super::record_plugin_install_pin(&other, Some(sha), "github:o/r@v1").expect("append");
        let mut names = super::plugin_lock_pinned_names();
        names.sort();
        assert_eq!(names, vec!["demo".to_owned(), "other".to_owned()]);
        assert!(super::read_plugin_lock_entry_full("demo").expect("read").is_some());
    }

    fn write_wasm_package_manifest(
        dir: &std::path::Path,
        name: &str,
        artifact_path: &str,
        sha256: &str,
    ) {
        std::fs::write(
            dir.join("plugin.json"),
            format!(
                r#"{{"name":"{name}","version":"1.0.0","target":"wasm","sdk":"*","entry":{{"kind":"wasm","path":"plugin.wasm","export":"handle"}},"wasm":"./plugin.wasm","artifact":{{"path":"{artifact_path}","sha256":"{sha256}"}},"cli":{{"command":"{name}"}}}}"#
            ),
        )
        .expect("wasm manifest");
    }

    fn write_wasm_package_fixture(dir: &std::path::Path, name: &str) -> String {
        std::fs::create_dir_all(dir).expect("package dir");
        std::fs::write(dir.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00verify-fixture")
            .expect("wasm artifact");
        let sha256 = maw_plugin_manifest::hash_file(&dir.join("plugin.wasm")).expect("hash wasm");
        write_wasm_package_manifest(dir, name, "plugin.wasm", &sha256);
        sha256
    }

    #[test]
    fn verify_package_dir_accepts_matching_pin_and_resolves_sha() {
        let root = temp_existing_dir("verify-pin-match");
        let _guard =
            crate::test_env::EnvVarGuard::set("MAW_PLUGINS_LOCK", root.join("plugins.lock"));
        let package = root.join("pkg");
        let sha256 = write_wasm_package_fixture(&package, "verify-match-demo");

        let ResolvedPackage::Wasm(verification) =
            verify_package_dir(&package, None, false, false).expect("pin match")
        else {
            panic!("expected wasm verification");
        };
        assert_eq!(verification.resolved_sha256.as_deref(), Some(sha256.as_str()));
        assert_eq!(verification.warning, None);

        // Explicit --sha256 equal to the manifest pin still verifies.
        let ResolvedPackage::Wasm(verification) =
            verify_package_dir(&package, Some(&sha256), false, false).expect("explicit pin match")
        else {
            panic!("expected wasm verification");
        };
        assert_eq!(verification.resolved_sha256.as_deref(), Some(sha256.as_str()));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_refuses_manifest_pin_mismatch() {
        let root = temp_existing_dir("verify-tamper");
        let package = root.join("pkg");
        write_wasm_package_fixture(&package, "verify-tamper-demo");
        std::fs::write(package.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00TAMPERED")
            .expect("tamper");
        let error = verify_package_dir(&package, None, false, false).expect_err("refused");
        assert!(error.contains("artifact sha256 mismatch — refusing to install"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_refuses_wasm_missing_pin() {
        let root = temp_existing_dir("verify-missing-pin");
        let package = root.join("pkg");
        std::fs::create_dir_all(&package).expect("package dir");
        std::fs::write(package.join("plugin.wasm"), b"\0asm\x01\x00\x00\x00unpinned")
            .expect("wasm artifact");
        std::fs::write(
            package.join("plugin.json"),
            r#"{"name":"verify-unpinned-demo","version":"1.0.0","target":"wasm","sdk":"*","artifact":{"path":"plugin.wasm"},"cli":{"command":"verify-unpinned-demo"}}"#,
        )
        .expect("manifest");
        let error = verify_package_dir(&package, None, false, false).expect_err("refused");
        assert!(error.contains("has no artifact.sha256"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_guards_artifact_path_traversal() {
        let root = temp_existing_dir("verify-traversal");
        let package = root.join("pkg");
        std::fs::create_dir_all(&package).expect("package dir");
        std::fs::write(root.join("outside.wasm"), b"\0asm\x01\x00\x00\x00outside")
            .expect("outside wasm");
        let sha256 = maw_plugin_manifest::hash_file(&root.join("outside.wasm")).expect("hash");
        write_wasm_package_manifest(&package, "verify-traversal-demo", "../outside.wasm", &sha256);
        let error = verify_package_dir(&package, None, false, false).expect_err("refused");
        assert!(error.contains("must stay inside the package"), "{error}");

        // Absolute artifact.path is refused by the same guard.
        let absolute = root.join("outside.wasm").display().to_string();
        write_wasm_package_manifest(&package, "verify-traversal-demo", &absolute, &sha256);
        let error = verify_package_dir(&package, None, false, false).expect_err("refused absolute");
        assert!(error.contains("must stay inside the package"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_explicit_sha256_mismatch_fatal_even_with_force() {
        let root = temp_existing_dir("verify-force-proof");
        let _guard =
            crate::test_env::EnvVarGuard::set("MAW_PLUGINS_LOCK", root.join("plugins.lock"));
        let package = root.join("pkg");
        write_wasm_package_fixture(&package, "verify-force-proof-demo");
        let wrong = "sha256:1111111111111111111111111111111111111111111111111111111111111111";
        let error = verify_package_dir(&package, Some(wrong), false, true).expect_err("refused");
        assert!(error.contains("sha256 mismatch — refusing to install"), "{error}");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_warn_unpinned_semantics() {
        let root = temp_existing_dir("verify-warn-unpinned");
        let _guard =
            crate::test_env::EnvVarGuard::set("MAW_PLUGINS_LOCK", root.join("plugins.lock"));
        let package = root.join("pkg");
        let sha256 = write_wasm_package_fixture(&package, "verify-warn-demo");

        // warn_unpinned=true, no lock entry, no --sha256 → unpinned warning.
        let ResolvedPackage::Wasm(verification) =
            verify_package_dir(&package, None, true, false).expect("warn")
        else {
            panic!("expected wasm verification");
        };
        assert!(verification.warning.expect("warning").contains("unpinned"));

        // warn_unpinned=true with an explicit --sha256 pin → no warning.
        let ResolvedPackage::Wasm(verification) =
            verify_package_dir(&package, Some(&sha256), true, false).expect("pinned")
        else {
            panic!("expected wasm verification");
        };
        assert_eq!(verification.warning, None);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn verify_package_dir_passes_through_non_wasm_packages() {
        let root = temp_existing_dir("verify-not-wasm");
        let package = root.join("pkg");
        std::fs::create_dir_all(&package).expect("package dir");

        // No plugin.json at all → the caller owns the route.
        assert!(matches!(
            verify_package_dir(&package, None, false, false).expect("no manifest"),
            ResolvedPackage::NotWasm
        ));

        // target=js → the caller owns the route (JS build / local verify).
        std::fs::write(
            package.join("plugin.json"),
            r#"{"name":"verify-js-demo","version":"0.1.0","sdk":"*","target":"js","entry":"index.ts","cli":{"command":"verify-js-demo"}}"#,
        )
        .expect("manifest");
        assert!(matches!(
            verify_package_dir(&package, None, false, false).expect("js manifest"),
            ResolvedPackage::NotWasm
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn git_lock_sources_collapse_github_urls_to_shorthand() {
        assert_eq!(
            super::git_install_lock_source("https://github.com/o/r.git", None, Some("v1")),
            "github:o/r@v1"
        );
        assert_eq!(
            super::git_install_lock_source(
                "https://github.com/o/r",
                Some(std::path::Path::new("packages/x")),
                Some("v1"),
            ),
            "github:o/r@v1/packages/x"
        );
        assert_eq!(
            super::git_install_lock_source(
                "https://example.com/repo.git",
                Some(std::path::Path::new("pkg")),
                None,
            ),
            "https://example.com/repo.git#pkg"
        );
    }
}
