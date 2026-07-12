use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output},
    time::{SystemTime, UNIX_EPOCH},
};

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-artifact-manager-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn install_plugin(root: &Path) -> PathBuf {
    let plugin = root.join("plugins/artifact-manager");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(
        plugin.join("plugin.json"),
        include_str!("fixtures/native-artifact-manager/artifact-manager-plugin/plugin.json"),
    )
    .expect("plugin json");
    fs::write(
        plugin.join("plugin.wasm"),
        include_bytes!("fixtures/native-artifact-manager/artifact-manager-plugin/plugin.wasm"),
    )
    .expect("plugin wasm");
    root.join("plugins")
}

fn run(root: &Path, plugins: &Path, args: &[&str]) -> Output {
    Command::new(bin())
        .args(args)
        .current_dir(root)
        .env("HOME", root.join("home"))
        .env("MAW_HOME", root.join("maw-home"))
        .env("MAW_PLUGINS_DIR", plugins)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .output()
        .expect("run maw-rs")
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stderr, b"");
    String::from_utf8(output.stdout).expect("utf8 stdout")
}

#[test]
fn artifact_manager_plugin_fallthrough_preserves_native_lifecycle_output() {
    let root = temp_dir("lifecycle");
    fs::create_dir_all(root.join("home")).expect("home");
    let plugins = install_plugin(&root);

    assert_eq!(
        stdout(run(&root, &plugins, &["artifact-manager", "ls"])),
        "No artifacts.\n"
    );

    let dir = root.join("maw-home/artifacts/team-b/t9");
    assert_eq!(
        stdout(run(
            &root,
            &plugins,
            &["art", "init", "team-b", "t9", "Subject", "Long", "desc"],
        )),
        format!("\x1b[32m✓\x1b[0m artifact created → {}\n", dir.display())
    );
    assert_eq!(
        stdout(run(
            &root,
            &plugins,
            &["art", "write", "team-b", "t9", "final", "answer"],
        )),
        format!(
            "\x1b[32m✓\x1b[0m result written → {}/result.md\n",
            dir.display()
        )
    );

    let source = root.join("source file.bin");
    fs::write(&source, [0_u8, 1, 2, 255]).expect("source");
    assert_eq!(
        stdout(run(
            &root,
            &plugins,
            &[
                "art",
                "attach",
                "team-b",
                "t9",
                source.to_str().expect("utf8")
            ],
        )),
        format!(
            "\x1b[32m✓\x1b[0m attached → {}/attachments/source_file.bin\n",
            dir.display()
        )
    );
    assert_eq!(
        fs::read(dir.join("attachments/source_file.bin")).expect("attachment"),
        [0_u8, 1, 2, 255]
    );

    let listed = stdout(run(&root, &plugins, &["art", "list", "team-b", "--json"]));
    let parsed: serde_json::Value = serde_json::from_str(&listed).expect("list json");
    let created_at = parsed[0]["createdAt"].as_str().expect("createdAt");
    assert!(created_at.ends_with('Z'));
    let listed = listed.replace(created_at, "2026-06-24T00:00:00.000Z");
    assert_eq!(
        listed,
        "[\n  {\n    \"team\": \"team-b\",\n    \"taskId\": \"t9\",\n    \"subject\": \"Subject\",\n    \"status\": \"completed\",\n    \"files\": 5,\n    \"hasResult\": true,\n    \"createdAt\": \"2026-06-24T00:00:00.000Z\"\n  }\n]\n"
    );

    let shown = stdout(run(&root, &plugins, &["art", "show", "team-b", "t9"]));
    assert_eq!(
        shown,
        format!(
            "\x1b[1mSubject\x1b[0m\nteam-b / t9 · completed · unowned\n\n\x1b[36m─── spec ───\x1b[0m\n# Subject\n\nLong desc\n\n\x1b[32m─── result ───\x1b[0m\nfinal answer\n\x1b[33m─── attachments (1) ───\x1b[0m\n  📎 source_file.bin\n\n\x1b[90m{}\x1b[0m\n",
            dir.display()
        )
    );
}

#[test]
fn native_artifact_manager_registrations_are_removed() {
    for command in ["artifact-manager", "art"] {
        assert_eq!(
            maw_cli::dispatcher_status(command),
            maw_cli::DispatchKind::NativeError,
            "{command}"
        );
    }
}
