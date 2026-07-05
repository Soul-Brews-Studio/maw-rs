#[test]
fn scaffold_as_reports_package_and_readme_write_failures() {
    let root = temp_dir("as-write-failures");
    let template = root.join("template");
    fs::create_dir_all(&template).expect("create template");
    fs::write(template.join("package.json"), r#"{"name":"template"}"#)
        .expect("write package template");

    if running_as_root() {
        eprintln!(
            "skip readonly package.json rewrite assertion: root bypasses OS readonly permissions"
        );
    } else {
        let package_path = template.join("package.json");
        let original_permissions = fs::metadata(&package_path)
            .expect("package metadata")
            .permissions();
        let mut readonly_permissions = original_permissions.clone();
        readonly_permissions.set_readonly(true);
        fs::set_permissions(&package_path, readonly_permissions).expect("make package readonly");
        let package_dest = root.join("package-dest");
        let package_error = scaffold_as("package-fail", &package_dest, &template)
            .expect_err("readonly package.json should reject rewrite");
        assert!(matches!(
            package_error.kind(),
            std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem
        ));
        let _ = fs::set_permissions(
            package_dest.join("package.json"),
            original_permissions.clone(),
        );
        let _ = fs::set_permissions(&package_path, original_permissions);
    }

    let readme_template = root.join("readme-template");
    let readme_dest = root.join("readme-dest");
    fs::create_dir_all(readme_template.join("README.md")).expect("readme dir");
    let readme_error = scaffold_as("readme-fail", &readme_dest, &readme_template)
        .expect_err("README.md directory should reject AS readme write");
    assert!(
        readme_error.to_string().contains("Is a directory")
            || readme_error.kind() == std::io::ErrorKind::PermissionDenied
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn copy_tree_reports_recursive_destination_conflict() {
    let root = temp_dir("copy-recursive-conflict");
    let src = root.join("src");
    let dest = root.join("dest");
    fs::create_dir_all(src.join("nested")).expect("nested source");
    fs::create_dir_all(&dest).expect("dest root");
    fs::write(dest.join("nested"), "not a directory").expect("blocking dest file");

    let error = copy_tree(&src, &dest).expect_err("nested dest file should reject recursive copy");
    assert!(matches!(
        error.kind(),
        std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::NotADirectory
    ));
    let _ = fs::remove_dir_all(root);
}

#[cfg(unix)]
#[test]
fn copy_tree_reports_unreadable_source_file() {
    use std::os::unix::fs::PermissionsExt;

    let root = temp_dir("copy-tree-unreadable-file");
    let src = root.join("src");
    let dest = root.join("dest");
    let file = src.join("secret.txt");
    fs::create_dir_all(&src).expect("src");
    fs::write(&file, "secret").expect("source file");
    let original = fs::metadata(&file).expect("metadata").permissions();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o000)).expect("chmod unreadable");

    let error = copy_tree(&src, &dest).expect_err("unreadable source should reject copy");

    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
    let _ = fs::set_permissions(&file, original);
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffold_as_reports_missing_template_path() {
    let root = temp_dir("as-missing-template");
    let error = scaffold_as(
        "missing-as",
        root.join("dest"),
        root.join("missing-template"),
    )
    .expect_err("missing AssemblyScript template should fail");
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(error
        .to_string()
        .contains("AssemblyScript template not found"));
}
