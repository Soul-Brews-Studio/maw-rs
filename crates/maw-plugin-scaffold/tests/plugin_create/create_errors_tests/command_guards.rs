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

