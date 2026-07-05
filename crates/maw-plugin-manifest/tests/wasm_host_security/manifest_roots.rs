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
