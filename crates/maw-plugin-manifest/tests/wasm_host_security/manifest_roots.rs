fn host_manifest_roots(dir: &Path, home: &Path, caps: &[&str]) -> MawWasmHost {
    let manifest = manifest(dir, caps);
    let loaded = maw_plugin_manifest::LoadedPlugin {
        manifest,
        dir: dir.to_path_buf(),
        wasm_path: dir.join("plugin.wasm"),
        entry_path: None,
        wasm_export: "handle".to_owned(),
        kind: maw_plugin_manifest::LoadedPluginKind::Wasm,
        disabled: false,
    };
    MawWasmHost::new(&loaded).with_manifest_fs_roots_from(home)
}

fn teams_root(home: &Path) -> PathBuf {
    home.join(".claude").join("teams")
}

static VAULT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct VaultEnvRestore {
    saved: [(&'static str, Option<std::ffi::OsString>); 4],
}

impl Drop for VaultEnvRestore {
    fn drop(&mut self) {
        for (key, value) in self.saved.iter() {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }
}

fn with_vault_env<T>(
    vault_root: Option<&Path>,
    config_dir: Option<&Path>,
    home: &Path,
    test: impl FnOnce() -> T,
) -> T {
    let _guard = VAULT_ENV_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let _restore = VaultEnvRestore {
        saved: ["MAW_VAULT_ROOT", "MAW_CONFIG_DIR", "MAW_HOME", "HOME"]
            .map(|key| (key, std::env::var_os(key))),
    };
    match vault_root {
        Some(path) => std::env::set_var("MAW_VAULT_ROOT", path),
        None => std::env::remove_var("MAW_VAULT_ROOT"),
    }
    match config_dir {
        Some(path) => std::env::set_var("MAW_CONFIG_DIR", path),
        None => std::env::remove_var("MAW_CONFIG_DIR"),
    }
    std::env::remove_var("MAW_HOME");
    std::env::set_var("HOME", home);
    test()
}

fn assert_host_error(value: &Value, code: &str) {
    assert_eq!(value["ok"], false, "{value}");
    assert_eq!(value["code"], code, "{value}");
}

#[test]
fn vault_read_cap_grants_env_root_for_paths_list_and_read() {
    let dir = temp("vault-env-plugin");
    let home = temp("vault-env-home");
    let vault = temp("vault-root");
    create_dir_all(&vault).expect("vault dir");
    with_vault_env(Some(&vault), None, &home, || {
        let host = host_manifest_roots(&dir, &home, &["fs:read:vault"]);
        let note = vault.join("neo/inbox.md");
        create_dir_all(note.parent().expect("note parent")).expect("note parent dir");
        write(&note, "hello-vault").expect("seed note");

        let resolved = call(&host, "maw.paths.get", &json!({ "name": "vault" }));
        assert_eq!(resolved["ok"], true, "{resolved}");
        assert_eq!(resolved["value"]["path"], vault.display().to_string());
        let listed = call(
            &host,
            "maw.fs.list",
            &json!({ "path": vault, "recursive": true }),
        );
        assert_eq!(listed["ok"], true, "{listed}");
        assert!(listed["value"]["entries"]
            .as_array()
            .is_some_and(|entries| {
                entries
                    .iter()
                    .any(|entry| entry["path"] == note.display().to_string())
            }));
        let ok = call(&host, "maw.fs.read", &json!({ "path": note }));
        assert_eq!(ok["value"]["content"], "hello-vault", "{ok}");

        let write_host = host_manifest_roots(&dir, &home, &["fs:read:vault", "fs:write:vault"]);
        assert_host_error(
            &call(
                &write_host,
                "maw.fs.write",
                &json!({ "path": vault.join("pwn.md"), "content": "pwn" }),
            ),
            "capability_denied",
        );
    });
}

#[test]
fn vault_root_can_fall_back_to_config() {
    let dir = temp("vault-config-plugin");
    let home = temp("vault-config-home");
    let vault = temp("vault-root");
    let config_dir = temp("vault-config-dir");
    create_dir_all(&vault).expect("vault dir");
    write(
        config_dir.join("maw.config.json"),
        json!({ "vaultRoot": vault }).to_string(),
    )
    .expect("config");
    with_vault_env(None, Some(&config_dir), &home, || {
        let host = host_manifest_roots(&dir, &home, &["fs:read:vault"]);
        let note = vault.join("atlas/inbox.md");
        create_dir_all(note.parent().expect("note parent")).expect("note parent dir");
        write(&note, "from-config").expect("note");
        let resolved = call(&host, "maw.paths.get", &json!({ "name": "vault" }));
        assert_eq!(
            resolved["value"]["path"],
            vault.display().to_string(),
            "{resolved}"
        );
        let ok = call(&host, "maw.fs.read", &json!({ "path": note }));
        assert_eq!(ok["value"]["content"], "from-config", "{ok}");
    });
}

#[test]
fn vault_without_cap_or_root_is_denied_or_missing() {
    let dir = temp("vault-deny-plugin");
    let home = temp("vault-deny-home");
    let vault = temp("vault-root");
    create_dir_all(&vault).expect("vault dir");
    let note = vault.join("msg.md");
    write(&note, "secret").expect("note");
    with_vault_env(Some(&vault), None, &home, || {
        let host = host_manifest_roots(&dir, &home, &[]);
        assert_host_error(
            &call(&host, "maw.paths.get", &json!({ "name": "vault" })),
            "capability_denied",
        );
        assert_host_error(
            &call(&host, "maw.fs.read", &json!({ "path": note })),
            "capability_denied",
        );
    });
    let empty_config = temp("vault-empty-config");
    with_vault_env(None, Some(&empty_config), &home, || {
        let host = host_manifest_roots(&dir, &home, &["fs:read:vault"]);
        let missing = call(&host, "maw.paths.get", &json!({ "name": "vault" }));
        assert_host_error(&missing, "not_found");
        assert!(missing["error"]
            .as_str()
            .unwrap_or_default()
            .contains("MAW_VAULT_ROOT"));
    });
}

#[test]
fn vault_read_cap_rejects_traversal_and_absolute_outside_paths() {
    let dir = temp("vault-traversal-plugin");
    let home = temp("vault-traversal-home");
    let vault = temp("vault-root");
    let outside = temp("vault-outside");
    create_dir_all(&vault).expect("vault dir");
    let outside_note = outside.join("secret.md");
    write(&outside_note, "outside").expect("outside");
    with_vault_env(Some(&vault), None, &home, || {
        let host = host_manifest_roots(&dir, &home, &["fs:read:vault"]);
        let via_parent = vault
            .join("..")
            .join(outside.file_name().expect("outside leaf"))
            .join("secret.md");
        for path in [via_parent, outside_note] {
            assert_host_error(
                &call(&host, "maw.fs.read", &json!({ "path": path })),
                "capability_denied",
            );
        }
    });
}

#[test]
fn manifest_read_cap_grants_exactly_the_named_teams_root() {
    let dir = temp("caps-read-plugin");
    let home = temp("caps-read-home");
    let host = host_manifest_roots(&dir, &home, &["fs:read:teams"]);

    // The registry created ~/.claude/teams; a file placed there is readable.
    let note = teams_root(&home).join("note.txt");
    write(&note, "hello-team").expect("seed note");
    let ok = call(&host, "maw.fs.read", &json!({ "path": note }));
    assert_eq!(ok["ok"], true, "{ok}");
    assert_eq!(ok["value"]["content"], "hello-team");

    // A path outside the granted root is denied, even though it exists.
    let outside = dir.join("outside.txt");
    write(&outside, "nope").expect("outside");
    let denied = call(&host, "maw.fs.read", &json!({ "path": outside }));
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied");

    // read cap does NOT confer write.
    let write_denied = call(
        &host,
        "maw.fs.write",
        &json!({ "path": teams_root(&home).join("x.txt"), "content": "x" }),
    );
    assert_eq!(write_denied["ok"], false, "{write_denied}");
    assert_eq!(write_denied["code"], "capability_denied");
}

#[test]
fn manifest_write_cap_grants_write_not_read() {
    let dir = temp("caps-write-plugin");
    let home = temp("caps-write-home");
    let host = host_manifest_roots(&dir, &home, &["fs:write:teams"]);

    let target = teams_root(&home).join("created.txt");
    let ok = call(
        &host,
        "maw.fs.write",
        &json!({ "path": target, "content": "written", "mode": "create" }),
    );
    assert_eq!(ok["ok"], true, "{ok}");
    assert_eq!(read_to_string(target).expect("written"), "written");

    // write cap alone leaves read roots empty -> read is denied.
    let seeded = teams_root(&home).join("seed.txt");
    write(&seeded, "seed").expect("seed");
    let denied = call(&host, "maw.fs.read", &json!({ "path": seeded }));
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied");
}

#[test]
fn undeclared_caps_grant_no_roots() {
    let dir = temp("caps-none-plugin");
    let home = temp("caps-none-home");
    // No fs caps declared -> registry grants nothing, teams dir is never made.
    let host = host_manifest_roots(&dir, &home, &["net:https:example.com"]);
    assert!(
        !teams_root(&home).exists(),
        "no fs cap must not create the root"
    );

    // Manually create the dir + a file; the host still may not read it.
    create_dir_all(teams_root(&home)).expect("teams dir");
    let note = teams_root(&home).join("note.txt");
    write(&note, "secret").expect("note");
    let denied = call(&host, "maw.fs.read", &json!({ "path": note }));
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied");
}

#[test]
fn unknown_scope_names_and_wildcards_grant_no_root() {
    let dir = temp("caps-unknown-plugin");
    let home = temp("caps-unknown-home");
    // None of these scopes are in the fixed registry: no path is ever mapped.
    let host = host_manifest_roots(
        &dir,
        &home,
        &["fs:read:secrets", "fs:read:*", "fs:read:/etc", "fs:write:*"],
    );
    assert!(!teams_root(&home).exists());

    create_dir_all(teams_root(&home)).expect("teams dir");
    let note = teams_root(&home).join("note.txt");
    write(&note, "secret").expect("note");
    let teams_denied = call(&host, "maw.fs.read", &json!({ "path": note }));
    assert_eq!(teams_denied["ok"], false, "{teams_denied}");

    // A manifest can never reach an absolute path by naming it as a scope.
    let etc_denied = call(&host, "maw.fs.read", &json!({ "path": "/etc/hosts" }));
    assert_eq!(etc_denied["ok"], false, "{etc_denied}");
    assert_eq!(etc_denied["code"], "capability_denied");
}

#[test]
fn mkdirp_creates_nested_dirs_within_root_then_reads_back() {
    let dir = temp("mkdirp-plugin");
    let home = temp("mkdirp-home");
    let host = host_manifest_roots(&dir, &home, &["fs:write:teams", "fs:read:teams"]);

    let nested = teams_root(&home)
        .join("squad")
        .join("state")
        .join("session.json");
    let ok = call(
        &host,
        "maw.fs.write",
        &json!({ "path": nested, "content": "{\"n\":1}", "mode": "create", "mkdirp": true }),
    );
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(teams_root(&home).join("squad/state").is_dir());

    // Round-trip through the host so the read-side TOCTOU checks also run.
    let back = call(&host, "maw.fs.read", &json!({ "path": nested }));
    assert_eq!(back["ok"], true, "{back}");
    assert_eq!(back["value"]["content"], "{\"n\":1}");
}

#[test]
fn mkdir_host_fn_creates_nested_dirs_within_root() {
    let dir = temp("mkdir-plugin");
    let home = temp("mkdir-home");
    let host = host_manifest_roots(&dir, &home, &["fs:write:teams"]);

    let target = teams_root(&home).join("a").join("b").join("c");
    let ok = call(&host, "maw.fs.mkdir", &json!({ "path": target }));
    assert_eq!(ok["ok"], true, "{ok}");
    assert!(target.is_dir());
}

#[test]
fn mkdir_denies_undeclared_write() {
    let dir = temp("mkdir-nowrite-plugin");
    let home = temp("mkdir-nowrite-home");
    let host = host_manifest_roots(&dir, &home, &["fs:read:teams"]);

    let target = teams_root(&home).join("nope");
    let denied = call(&host, "maw.fs.mkdir", &json!({ "path": target }));
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied");
    assert!(!target.exists());
}

#[test]
fn mkdirp_symlink_ancestor_escape_is_denied() {
    if running_as_root() {
        eprintln!("skip root-only run: OS root bypasses host-side permission assumptions");
        return;
    }
    let dir = temp("mkdirp-symlink-plugin");
    let home = temp("mkdirp-symlink-home");
    let host = host_manifest_roots(&dir, &home, &["fs:write:teams"]);

    // A symlink inside the root pointing OUT of the root must not be traversed.
    let outside = temp("mkdirp-symlink-outside");
    let evil = teams_root(&home).join("evil");
    symlink(&outside, &evil).expect("symlink");

    let escaped = teams_root(&home).join("evil").join("sub").join("file.txt");
    let denied = call(
        &host,
        "maw.fs.write",
        &json!({ "path": escaped, "content": "pwned", "mode": "create", "mkdirp": true }),
    );
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied");
    assert!(
        !outside.join("sub").exists(),
        "must not create outside the root"
    );

    // maw.fs.mkdir must refuse the same escape.
    let mkdir_denied = call(
        &host,
        "maw.fs.mkdir",
        &json!({ "path": teams_root(&home).join("evil").join("sub2") }),
    );
    assert_eq!(mkdir_denied["ok"], false, "{mkdir_denied}");
    assert!(!outside.join("sub2").exists());
}

#[test]
fn mkdirp_parent_traversal_escape_is_denied() {
    let dir = temp("mkdirp-traversal-plugin");
    let home = temp("mkdirp-traversal-home");
    let host = host_manifest_roots(&dir, &home, &["fs:write:teams"]);

    // `..` climbs out of teams into ~/.claude (a real dir, but outside the
    // granted root). Ancestor resolution must reject before creating anything.
    let escaped = teams_root(&home)
        .join("..")
        .join("escapee")
        .join("file.txt");
    let denied = call(
        &host,
        "maw.fs.write",
        &json!({ "path": escaped, "content": "pwned", "mode": "create", "mkdirp": true }),
    );
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied");
    assert!(
        !home.join(".claude").join("escapee").exists(),
        "must not create outside the root"
    );
}
