use maw_plugin_scaffold::{
    build_manifest_json, cmd_plugin_create, copy_tree, scaffold_as, scaffold_rust,
    validate_plugin_name, PluginCreateError, PluginCreateRequest, PluginLanguage,
};
use serde_json::Value;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

#[test]
fn plugin_create_error_display_matches_command_messages() {
    let dest = PathBuf::from("plugins/existing");
    assert!(PluginCreateError::MissingType
        .to_string()
        .contains("Specify either --rust or --as"));
    assert_eq!(
        PluginCreateError::ConflictingTypes.to_string(),
        "  Specify --rust or --as, not both"
    );
    assert!(PluginCreateError::MissingName
        .to_string()
        .contains("maw plugin create"));
    assert_eq!(
        PluginCreateError::Scaffold("template exploded".to_owned()).to_string(),
        "✗ template exploded"
    );
    assert!(PluginCreateError::DestinationExists(dest)
        .to_string()
        .contains("plugins/existing"));
}

#[test]
fn validate_plugin_name_accepts_simple_lowercase_name() {
    assert_eq!(validate_plugin_name("hello"), None);
}

#[test]
fn validate_plugin_name_accepts_name_with_hyphens_and_digits() {
    assert_eq!(validate_plugin_name("my-plugin-2"), None);
}

#[test]
fn validate_plugin_name_accepts_name_with_underscores() {
    assert_eq!(validate_plugin_name("my_plugin"), None);
}

#[test]
fn validate_plugin_name_rejects_empty_string() {
    assert!(validate_plugin_name("").is_some());
}

#[test]
fn validate_plugin_name_rejects_name_starting_with_digit() {
    assert!(validate_plugin_name("2plugin").is_some());
}

#[test]
fn validate_plugin_name_rejects_name_with_uppercase_letters() {
    assert!(validate_plugin_name("MyPlugin").is_some());
}

#[test]
fn validate_plugin_name_rejects_name_with_spaces() {
    assert!(validate_plugin_name("my plugin").is_some());
}

#[test]
fn copy_tree_copies_files_preserving_structure() {
    let root = unique_temp_dir("copy-structure");
    let src = root.join("src");
    let dest = root.join("copy");
    fs::create_dir_all(src.join("sub")).expect("create source subdir");
    fs::write(src.join("a.txt"), "hello").expect("write source file");
    fs::write(src.join("sub").join("b.txt"), "world").expect("write nested source file");

    copy_tree(&src, &dest).expect("copy tree succeeds");

    assert_eq!(
        fs::read_to_string(dest.join("a.txt")).expect("read copied file"),
        "hello"
    );
    assert_eq!(
        fs::read_to_string(dest.join("sub").join("b.txt")).expect("read nested copied file"),
        "world"
    );

    fs::remove_dir_all(root).ok();
}

#[test]
fn copy_tree_skips_target_directory() {
    let root = unique_temp_dir("copy-skip-target");
    let src = root.join("src");
    let dest = root.join("copy");
    fs::create_dir_all(src.join("target")).expect("create target dir");
    fs::write(src.join("keep.txt"), "yes").expect("write kept file");
    fs::write(src.join("target").join("artifact.wasm"), "binary").expect("write skipped artifact");

    copy_tree(&src, &dest).expect("copy tree succeeds");

    assert!(dest.join("keep.txt").exists());
    assert!(!dest.join("target").exists());

    fs::remove_dir_all(root).ok();
}

