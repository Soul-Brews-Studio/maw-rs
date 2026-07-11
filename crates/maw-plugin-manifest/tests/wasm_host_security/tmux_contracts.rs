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

#[test]
fn tmux_command_host_requires_manage_cap_and_restricts_argv() {
    let dir = temp("tmux-command");
    let denied = call(
        &host(&dir, &["tmux:read"]),
        "maw.tmux.command",
        &json!({"command":"unlink-window","args":["-t","view:oracle"]}),
    );
    assert_eq!(denied["code"], "capability_denied", "{denied}");

    let host = host(&dir, &["tmux:raw:new-session", "tmux:raw:unlink-window"])
        .with_tmux_dry_run();
    let invalid = call(
        &host,
        "maw.tmux.command",
        &json!({"command":"new-session","args":["-d","-s","view","sh","-c","id"]}),
    );
    assert_eq!(invalid["code"], "invalid_args", "{invalid}");

    let allowed = call(
        &host,
        "maw.tmux.command",
        &json!({"command":"unlink-window","args":["-t","view:oracle"]}),
    );
    assert_eq!(allowed["ok"], true, "{allowed}");
    assert!(
        host.audit_json_lines()
            .contains("\"capability\":\"tmux:raw:unlink-window\"")
    );
}

#[test]
fn tmux_command_host_manages_mega_read_and_kill_shapes() {
    let dir = temp("tmux-command-mega");
    let read_host = host(&dir, &["tmux:read"]).with_tmux_dry_run();
    let list = call(
        &read_host,
        "maw.tmux.command",
        &json!({"command":"list-windows","args":["-t","01-alpha","-F","#{window_index}\t#{window_name}\t#{window_active}\t#{window_panes}"]}),
    );
    assert_eq!(list["ok"], true, "{list}");

    let denied = call(
        &read_host,
        "maw.tmux.command",
        &json!({"command":"kill-session","args":["-t","01-alpha"]}),
    );
    assert_eq!(denied["code"], "capability_denied", "{denied}");

    let kill_host = host(&dir, &["tmux:raw:kill-session"]).with_tmux_dry_run();
    let kill = call(
        &kill_host,
        "maw.tmux.command",
        &json!({"command":"kill-session","args":["-t","01-alpha"]}),
    );
    assert_eq!(kill["ok"], true, "{kill}");
    let injected = call(
        &kill_host,
        "maw.tmux.command",
        &json!({"command":"kill-session","args":["-t","-Sbad"]}),
    );
    assert_eq!(injected["code"], "invalid_args", "{injected}");
}

#[test]
fn tmux_command_host_manages_tile_shapes_exactly() {
    let dir = temp("tmux-tile");
    let host = host(&dir, &[
        "tmux:read", "tmux:raw:list-panes", "tmux:raw:split-window",
        "tmux:raw:select-pane", "tmux:raw:set-option", "tmux:raw:send-keys",
        "tmux:raw:select-layout", "tmux:raw:swap-pane", "tmux:raw:kill-pane",
    ]).with_tmux_dry_run();
    let allowed = [
        json!({"command":"display-message","args":["-p","#{pane_id}"]}),
        json!({"command":"list-panes","args":["-t","@7","-F","#{pane_id}|||#{pane_title}|||#{@maw_tile}"]}),
        json!({"command":"split-window","args":["-t","%1","-h","-P","-F","#{pane_id}","export MAW_TILE_ROLE='tile-1'; exec zsh"]}),
        json!({"command":"select-pane","args":["-t","%2","-T","tile-1"]}),
        json!({"command":"set-option","args":["-p","-t","%2","@maw_tile","1"]}),
        json!({"command":"send-keys","args":["-t","%2","-l","claude"]}),
        json!({"command":"select-layout","args":["-t","@7","main-vertical"]}),
        json!({"command":"swap-pane","args":["-s","%2","-t","%3"]}),
        json!({"command":"kill-pane","args":["-t","%2"]}),
    ];
    for request in allowed {
        let result = call(&host, "maw.tmux.command", &request);
        assert_eq!(result["ok"], true, "{request}: {result}");
    }
    for request in [
        json!({"command":"split-window","args":["-t","%1","sh","-c","id"]}),
        json!({"command":"set-option","args":["-p","-t","%2","@evil","1"]}),
        json!({"command":"send-keys","args":["-t","%2","-l","rm -rf"]}),
    ] {
        let result = call(&host, "maw.tmux.command", &request);
        assert_eq!(result["code"], "invalid_args", "{request}: {result}");
    }
}

#[test]
fn tmux_command_host_manages_select_layout_exactly() {
    let dir = temp("tmux-select-layout");
    let host = host(&dir, &["tmux:raw:select-layout"]).with_tmux_dry_run();

    for args in [json!(["main-vertical"]), json!(["-t", "team:work", "tiled"])] {
        let allowed = call(&host, "maw.tmux.command", &json!({"command":"select-layout","args":args}));
        assert_eq!(allowed["ok"], true, "{allowed}");
    }

    for args in [
        json!(["broken"]),
        json!(["-t", "bad\ntarget", "tiled"]),
        json!(["-t", "team:work", "tiled", "extra"]),
    ] {
        let denied = call(&host, "maw.tmux.command", &json!({"command":"select-layout","args":args}));
        assert_eq!(denied["code"], "invalid_args", "{denied}");
    }
}
