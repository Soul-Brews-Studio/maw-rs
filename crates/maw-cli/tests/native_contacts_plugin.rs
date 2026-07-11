use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use maw_cli::{dispatcher_status, DispatchKind};
use serde_json::Value;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_maw-rs"))
}

fn temp_dir(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("maw-rs-contacts-{name}-{stamp}"));
    fs::create_dir_all(&path).expect("temp dir");
    path
}

fn run(args: &[&str], cwd: &Path, maw_home: &Path, plugins_dir: &Path) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .current_dir(cwd)
        .env("HOME", maw_home)
        .env("MAW_HOME", maw_home)
        .env("MAW_JS_REF_DIR", "/nonexistent")
        .env("MAW_PLUGINS_DIR", plugins_dir)
        .output()
        .expect("run maw-rs")
}

fn install_contacts_plugin(root: &Path) -> PathBuf {
    let plugin = root.join("plugins/contacts");
    fs::create_dir_all(&plugin).expect("plugin dir");
    fs::write(
        plugin.join("plugin.json"),
        include_str!("fixtures/native-contacts/contacts-plugin/plugin.json"),
    )
    .expect("plugin json");
    fs::write(
        plugin.join("plugin.wasm"),
        include_bytes!("fixtures/native-contacts/contacts-plugin/plugin.wasm"),
    )
    .expect("plugin wasm");
    root.join("plugins")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn contacts_plugin_add_list_remove_matches_committed_golden_without_ref_checkout() {
    let root = temp_dir("golden");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    let plugins = install_contacts_plugin(&root);
    fs::create_dir_all(&cwd).expect("cwd");

    let empty = run(&["contacts"], &cwd, &maw_home, &plugins);
    assert_success(&empty);
    assert_eq!(
        String::from_utf8(empty.stdout).expect("stdout"),
        include_str!("fixtures/native-contacts/contacts-empty.stdout")
    );
    assert_eq!(String::from_utf8(empty.stderr).expect("stderr"), "");

    let add = run(
        &[
            "contacts",
            "add",
            "nova",
            "--maw",
            "bigboy-vps:nova",
            "--thread",
            "thread-79",
            "--inbox",
            "inbox/nova",
            "--repo",
            "/repo",
            "--notes",
            "native port",
        ],
        &cwd,
        &maw_home,
        &plugins,
    );
    assert_success(&add);
    assert_eq!(
        String::from_utf8(add.stdout).expect("stdout"),
        include_str!("fixtures/native-contacts/contacts-add.stdout")
    );
    assert_eq!(String::from_utf8(add.stderr).expect("stderr"), "");

    let contacts_path = cwd.join("psi/contacts.json");
    let saved: Value = serde_json::from_str(
        &fs::read_to_string(&contacts_path).expect("contacts file written under temp cwd psi"),
    )
    .expect("contacts json");
    assert_eq!(saved["contacts"]["nova"]["maw"], "bigboy-vps:nova");
    assert_eq!(saved["contacts"]["nova"]["thread"], "thread-79");
    assert_eq!(saved["contacts"]["nova"]["inbox"], "inbox/nova");
    assert_eq!(saved["contacts"]["nova"]["repo"], "/repo");
    assert_eq!(saved["contacts"]["nova"]["notes"], "native port");
    assert!(saved["updated"].as_str().is_some_and(|value| {
        value.ends_with('Z')
            && value.contains('T')
            && value.len() == "2026-06-25T00:00:00.000Z".len()
    }));

    let list = run(&["contacts"], &cwd, &maw_home, &plugins);
    assert_success(&list);
    assert_eq!(
        String::from_utf8(list.stdout).expect("stdout"),
        include_str!("fixtures/native-contacts/contacts-list.stdout")
    );
    assert_eq!(String::from_utf8(list.stderr).expect("stderr"), "");

    let remove = run(&["contact", "remove", "nova"], &cwd, &maw_home, &plugins);
    assert_success(&remove);
    assert_eq!(
        String::from_utf8(remove.stdout).expect("stdout"),
        include_str!("fixtures/native-contacts/contacts-rm.stdout")
    );
    assert_eq!(String::from_utf8(remove.stderr).expect("stderr"), "");

    let retired: Value = serde_json::from_str(
        &fs::read_to_string(&contacts_path).expect("contacts file remains after retire"),
    )
    .expect("contacts json");
    assert_eq!(retired["contacts"]["nova"]["retired"], true);

    let after_remove = run(&["contacts"], &cwd, &maw_home, &plugins);
    assert_success(&after_remove);
    assert_eq!(
        String::from_utf8(after_remove.stdout).expect("stdout"),
        include_str!("fixtures/native-contacts/contacts-after-rm.stdout")
    );
    assert_eq!(String::from_utf8(after_remove.stderr).expect("stderr"), "");
}

#[test]
fn contacts_plugin_honors_configured_psi_path_in_temp_maw_home() {
    let root = temp_dir("psi-path");
    let maw_home = root.join("home");
    let cwd = root.join("repo");
    let configured_psi = root.join("configured-psi");
    let plugins = install_contacts_plugin(&root);
    fs::create_dir_all(maw_home.join("config")).expect("config dir");
    fs::create_dir_all(&cwd).expect("cwd");
    fs::write(
        maw_home.join("config/maw.config.json"),
        format!(
            "{{\n  \"psiPath\": {}\n}}\n",
            serde_json::to_string(configured_psi.to_str().expect("utf8 path")).expect("path json")
        ),
    )
    .expect("config");

    let add = run(
        &["contacts", "add", "tk", "--maw=tonk"],
        &cwd,
        &maw_home,
        &plugins,
    );
    assert_success(&add);

    assert!(configured_psi.join("contacts.json").exists());
    assert!(!cwd.join("psi/contacts.json").exists());
    assert!(!cwd.join("ψ/contacts.json").exists());
}

#[test]
fn native_dispatcher_registration_is_removed_for_plugin_fallthrough() {
    assert_eq!(dispatcher_status("contacts"), DispatchKind::NativeError);
    assert_eq!(dispatcher_status("contact"), DispatchKind::NativeError);
}
