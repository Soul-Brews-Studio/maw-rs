use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn maw_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "maw-rs-plugin-install-{label}-{}-{nonce}-{count}",
        std::process::id()
    ));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn assert_success(output: &Output, context: &str) {
    assert!(
        output.status.success(),
        "{context}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("run git");
    assert_success(&output, &format!("git {}", args.join(" ")));
}

fn write_js_plugin(dir: &Path, name: &str) {
    fs::create_dir_all(dir.join("src")).expect("src dir");
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "target": "js",
  "sdk": "*",
  "entry": "src/index.ts",
  "cli": {{ "command": "{name}" }}
}}
"#
        ),
    )
    .expect("manifest");
    fs::write(
        dir.join("src/index.ts"),
        "export default async function main() { return { ok: true }; }\n",
    )
    .expect("entry");
}

fn write_built_js_plugin(dir: &Path, name: &str) {
    write_js_plugin(dir, name);
    fs::create_dir_all(dir.join("dist")).expect("dist dir");
    fs::write(
        dir.join("dist/plugin.json"),
        format!(
            r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "target": "js",
  "sdk": "*",
  "entry": "./index.js",
  "artifact": {{ "path": "./index.js", "sha256": "sha256:test" }},
  "cli": {{ "command": "{name}" }}
}}
"#
        ),
    )
    .expect("dist manifest");
    fs::write(
        dir.join("dist/index.js"),
        "export default async function main() { return { ok: true }; }\n",
    )
    .expect("dist entry");
}

fn write_serve_only_plugin(dir: &Path, name: &str) {
    fs::create_dir_all(dir).expect("plugin dir");
    fs::write(
        dir.join("plugin.json"),
        format!(
            r#"{{
  "name": "{name}",
  "version": "0.1.0",
  "sdk": "*",
  "cli": {{ "command": "{name}" }},
  "engine": {{
    "serve": {{
      "command": "bun run server-demo.ts",
      "prefix": "/api/{name}"
    }}
  }}
}}
"#
        ),
    )
    .expect("manifest");
}

fn fixture_repo(root: &Path, name: &str) -> PathBuf {
    let repo = root.join("repo");
    fs::create_dir_all(&repo).expect("repo dir");
    write_js_plugin(&repo, name);

    commit_fixture_repo(&repo);
    repo
}

fn monorepo_fixture(root: &Path, name: &str) -> (PathBuf, PathBuf) {
    let repo = root.join("repo");
    let package = repo.join("packages").join(name);
    write_js_plugin(&package, name);
    fs::write(repo.join("README.md"), "# fixture monorepo\n").expect("readme");
    commit_fixture_repo(&repo);
    (repo, package)
}

fn commit_fixture_repo(repo: &Path) {
    let init = Command::new("git")
        .arg("init")
        .arg("-q")
        .arg(repo)
        .output()
        .expect("git init");
    assert_success(&init, "git init");
    git(repo, &["add", "."]);
    let commit = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=maw-rs test",
            "-c",
            "user.email=maw-rs-test@example.invalid",
            "commit",
            "-q",
            "-m",
            "fixture",
        ])
        .output()
        .expect("git commit");
    assert_success(&commit, "git commit");
}

fn with_host_plugin_env<'a>(command: &'a mut Command, root: &Path) -> &'a mut Command {
    command
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env_remove("MAW_PLUGINS_DIR")
        .env_remove("MAW_DATA_DIR")
        .env_remove("MAW_CONFIG_DIR")
        .env_remove("MAW_STATE_DIR")
        .env_remove("MAW_CACHE_DIR")
}

#[test]
fn plugin_install_git_root_manifest_repo_regression() {
    let root = temp_dir("git-file");
    let repo = fixture_repo(&root, "git-fixture");
    let maw_home = root.join("maw-home");
    let default_plugin_root = maw_home.join("plugins");
    let file_url = format!(
        "file://{}",
        repo.canonicalize().expect("repo path").display()
    );

    let output = with_host_plugin_env(
        Command::new(maw_bin()).args(["plugin", "install", &file_url]),
        &root,
    )
    .output()
    .expect("maw plugin install");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let install_dir = default_plugin_root.join("git-fixture");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("installed git-fixture@0.1.0 {}\n", install_dir.display())
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(install_dir.join("plugin.json").is_file());
    assert!(install_dir.join("index.js").is_file());
    assert!(!install_dir.join("src/index.ts").exists());

    let no_bun_bin = root.join("no-bun-bin");
    fs::create_dir_all(&no_bun_bin).expect("no bun bin");
    let verb_path = std::env::join_paths([no_bun_bin]).expect("verb path");
    let verb = with_host_plugin_env(Command::new(maw_bin()).arg("git-fixture"), &root)
        .env("PATH", verb_path)
        .output()
        .expect("maw git-fixture");
    assert_eq!(verb.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&verb.stdout).is_empty());
    assert!(
        String::from_utf8_lossy(&verb.stderr)
            .contains("TS/JS plugin requires prebuilt WASM artifact"),
        "stderr: {}",
        String::from_utf8_lossy(&verb.stderr)
    );

    let _ = fs::remove_dir_all(root);
}

