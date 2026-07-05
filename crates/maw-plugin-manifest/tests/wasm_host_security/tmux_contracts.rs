#[test]
fn hard_denies_sudo_independent_of_manifest() {
    let dir = temp("sudo");
    let host = host(
        &dir,
        &[
            "proc:exec:sudo",
            "proc:exec:su",
            "proc:exec:doas",
            "proc:exec:pkexec",
            "fs:read:sandbox",
        ],
    );
    for cmd in ["sudo", "su", "doas", "pkexec"] {
        let result = call(
            &host,
            "maw.exec.run",
            &json!({"cmd": cmd, "args": ["id"], "cwd": dir}),
        );
        assert_eq!(result["ok"], false, "{cmd}");
        assert_eq!(result["code"], "capability_denied", "{cmd}");
    }
}

#[test]
fn host_error_code_serializes_contract_labels() {
    assert_eq!(
        serde_json::to_value(HostErrorCode::CapabilityDenied).unwrap(),
        "capability_denied"
    );
}

#[test]
fn tmux_send_host_denies_destructive_keys_without_force_cap() {
    let dir = temp("tmux-destructive-deny");
    let host = host(&dir, &["tmux:send"])
        .with_tmux_pane_command("safe-pane", "bash")
        .with_tmux_dry_run();

    for keys in [
        json!(["C-c"]),
        json!(["rm -rf /tmp/pwn"]),
        json!(["kill 1234"]),
    ] {
        let denied = call(
            &host,
            "maw.tmux.send_keys",
            &json!({"target":"safe-pane","keys":keys,"literal":true}),
        );
        assert_eq!(denied["ok"], false, "{denied}");
        assert_eq!(denied["code"], "capability_denied", "{denied}");
    }
    assert_eq!(
        host.audit_json_lines(),
        "",
        "denied sends must not audit as host mutation"
    );
}

#[test]
fn tmux_send_host_denies_ai_pane_collision_without_force_or_explicit_allow() {
    let dir = temp("tmux-ai-deny");
    let host = host(&dir, &["tmux:send"])
        .with_tmux_pane_command("ai-pane", "claude")
        .with_tmux_dry_run();

    let denied = call(
        &host,
        "maw.tmux.send_keys",
        &json!({"target":"ai-pane","keys":["hello"],"literal":true}),
    );

    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied", "{denied}");
    assert_eq!(
        host.audit_json_lines(),
        "",
        "AI collision deny must happen before mutation audit"
    );
}

#[test]
fn tmux_send_host_allows_non_destructive_send_with_plain_cap_only() {
    let dir = temp("tmux-safe-allow");
    let host = host(&dir, &["tmux:send"])
        .with_tmux_pane_command("safe-pane", "bash")
        .with_tmux_dry_run();

    let allowed = call(
        &host,
        "maw.tmux.send_keys",
        &json!({"target":"safe-pane","keys":["hello world"],"literal":true}),
    );

    assert_eq!(allowed["ok"], true, "{allowed}");
    assert_eq!(allowed["value"]["sent"], true);
    let audit = host.audit_json_lines();
    assert!(
        audit.contains("\"host_fn\":\"maw.tmux.send_keys\""),
        "{audit}"
    );
    assert!(audit.contains("\"capability\":\"tmux:send\""), "{audit}");
    assert!(
        !audit.contains("tmux:send:force"),
        "plain send over-declared force: {audit}"
    );
}

#[test]
fn tmux_send_host_allows_destructive_send_with_force_cap() {
    let dir = temp("tmux-force-allow");
    let host = host(&dir, &["tmux:send:force"])
        .with_tmux_pane_command("ai-pane", "claude")
        .with_tmux_dry_run();

    let allowed = call(
        &host,
        "maw.tmux.send_keys",
        &json!({"target":"ai-pane","keys":["C-c"],"literal":true}),
    );

    assert_eq!(allowed["ok"], true, "{allowed}");
    assert_eq!(allowed["value"]["destructive"], true);
    let audit = host.audit_json_lines();
    assert!(
        audit.contains("\"capability\":\"tmux:send:force\""),
        "{audit}"
    );
}

