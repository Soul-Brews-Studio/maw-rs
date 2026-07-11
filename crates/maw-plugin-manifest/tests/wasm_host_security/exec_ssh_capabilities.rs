#[test]
fn batch3_host_direct_denies_ssrf_undeclared_exec_and_privileged_exec() {
    let dir = temp("batch3-host-direct");

    let ssrf_host = host(&dir, &["net:http:127.0.0.1"]);
    let ssrf = call(
        &ssrf_host,
        "maw.http.request",
        &json!({"method": "GET", "url": "http://127.0.0.1:3456/api/identity"}),
    );
    assert_eq!(ssrf["ok"], false);
    assert_eq!(ssrf["code"], "capability_denied");

    let exec_host = host(&dir, &["proc:exec:git", "fs:read:sandbox"]);
    let undeclared = call(
        &exec_host,
        "maw.exec.run",
        &json!({"cmd": "sh", "args": ["-c", "echo pwned"], "cwd": dir, "allowNonZero": true}),
    );
    assert_eq!(undeclared["ok"], false);
    assert_eq!(undeclared["code"], "capability_denied");

    let privileged_host = host(&dir, &["proc:exec:sudo", "fs:read:sandbox"]);
    let privileged = call(
        &privileged_host,
        "maw.exec.run",
        &json!({"cmd": "sudo", "args": ["git", "status"], "cwd": dir, "allowNonZero": true}),
    );
    assert_eq!(privileged["ok"], false);
    assert_eq!(privileged["code"], "capability_denied");
}

#[test]
fn cli_run_requires_the_exact_native_command_capability() {
    let dir = temp("cli-run-capability");
    let host = host(&dir, &["cli:run:costs"]);
    let denied = call(
        &host,
        "maw.cli.run",
        &json!({"command": "bud", "args": ["sprout", "--dry-run"]}),
    );
    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "capability_denied", "{denied}");
}

#[test]
fn ssh_exec_refuses_option_injection_host_before_ssh_spawn() {
    let dir = temp("ssh-host-option-injection");
    let payload = dir.join("proxycommand-payload");
    let injected_host = format!("-oProxyCommand=touch+{}", payload.display());
    let host = host(&dir, &["shell:ssh:*", "proc:exec:ssh"]);

    let denied = call(
        &host,
        "maw.ssh.exec",
        &json!({"host": injected_host, "cmd": "true", "args": [], "timeoutMs": 100}),
    );

    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "invalid_args", "{denied}");
    assert!(
        !payload.exists(),
        "ssh option-injection payload must not run before rejection"
    );
}

#[test]
fn ssh_tmux_capture_refuses_option_injection_target_before_ssh_spawn() {
    let dir = temp("ssh-tmux-capture-target-option-injection");
    let payload = dir.join("tmux-capture-payload");
    let host = host(&dir, &["shell:ssh:safe-host", "proc:exec:ssh"]);

    let denied = call(
        &host,
        "maw.ssh.tmux_capture",
        &json!({"host": "safe-host", "target": "-X", "lines": 5}),
    );

    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "invalid_args", "{denied}");
    assert!(
        !payload.exists(),
        "tmux target option-injection payload sentinel must remain absent"
    );
}

#[test]
fn ssh_tmux_send_keys_refuses_option_injection_target_before_ssh_spawn() {
    let dir = temp("ssh-tmux-send-target-option-injection");
    let payload = dir.join("tmux-send-payload");
    let host = host(&dir, &["shell:ssh:safe-host", "proc:exec:ssh"]);

    let denied = call(
        &host,
        "maw.ssh.tmux_send_keys",
        &json!({"host": "safe-host", "target": "-X", "keys": ["Enter"]}),
    );

    assert_eq!(denied["ok"], false, "{denied}");
    assert_eq!(denied["code"], "invalid_args", "{denied}");
    assert!(
        !payload.exists(),
        "tmux target option-injection payload sentinel must remain absent"
    );
}

#[test]
fn exec_run_preserves_legitimate_leading_dash_args_for_generic_exec() {
    let dir = temp("exec-generic-leading-dash-args");
    let host = host(&dir, &["proc:exec:git", "fs:read:sandbox"]);

    let result = call(
        &host,
        "maw.exec.run",
        &json!({"cmd": "git", "args": ["--version"], "cwd": dir, "allowNonZero": true}),
    );

    assert_eq!(result["ok"], true, "{result}");
    assert!(
        result["value"]["stdout"]
            .as_str()
            .unwrap_or_default()
            .contains("git version"),
        "{result}"
    );
}

#[test]
fn exec_enforces_capability_and_env_allowlist() {
    let dir = temp("exec");
    let host = host(&dir, &["proc:exec:env", "fs:read:sandbox"]);
    let denied_env = call(
        &host,
        "maw.exec.run",
        &json!({
            "cmd": "env",
            "cwd": dir,
            "env": { "SECRET_TOKEN": "do-not-pass" },
            "allowNonZero": true
        }),
    );
    assert_eq!(denied_env["ok"], false);
    assert_eq!(denied_env["code"], "capability_denied");

    let out = call(
        &host,
        "maw.exec.run",
        &json!({
            "cmd": "env",
            "cwd": dir,
            "env": { "MAW_VISIBLE": "yes", "HOME": "/should/not/inherit" },
            "allowNonZero": true
        }),
    );
    assert_eq!(out["ok"], true);
    let stdout = out["value"]["stdout"].as_str().unwrap_or_default();
    assert!(stdout.contains("MAW_VISIBLE=yes"));
    assert!(!stdout.contains("HOME=/should/not/inherit"));
}
