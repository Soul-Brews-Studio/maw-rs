#[test]
fn fs_write_and_remove_hard_deny_protected_security_state_paths() {
    let dir = temp("protected-state");
    let state = dir.join("state");
    create_dir_all(state.join("consent-pending")).expect("consent dir");
    create_dir_all(state.join("normal")).expect("normal dir");
    write(state.join("trust.json"), r#"{"version":1,"trust":{}}"#).expect("trust");
    write(state.join("peer-key"), "peer-secret").expect("peer key");
    write(state.join("audit.jsonl"), "{\"event\":\"host-only\"}\n").expect("audit");
    let host = host(&dir, &["fs:write:state"]).with_fs_root("state", &state);

    let trust = call(
        &host,
        "maw.fs.write",
        &json!({"path": state.join("trust.json"), "content": "{\"trust\":{\"pwn\":true}}", "mode": "overwrite"}),
    );
    assert_eq!(trust["ok"], false, "{trust}");
    assert_eq!(trust["code"], "capability_denied");
    assert!(trust["error"]
        .as_str()
        .unwrap_or_default()
        .contains("protected security-state"));
    assert_eq!(
        read_to_string(state.join("trust.json")).expect("trust unchanged"),
        r#"{"version":1,"trust":{}}"#
    );

    let remove_peer_key = call(
        &host,
        "maw.fs.remove",
        &json!({"path": state.join("peer-key"), "recursive": false}),
    );
    assert_eq!(remove_peer_key["ok"], false, "{remove_peer_key}");
    assert_eq!(remove_peer_key["code"], "capability_denied");
    assert_eq!(
        read_to_string(state.join("peer-key")).expect("peer key survives"),
        "peer-secret"
    );

    let audit = call(
        &host,
        "maw.fs.write",
        &json!({"path": state.join("audit.jsonl"), "content": "{\"event\":\"plugin\"}\n", "mode": "append"}),
    );
    assert_eq!(audit["ok"], false, "{audit}");
    assert_eq!(audit["code"], "capability_denied");
    assert_eq!(
        read_to_string(state.join("audit.jsonl")).expect("audit unchanged"),
        "{\"event\":\"host-only\"}\n"
    );

    let normal = call(
        &host,
        "maw.fs.write",
        &json!({"path": state.join("plugin-cache.json"), "content": "{}", "mode": "create"}),
    );
    assert_eq!(normal["ok"], true, "{normal}");
    assert_eq!(
        read_to_string(state.join("plugin-cache.json")).expect("normal write"),
        "{}"
    );
}

#[test]
fn fs_write_resolves_traversal_and_symlink_into_protected_state_before_deny() {
    let dir = temp("protected-resolve");
    let state = dir.join("state");
    create_dir_all(state.join("consent-pending")).expect("consent dir");
    create_dir_all(state.join("normal")).expect("normal dir");
    write(state.join("trust.json"), r#"{"version":1,"trust":{}}"#).expect("trust");
    let alias = dir.join("alias-consent");
    symlink(state.join("consent-pending"), &alias).expect("protected dir symlink");
    let host = host(&dir, &["fs:write:state"]).with_fs_root("state", &state);

    let traversal = call(
        &host,
        "maw.fs.write",
        &json!({"path": state.join("normal/../trust.json"), "content": "pwn", "mode": "overwrite"}),
    );
    assert_eq!(traversal["ok"], false, "{traversal}");
    assert_eq!(traversal["code"], "capability_denied");
    assert_eq!(
        read_to_string(state.join("trust.json")).expect("trust unchanged"),
        r#"{"version":1,"trust":{}}"#
    );

    let symlink_into_protected = call(
        &host,
        "maw.fs.write",
        &json!({"path": alias.join("req-evil.json"), "content": "{}", "mode": "create"}),
    );
    assert_eq!(
        symlink_into_protected["ok"], false,
        "{symlink_into_protected}"
    );
    assert_eq!(symlink_into_protected["code"], "capability_denied");
    assert!(
        !state.join("consent-pending/req-evil.json").exists(),
        "protected consent dir must not be written through symlink"
    );
}

#[test]
fn fs_remove_host_side_allows_only_declared_root_and_real_files() {
    if running_as_root() {
        eprintln!("skip root-only run: OS root bypasses host-side permission assumptions");
        return;
    }
    let dir = temp("remove-allowed");
    let victim = dir.join("victim.txt");
    write(&victim, "delete me").expect("victim");
    let host = host(&dir, &["fs:write:sandbox"]);

    let removed = call(
        &host,
        "maw.fs.remove",
        &json!({"path": victim, "recursive": false}),
    );

    assert_eq!(removed["ok"], true, "{removed}");
    assert!(!dir.join("victim.txt").exists());
    let audit = host.audit_json_lines();
    assert!(audit.contains("\"host_fn\":\"maw.fs.remove\""), "{audit}");
    assert!(
        audit.contains("\"capability\":\"fs:write:sandbox\""),
        "{audit}"
    );
}

#[test]
fn fs_remove_denies_outside_root_traversal_symlink_and_glob() {
    let dir = temp("remove-deny");
    let outside_dir = temp("remove-outside");
    let outside_file = outside_dir.join("outside.txt");
    write(&outside_file, "must survive").expect("outside");
    create_dir_all(dir.join("nested")).expect("nested");
    symlink(&outside_file, dir.join("nested/link-outside")).expect("symlink");
    let inside_file = dir.join("nested/inside.txt");
    write(&inside_file, "inside").expect("inside");
    let host = host(&dir, &["fs:write:sandbox"]);

    let outside = call(
        &host,
        "maw.fs.remove",
        &json!({"path": outside_file, "recursive": false}),
    );
    assert_eq!(outside["ok"], false, "{outside}");
    assert_eq!(outside["code"], "capability_denied");

    let traversal = call(
        &host,
        "maw.fs.remove",
        &json!({"path": dir.join("../").join(outside_dir.file_name().unwrap()).join("outside.txt"), "recursive": false}),
    );
    assert_eq!(traversal["ok"], false, "{traversal}");
    assert_eq!(traversal["code"], "capability_denied");

    let symlink_escape = call(
        &host,
        "maw.fs.remove",
        &json!({"path": dir.join("nested/link-outside"), "recursive": false}),
    );
    assert_eq!(symlink_escape["ok"], false, "{symlink_escape}");
    assert_eq!(symlink_escape["code"], "capability_denied");

    let glob = call(
        &host,
        "maw.fs.remove",
        &json!({"path": format!("{}/*.txt", dir.display()), "recursive": true}),
    );
    assert_eq!(glob["ok"], false, "{glob}");
    assert_eq!(glob["code"], "capability_denied");

    assert_eq!(
        read_to_string(&outside_file).expect("outside survives"),
        "must survive"
    );
    assert!(
        inside_file.exists(),
        "denied calls must not delete inside by accident"
    );
}

#[test]
fn fs_remove_recursive_is_confined_and_does_not_follow_symlink_escape() {
    if running_as_root() {
        eprintln!("skip root-only run: OS root bypasses host-side permission assumptions");
        return;
    }
    let dir = temp("remove-recursive");
    let outside_dir = temp("remove-recursive-outside");
    let outside_file = outside_dir.join("outside.txt");
    write(&outside_file, "outside").expect("outside");
    let tree = dir.join("tree");
    create_dir_all(tree.join("child")).expect("tree");
    write(tree.join("child/file.txt"), "inside").expect("inside");
    symlink(&outside_file, tree.join("child/link-outside")).expect("symlink");
    let host = host(&dir, &["fs:write:sandbox"]);

    let removed = call(
        &host,
        "maw.fs.remove",
        &json!({"path": tree, "recursive": true}),
    );

    assert_eq!(removed["ok"], true, "{removed}");
    assert!(!dir.join("tree").exists());
    assert_eq!(
        read_to_string(&outside_file).expect("outside survives"),
        "outside"
    );
}

// --- #72 blockers 2+4: manifest fs caps -> named host roots + safe recursive mkdir ---
