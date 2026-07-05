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

