#[test]
fn scaffold_rust_reports_copy_and_write_failures() {
    let root = temp_dir("rust-copy-write-failures");
    let template = root.join("template");
    fs::create_dir_all(&template).expect("create template");
    fs::write(
        template.join("Cargo.toml"),
        "name = \"template\"\nmaw-plugin-sdk = { path = \"old\" }\n",
    )
    .expect("write cargo template");

    let blocking_file = root.join("blocking-file");
    fs::write(&blocking_file, "not a directory").expect("write blocking file");
    let copy_error = scaffold_rust(
        "copy-fail",
        blocking_file.join("plugin"),
        &template,
        "../sdk",
    )
    .expect_err("copy_tree failure should propagate from scaffold_rust");
    assert!(matches!(
        copy_error.kind(),
        std::io::ErrorKind::NotADirectory | std::io::ErrorKind::AlreadyExists
    ));

    if running_as_root() {
        eprintln!(
            "skip readonly Cargo.toml rewrite assertion: root bypasses OS readonly permissions"
        );
    } else {
        let readonly_dest = root.join("readonly-dest");
        let cargo_path = template.join("Cargo.toml");
        let original_permissions = fs::metadata(&cargo_path)
            .expect("cargo metadata")
            .permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&cargo_path, readonly_permissions).expect("make cargo readonly");
        let write_error = scaffold_rust("write-fail", &readonly_dest, &template, "../sdk")
            .expect_err("readonly Cargo.toml should reject rewrite");
        assert!(matches!(
            write_error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
        ));
        let _ = fs::set_permissions(
            readonly_dest.join("Cargo.toml"),
            original_permissions.clone(),
        );
        let _ = fs::set_permissions(&cargo_path, original_permissions);
    }

    let readme_template = root.join("readme-template");
    let readme_dest = root.join("readme-dest");
    fs::create_dir_all(readme_template.join("README.md")).expect("readme dir");
    fs::write(
        readme_template.join("Cargo.toml"),
        "name = \"template\"\nmaw-plugin-sdk = { path = \"old\" }\n",
    )
    .expect("write cargo template");
    let readme_error = scaffold_rust("readme-fail", &readme_dest, &readme_template, "../sdk")
        .expect_err("README.md directory should reject readme write");
    assert!(
        readme_error.to_string().contains("Is a directory")
            || readme_error.kind() == std::io::ErrorKind::PermissionDenied
    );

    let _ = fs::remove_dir_all(root);
}