#[test]
fn copy_tree_skips_git_and_node_modules_entries() {
    let root = unique_temp_dir("copy-skip-extra");
    let src = root.join("src");
    let dest = root.join("copy");
    fs::create_dir_all(src.join(".git")).expect("create git dir");
    fs::create_dir_all(src.join("node_modules")).expect("create node_modules dir");
    fs::write(src.join(".git").join("config"), "secret").expect("write git file");
    fs::write(src.join("node_modules").join("pkg.js"), "pkg").expect("write module file");

    copy_tree(&src, &dest).expect("copy tree succeeds");

    assert!(!dest.join(".git").exists());
    assert!(!dest.join("node_modules").exists());

    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_creates_destination_directory() {
    let root = unique_temp_dir("scaffold-rust-create");
    let template = root.join("template");
    make_rust_template(&template, "../../maw-plugin-sdk");
    let dest = root.join("my-plugin");

    scaffold_rust("my-plugin", &dest, &template, "/fake/sdk").expect("scaffold rust succeeds");

    assert!(dest.exists());
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_rewrites_cargo_package_name() {
    let root = unique_temp_dir("scaffold-rust-name");
    let template = root.join("template");
    make_rust_template(&template, "../../maw-plugin-sdk");
    let dest = root.join("my-plugin");

    scaffold_rust("my-plugin", &dest, &template, "/fake/sdk").expect("scaffold rust succeeds");

    let cargo = fs::read_to_string(dest.join("Cargo.toml")).expect("read scaffolded cargo");
    assert!(cargo.contains(r#"name = "my-plugin""#));
    assert!(!cargo.contains(r#"name = "hello-rust""#));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_replaces_relative_sdk_path_with_absolute_path() {
    let root = unique_temp_dir("scaffold-rust-sdk");
    let template = root.join("template");
    make_rust_template(&template, "../../maw-plugin-sdk");
    let dest = root.join("my-plugin");
    let sdk_abs = "/home/user/.bun/install/global/node_modules/maw/src/wasm/maw-plugin-sdk";

    scaffold_rust("my-plugin", &dest, &template, sdk_abs).expect("scaffold rust succeeds");

    let cargo = fs::read_to_string(dest.join("Cargo.toml")).expect("read scaffolded cargo");
    assert!(cargo.contains(&format!(r#"path = "{sdk_abs}""#)));
    assert!(!cargo.contains("../../maw-plugin-sdk"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_writes_readme_at_destination() {
    let root = unique_temp_dir("scaffold-rust-readme");
    let template = root.join("template");
    make_rust_template(&template, "../../maw-plugin-sdk");
    let dest = root.join("my-plugin");

    scaffold_rust("my-plugin", &dest, &template, "/fake/sdk").expect("scaffold rust succeeds");

    let readme = fs::read_to_string(dest.join("README.md")).expect("read scaffolded readme");
    assert!(readme.contains("my-plugin"));
    assert!(readme.contains("maw plugin install"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_copies_src_lib_rs_from_template() {
    let root = unique_temp_dir("scaffold-rust-lib");
    let template = root.join("template");
    make_rust_template(&template, "../../maw-plugin-sdk");
    let dest = root.join("my-plugin");

    scaffold_rust("my-plugin", &dest, &template, "/fake/sdk").expect("scaffold rust succeeds");

    assert!(dest.join("src").join("lib.rs").exists());
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_throws_if_template_directory_does_not_exist() {
    let root = unique_temp_dir("scaffold-rust-missing");
    let err = scaffold_rust(
        "my-plugin",
        root.join("my-plugin"),
        root.join("missing"),
        "/fake/sdk",
    )
    .expect_err("missing template should error");

    assert!(err.to_string().contains("Rust template not found"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_rust_writes_plugin_json_manifest_contract() {
    let root = unique_temp_dir("scaffold-rust-manifest");
    let template = root.join("template");
    make_rust_template(&template, "../../maw-plugin-sdk");
    let dest = root.join("my-rust-plugin");

    scaffold_rust("my-rust-plugin", &dest, &template, "/fake/sdk").expect("scaffold rust succeeds");

    let data: Value = serde_json::from_str(
        &fs::read_to_string(dest.join("plugin.json")).expect("read scaffolded manifest"),
    )
    .expect("valid manifest json");
    assert_eq!(data["name"], "my-rust-plugin");
    assert_eq!(data["version"], "0.1.0");
    assert_eq!(data["sdk"], "^1.0.0");
    assert_eq!(
        data["wasm"],
        "./target/wasm32-unknown-unknown/release/my_rust_plugin.wasm"
    );
    assert_eq!(data["cli"]["command"], "my-rust-plugin");
    assert_eq!(data["api"]["path"], "/api/plugins/my-rust-plugin");
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_creates_destination_directory() {
    let root = unique_temp_dir("scaffold-as-create");
    let template = root.join("template");
    make_as_template(&template);
    let dest = root.join("my-as-plugin");

    scaffold_as("my-as-plugin", &dest, &template).expect("scaffold as succeeds");

    assert!(dest.exists());
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_rewrites_package_json_name() {
    let root = unique_temp_dir("scaffold-as-name");
    let template = root.join("template");
    make_as_template(&template);
    let dest = root.join("my-as-plugin");

    scaffold_as("my-as-plugin", &dest, &template).expect("scaffold as succeeds");

    let package: Value =
        serde_json::from_str(&fs::read_to_string(dest.join("package.json")).expect("read package"))
            .expect("valid package json");
    assert_eq!(package["name"], "my-as-plugin");
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_writes_readme_at_destination() {
    let root = unique_temp_dir("scaffold-as-readme");
    let template = root.join("template");
    make_as_template(&template);
    let dest = root.join("my-as-plugin");

    scaffold_as("my-as-plugin", &dest, &template).expect("scaffold as succeeds");

    let readme = fs::read_to_string(dest.join("README.md")).expect("read scaffolded readme");
    assert!(readme.contains("my-as-plugin"));
    assert!(readme.contains("maw plugin install"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_allows_template_without_package_json() {
    let root = unique_temp_dir("scaffold-as-no-package");
    let template = root.join("template");
    fs::create_dir_all(template.join("assembly")).expect("create assembly dir");
    fs::write(
        template.join("assembly").join("index.ts"),
        "export function handle(): i32 { return 0; }\n",
    )
    .expect("write assembly source");
    let dest = root.join("my-as-plugin");

    scaffold_as("my-as-plugin", &dest, &template)
        .expect("scaffold as succeeds without package json");

    assert!(dest.join("plugin.json").exists());
    assert!(!dest.join("package.json").exists());
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_rejects_invalid_package_json_shapes() {
    let root = unique_temp_dir("scaffold-as-invalid-package");
    let template = root.join("template");
    make_as_template(&template);
    fs::write(template.join("package.json"), "not json").expect("write invalid json");

    let err = scaffold_as("my-as-plugin", root.join("bad-json"), &template)
        .expect_err("invalid package json should fail");
    assert!(err.to_string().contains("package.json: invalid JSON"));

    fs::write(template.join("package.json"), "[]").expect("write non-object json");
    let err = scaffold_as("my-as-plugin", root.join("non-object"), &template)
        .expect_err("non-object package json should fail");
    assert!(err
        .to_string()
        .contains("package.json: must be a JSON object"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_throws_if_template_directory_does_not_exist() {
    let root = unique_temp_dir("scaffold-as-missing");
    let err = scaffold_as(
        "my-as-plugin",
        root.join("my-as-plugin"),
        root.join("missing"),
    )
    .expect_err("missing template should error");

    assert!(err
        .to_string()
        .contains("AssemblyScript template not found"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn scaffold_as_writes_plugin_json_manifest_contract() {
    let root = unique_temp_dir("scaffold-as-manifest");
    let template = root.join("template");
    make_as_template(&template);
    let dest = root.join("my-as-plugin");

    scaffold_as("my-as-plugin", &dest, &template).expect("scaffold as succeeds");

    let data: Value = serde_json::from_str(
        &fs::read_to_string(dest.join("plugin.json")).expect("read scaffolded manifest"),
    )
    .expect("valid manifest json");
    assert_eq!(data["name"], "my-as-plugin");
    assert_eq!(data["version"], "0.1.0");
    assert_eq!(data["sdk"], "^1.0.0");
    assert_eq!(data["wasm"], "./build/release.wasm");
    assert_eq!(data["cli"]["command"], "my-as-plugin");
    assert_eq!(data["api"]["path"], "/api/plugins/my-as-plugin");
    fs::remove_dir_all(root).ok();
}

#[test]
fn cmd_plugin_create_rejects_existing_destination() {
    let root = unique_temp_dir("cmd-plugin-existing");
    let existing = root.join("existing");
    fs::create_dir_all(&existing).expect("create existing destination");
    let request = PluginCreateRequest {
        name: Some("my-plugin".to_owned()),
        rust: true,
        assembly_script: false,
        dest: existing.clone(),
    };

    let err = cmd_plugin_create(
        &request,
        root.join("rust-template"),
        root.join("as-template"),
        "/fake/sdk",
    )
    .expect_err("existing destination should fail");

    assert_eq!(err, PluginCreateError::DestinationExists(existing.clone()));
    assert!(err.to_string().contains("Destination already exists"));
    assert!(err.to_string().contains(&existing.display().to_string()));
    fs::remove_dir_all(root).ok();
}

#[test]
fn cmd_plugin_create_rejects_missing_or_conflicting_type_flags() {
    let root = unique_temp_dir("cmd-plugin-flags");
    let missing = PluginCreateRequest {
        name: Some("my-plugin".to_owned()),
        rust: false,
        assembly_script: false,
        dest: root.join("missing"),
    };
    assert_eq!(
        cmd_plugin_create(&missing, root.join("rust"), root.join("as"), "/fake/sdk")
            .expect_err("missing type should fail"),
        PluginCreateError::MissingType
    );

    let conflicting = PluginCreateRequest {
        name: Some("my-plugin".to_owned()),
        rust: true,
        assembly_script: true,
        dest: root.join("conflicting"),
    };
    assert_eq!(
        cmd_plugin_create(
            &conflicting,
            root.join("rust"),
            root.join("as"),
            "/fake/sdk"
        )
        .expect_err("conflicting type should fail"),
        PluginCreateError::ConflictingTypes
    );
    fs::remove_dir_all(root).ok();
}

#[test]
fn cmd_plugin_create_rejects_missing_or_invalid_name() {
    let root = unique_temp_dir("cmd-plugin-name");
    let missing = PluginCreateRequest {
        name: None,
        rust: true,
        assembly_script: false,
        dest: root.join("missing"),
    };
    assert_eq!(
        cmd_plugin_create(&missing, root.join("rust"), root.join("as"), "/fake/sdk")
            .expect_err("missing name should fail"),
        PluginCreateError::MissingName
    );

    let invalid = PluginCreateRequest {
        name: Some("Bad Name".to_owned()),
        rust: true,
        assembly_script: false,
        dest: root.join("invalid"),
    };
    let err = cmd_plugin_create(&invalid, root.join("rust"), root.join("as"), "/fake/sdk")
        .expect_err("invalid name should fail");
    assert!(matches!(err, PluginCreateError::InvalidName(_)));
    assert!(err.to_string().contains("Invalid plugin name"));
    fs::remove_dir_all(root).ok();
}

#[test]
fn cmd_plugin_create_dispatches_rust_and_assemblyscript_scaffolds() {
    let root = unique_temp_dir("cmd-plugin-dispatch");
    let rust_template = root.join("rust-template");
    let as_template = root.join("as-template");
    make_rust_template(&rust_template, "../../maw-plugin-sdk");
    make_as_template(&as_template);

    let rust_dest = root.join("rust-plugin");
    cmd_plugin_create(
        &PluginCreateRequest {
            name: Some("rust-plugin".to_owned()),
            rust: true,
            assembly_script: false,
            dest: rust_dest.clone(),
        },
        &rust_template,
        &as_template,
        "/fake/sdk",
    )
    .expect("rust dispatch succeeds");
    assert!(rust_dest.join("Cargo.toml").exists());

    let as_dest = root.join("as-plugin");
    cmd_plugin_create(
        &PluginCreateRequest {
            name: Some("as-plugin".to_owned()),
            rust: false,
            assembly_script: true,
            dest: as_dest.clone(),
        },
        &rust_template,
        &as_template,
        "/fake/sdk",
    )
    .expect("as dispatch succeeds");
    assert!(as_dest.join("package.json").exists());
    fs::remove_dir_all(root).ok();
}

#[test]
fn cmd_plugin_create_wraps_scaffold_errors() {
    let root = unique_temp_dir("cmd-plugin-scaffold-error");
    let err = cmd_plugin_create(
        &PluginCreateRequest {
            name: Some("my-plugin".to_owned()),
            rust: true,
            assembly_script: false,
            dest: root.join("my-plugin"),
        },
        root.join("missing-rust-template"),
        root.join("missing-as-template"),
        "/fake/sdk",
    )
    .expect_err("missing template should be wrapped");

    assert!(matches!(err, PluginCreateError::Scaffold(_)));
    assert!(err.to_string().contains("Rust template not found"));
    fs::remove_dir_all(root).ok();
}

