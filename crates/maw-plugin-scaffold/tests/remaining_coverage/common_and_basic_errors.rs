use maw_plugin_scaffold::{copy_tree, scaffold_as, scaffold_rust, validate_plugin_name};
use std::{
    fs,
    time::{SystemTime, UNIX_EPOCH},
};

fn temp_dir(name: &str) -> std::path::PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("maw-plugin-scaffold-{name}-{nonce}"))
}

fn running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

#[test]
fn scaffold_rust_writes_manifest_and_readme() {
    let root = temp_dir("rust");
    let template = root.join("template");
    let dest = root.join("dest");
    fs::create_dir_all(&template).expect("create template");
    fs::write(
        template.join("Cargo.toml"),
        r#"[package]
name = "template"
[dependencies]
maw-plugin-sdk = { path = "old" }
"#,
    )
    .expect("write cargo template");

    scaffold_rust("hello-world", &dest, &template, "../sdk").expect("scaffold rust");

    assert!(fs::read_to_string(dest.join("plugin.json"))
        .expect("read manifest")
        .contains("hello-world"));
    assert!(fs::read_to_string(dest.join("README.md"))
        .expect("read readme")
        .contains("hello-world"));
    assert!(fs::read_to_string(dest.join("Cargo.toml"))
        .expect("read cargo")
        .contains(r#"maw-plugin-sdk = { path = "../sdk" }"#));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffold_as_writes_manifest_and_rewrites_package() {
    let root = temp_dir("as");
    let template = root.join("template");
    let dest = root.join("dest");
    fs::create_dir_all(&template).expect("create template");
    fs::write(template.join("package.json"), r#"{"name":"template"}"#)
        .expect("write package template");

    scaffold_as("hello_as", &dest, &template).expect("scaffold as");

    assert!(fs::read_to_string(dest.join("plugin.json"))
        .expect("read manifest")
        .contains("hello-as"));
    assert!(fs::read_to_string(dest.join("README.md"))
        .expect("read readme")
        .contains("hello_as"));
    assert!(fs::read_to_string(dest.join("package.json"))
        .expect("read package")
        .contains("hello_as"));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn empty_plugin_name_fails_validator() {
    assert_eq!(
        validate_plugin_name(""),
        Some("name is required".to_owned())
    );
}

#[test]
fn scaffold_rust_reports_plugin_json_write_error() {
    let root = temp_dir("rust-plugin-json-dir");
    let template = root.join("template");
    let dest = root.join("dest");
    fs::create_dir_all(template.join("plugin.json")).expect("create plugin dir");
    fs::write(
        template.join("Cargo.toml"),
        r#"[package]
name = "template"
[dependencies]
maw-plugin-sdk = { path = "old" }
"#,
    )
    .expect("write cargo template");

    let error = scaffold_rust("hello-world", &dest, &template, "../sdk")
        .expect_err("plugin.json directory should reject manifest write");
    assert!(
        error.to_string().contains("Is a directory")
            || error.kind() == std::io::ErrorKind::PermissionDenied
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffold_as_reports_plugin_json_write_error() {
    let root = temp_dir("as-plugin-json-dir");
    let template = root.join("template");
    let dest = root.join("dest");
    fs::create_dir_all(template.join("plugin.json")).expect("create plugin dir");

    let error = scaffold_as("hello-as", &dest, &template)
        .expect_err("plugin.json directory should reject manifest write");
    assert!(
        error.to_string().contains("Is a directory")
            || error.kind() == std::io::ErrorKind::PermissionDenied
    );
    let _ = fs::remove_dir_all(root);
}

#[test]
fn copy_tree_reports_create_dir_and_read_dir_errors() {
    let root = temp_dir("copy-errors");
    fs::create_dir_all(&root).expect("create root");
    let src_file = root.join("src-file");
    let dest_file = root.join("dest-file");
    fs::write(&src_file, "not a dir").expect("write src file");
    fs::write(&dest_file, "not a dir").expect("write dest file");

    let create_error = copy_tree(&src_file, dest_file.join("child"))
        .expect_err("create_dir_all below file should fail");
    assert!(matches!(
        create_error.kind(),
        std::io::ErrorKind::NotADirectory | std::io::ErrorKind::AlreadyExists
    ));

    let read_error = copy_tree(&src_file, root.join("dest-dir"))
        .expect_err("reading a file as a directory should fail");
    assert!(matches!(
        read_error.kind(),
        std::io::ErrorKind::NotADirectory | std::io::ErrorKind::InvalidInput
    ));
    let _ = fs::remove_dir_all(root);
}

#[test]
fn scaffold_reports_cargo_package_read_and_package_write_errors() {
    let root = temp_dir("midstream-errors");
    let rust_template = root.join("rust-template");
    let rust_dest = root.join("rust-dest");
    fs::create_dir_all(rust_template.join("Cargo.toml")).expect("cargo dir");
    let rust_error = scaffold_rust("hello-world", &rust_dest, &rust_template, "../sdk")
        .expect_err("Cargo.toml directory should reject read");
    assert!(
        rust_error.to_string().contains("Is a directory")
            || rust_error.kind() == std::io::ErrorKind::PermissionDenied
    );

    let as_template = root.join("as-template");
    let as_dest = root.join("as-dest");
    fs::create_dir_all(&as_template).expect("as template");
    fs::write(as_template.join("package.json"), r#"{"name":"template"}"#)
        .expect("package template");
    fs::create_dir_all(as_dest.join("package.json")).expect("dest package dir");
    let as_error = scaffold_as("hello-as", &as_dest, &as_template)
        .expect_err("package.json directory should reject rewrite");
    assert!(
        as_error.to_string().contains("Is a directory")
            || as_error.kind() == std::io::ErrorKind::PermissionDenied
    );
    let _ = fs::remove_dir_all(root);
}