#[test]
fn plugin_install_git_existing_plugin_requires_force_to_reinstall() {
    let root = temp_dir("git-reinstall");
    let repo = fixture_repo(&root, "reinstall-fixture");
    let install_root = root.join("plugins");
    let file_url = format!(
        "file://{}",
        repo.canonicalize().expect("repo path").display()
    );
    let run = |force: bool| {
        let mut command = Command::new(maw_bin());
        command.args([
            "plugin",
            "install",
            &file_url,
            "--root",
            install_root.to_str().expect("install root utf8"),
        ]);
        if force {
            command.arg("--force");
        }
        with_host_plugin_env(&mut command, &root)
            .output()
            .expect("maw plugin reinstall")
    };

    assert_success(&run(false), "initial install");
    let install_dir = install_root.join("reinstall-fixture");
    fs::write(install_dir.join("stale.txt"), "stale\n").expect("stale file");

    let refused = run(false);
    assert_eq!(refused.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&refused.stdout).is_empty());
    assert_eq!(
        String::from_utf8_lossy(&refused.stderr),
        "plugin 'reinstall-fixture' is already installed; use --force to reinstall\n"
    );

    let forced = run(true);
    assert_success(&forced, "forced reinstall");
    assert!(String::from_utf8_lossy(&forced.stderr).is_empty());
    assert!(install_dir.join("plugin.json").is_file());
    assert!(!install_dir.join("stale.txt").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn plugin_install_git_subpath_builds_selected_package() {
    let root = temp_dir("git-subpath");
    let (repo, _) = monorepo_fixture(&root, "subpath-fixture");
    let install_root = root.join("plugins");
    let file_url = format!(
        "file://{}",
        repo.canonicalize().expect("repo path").display()
    );

    let output = with_host_plugin_env(
        Command::new(maw_bin()).args([
            "plugin",
            "install",
            &file_url,
            "--path",
            "packages/subpath-fixture",
            "--root",
            install_root.to_str().expect("install root utf8"),
        ]),
        &root,
    )
    .output()
    .expect("maw plugin install subpath");

    assert_success(&output, "plugin install subpath");
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    assert!(install_root.join("subpath-fixture/plugin.json").is_file());
    assert!(install_root.join("subpath-fixture/index.js").is_file());
    assert!(!install_root.join("README.md").exists());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn plugin_install_git_sha256_pin_combines_with_subpath() {
    let root = temp_dir("git-subpath-sha");
    let (repo, _) = monorepo_fixture(&root, "pinned-subpath");
    let file_url = format!(
        "file://{}",
        repo.canonicalize().expect("repo path").display()
    );
    let probe_root = root.join("probe-plugins");
    let probe = with_host_plugin_env(
        Command::new(maw_bin()).args([
            "plugin",
            "install",
            &file_url,
            "--path",
            "packages/pinned-subpath",
            "--root",
            probe_root.to_str().expect("probe root utf8"),
        ]),
        &root,
    )
    .output()
    .expect("probe install");
    assert_success(&probe, "probe install");
    let installed_manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(probe_root.join("pinned-subpath/plugin.json")).expect("installed manifest"),
    )
    .expect("manifest json");
    let sha256 = installed_manifest["artifact"]["sha256"]
        .as_str()
        .expect("artifact sha256");

    let install_root = root.join("plugins");
    let pinned = with_host_plugin_env(
        Command::new(maw_bin()).args([
            "plugin",
            "install",
            &file_url,
            "--path",
            "packages/pinned-subpath",
            "--sha256",
            sha256,
            "--root",
            install_root.to_str().expect("install root utf8"),
        ]),
        &root,
    )
    .output()
    .expect("pinned subpath install");

    assert_success(&pinned, "pinned subpath install");
    assert!(String::from_utf8_lossy(&pinned.stderr).is_empty());
    assert!(install_root.join("pinned-subpath/plugin.json").is_file());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn plugin_install_local_dir_with_explicit_root_still_works() {
    let root = temp_dir("local-root");
    let source = root.join("source");
    let install_root = root.join("plugins");
    write_built_js_plugin(&source, "local-fixture");

    let output = Command::new(maw_bin())
        .args([
            "plugin",
            "install",
            source.to_str().expect("source utf8"),
            "--root",
            install_root.to_str().expect("install root utf8"),
        ])
        .output()
        .expect("maw plugin install local");

    assert_eq!(
        output.status.code(),
        Some(0),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stderr).is_empty());
    let install_dir = install_root.join("local-fixture");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        format!("installed local-fixture@0.1.0 {}\n", install_dir.display())
    );
    assert!(install_dir.join("plugin.json").is_file());
    assert!(install_dir.join("index.js").is_file());

    let _ = fs::remove_dir_all(root);
}

#[test]
fn plugin_install_then_bare_invoke_serve_only_prints_mounted_url() {
    let root = temp_dir("serve-only");
    let source = root.join("source");
    write_serve_only_plugin(&source, "agora-fixture");

    let install = with_host_plugin_env(
        Command::new(maw_bin()).args(["plugin", "install", source.to_str().expect("source utf8")]),
        &root,
    )
    .output()
    .expect("maw plugin install");
    assert_success(&install, "install serve-only plugin");

    let invoke = with_host_plugin_env(Command::new(maw_bin()).arg("agora-fixture"), &root)
        .env("MAW_PORT", "4567")
        .output()
        .expect("maw agora-fixture");

    assert_success(&invoke, "invoke serve-only plugin");
    assert_eq!(
        String::from_utf8_lossy(&invoke.stdout),
        "http://localhost:4567/api/agora-fixture/\n"
    );
    assert!(String::from_utf8_lossy(&invoke.stderr).is_empty());

    let _ = fs::remove_dir_all(root);
}
