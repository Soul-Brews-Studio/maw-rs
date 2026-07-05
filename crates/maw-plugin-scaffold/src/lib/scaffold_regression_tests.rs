    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("maw-plugin-scaffold-{name}-{nonce}"))
    }

    #[test]
    fn scaffold_rust_writes_manifest_readme_and_rewritten_cargo() {
        let template = temp_dir("rust-template");
        let dest = temp_dir("rust-dest");
        fs::create_dir_all(&template).expect("create template");
        fs::write(
            template.join("Cargo.toml"),
            "name = \"template\"\nmaw-plugin-sdk = { path = \"../old\" }\n",
        )
        .expect("write cargo");

        scaffold_rust("hello-plugin", &dest, &template, "../sdk").expect("scaffold rust");

        let cargo = fs::read_to_string(dest.join("Cargo.toml")).expect("read cargo");
        assert!(cargo.contains("name = \"hello-plugin\""));
        assert!(cargo.contains("maw-plugin-sdk = { path = \"../sdk\" }"));
        let manifest = fs::read_to_string(dest.join("plugin.json")).expect("read manifest");
        assert!(manifest.contains("\"name\": \"hello-plugin\""));
        assert!(fs::read_to_string(dest.join("README.md"))
            .expect("read readme")
            .contains("hello-plugin"));
        let _ = fs::remove_dir_all(template);
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn scaffold_as_rewrites_package_and_writes_manifest() {
        let template = temp_dir("as-template");
        let dest = temp_dir("as-dest");
        fs::create_dir_all(&template).expect("create template");
        fs::write(template.join("package.json"), r#"{"name":"template"}"#).expect("write package");

        scaffold_as("hello_as", &dest, &template).expect("scaffold as");

        assert!(fs::read_to_string(dest.join("package.json"))
            .expect("read package")
            .contains("\"name\": \"hello_as\""));
        assert!(fs::read_to_string(dest.join("plugin.json"))
            .expect("read manifest")
            .contains("\"name\": \"hello-as\""));
        let _ = fs::remove_dir_all(template);
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn scaffold_edges_cover_package_without_manifest_and_name_start() {
        let template = temp_dir("as-template-no-package");
        let dest = temp_dir("as-dest-no-package");
        fs::create_dir_all(&template).expect("create template");

        scaffold_as("edge_plugin", &dest, &template).expect("scaffold as without package");

        assert!(dest.join("plugin.json").exists());
        assert!(!dest.join("package.json").exists());
        assert_eq!(
            validate_plugin_name("1bad"),
            Some(
                "\"1bad\" is invalid — use lowercase letters, digits, - or _ (must start with a letter)"
                    .to_owned()
            )
        );
        let _ = fs::remove_dir_all(template);
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn invalid_empty_plugin_name_is_rejected() {
        assert_eq!(
            validate_plugin_name("").as_deref(),
            Some("name is required")
        );
        assert!(validate_plugin_name("1bad").is_some());
        assert!(!is_valid_plugin_name(""));
    }

    #[test]
    fn private_rewriters_cover_no_newline_and_readme_shapes() {
        let cargo = "name = \"template\"\n[dependencies]\nmaw-plugin-sdk = { path = \"old\" }";
        let rewritten = rewrite_rust_cargo_toml(cargo, "hello-rust", "../sdk");

        assert!(!rewritten.ends_with('\n'));
        assert!(rewritten.contains("name = \"hello-rust\""));
        assert!(rewritten.contains("maw-plugin-sdk = { path = \"../sdk\" }"));
        assert!(
            rust_readme("hello-rust", Path::new("/tmp/plugin"), "../sdk").contains("maw::send")
        );
        assert!(as_readme("hello-as", Path::new("/tmp/as-plugin")).contains("npm run build"));
    }

    #[test]
    fn copy_tree_recurses_and_skips_artifact_directories() {
        let template = temp_dir("copy-tree-template");
        let dest = temp_dir("copy-tree-dest");
        fs::create_dir_all(template.join("src/nested")).expect("create nested");
        fs::create_dir_all(template.join("target")).expect("create target");
        fs::create_dir_all(template.join(".git")).expect("create git");
        fs::create_dir_all(template.join("node_modules")).expect("create modules");
        fs::write(template.join("src/nested/lib.rs"), "pub fn ok() {}\n").expect("write nested");
        fs::write(template.join("target/skip"), "skip").expect("write target");
        fs::write(template.join(".git/skip"), "skip").expect("write git");
        fs::write(template.join("node_modules/skip"), "skip").expect("write modules");

        copy_tree(&template, &dest).expect("copy template tree");

        assert!(dest.join("src/nested/lib.rs").exists());
        assert!(!dest.join("target").exists());
        assert!(!dest.join(".git").exists());
        assert!(!dest.join("node_modules").exists());
        let _ = fs::remove_dir_all(template);
        let _ = fs::remove_dir_all(dest);
    }

    #[test]
    fn scaffold_reports_midstream_template_shape_errors() {
        let rust_template = temp_dir("rust-template-missing-cargo");
        let rust_dest = temp_dir("rust-dest-missing-cargo");
        fs::create_dir_all(&rust_template).expect("create rust template");
        let error = scaffold_rust("hello-rust", &rust_dest, &rust_template, "../sdk")
            .expect_err("missing Cargo.toml should surface read error");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);

        let as_template = temp_dir("as-template-package-dir");
        let as_dest = temp_dir("as-dest-package-dir");
        fs::create_dir_all(as_template.join("package.json")).expect("create package dir");
        let error = scaffold_as("hello-as", &as_dest, &as_template)
            .expect_err("package.json directory should surface read error");
        assert!(error.to_string().contains("Is a directory"));

        let _ = fs::remove_dir_all(rust_template);
        let _ = fs::remove_dir_all(rust_dest);
        let _ = fs::remove_dir_all(as_template);
        let _ = fs::remove_dir_all(as_dest);
    }

    #[test]
    fn copy_tree_private_entry_errors_are_covered() {
        let err = read_tree_entry(Err(io::ErrorKind::Other.into())).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Other);
        let denied: io::Error = io::ErrorKind::PermissionDenied.into();
        let err = tree_entry_from_parts("x".into(), "x".into(), Err(denied)).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
        let root = temp_dir("copy-entry-error");
        fs::create_dir_all(&root).expect("root");
        let err = copy_tree_entries([Err(io::ErrorKind::Interrupted.into())], &root).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::Interrupted);
        let _ = fs::remove_dir_all(root);
    }
