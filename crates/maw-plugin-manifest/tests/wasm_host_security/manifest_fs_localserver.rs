#[test]
fn manifest_accepts_entry_object_wasm_form() {
    let dir = temp("entry-object");
    let parsed = manifest(&dir, &["fs:read:sandbox"]);
    assert_eq!(parsed.entry.as_deref(), Some("plugin.wasm"));
    assert_eq!(parsed.target, None);
}

#[test]
fn fs_read_denies_symlink_escape_and_proc() {
    let dir = temp("symlink");
    write(dir.join("safe.txt"), "ok").expect("safe");
    symlink("/etc/passwd", dir.join("escape")).expect("symlink");
    let host = host(&dir, &["fs:read:sandbox"]);

    let safe = call(&host, "maw.fs.read", &json!({"path": dir.join("safe.txt")}));
    assert_eq!(safe["ok"], true);
    assert_eq!(safe["value"]["content"], "ok");

    let escaped = call(&host, "maw.fs.read", &json!({"path": dir.join("escape")}));
    assert_eq!(escaped["ok"], false);
    assert_eq!(escaped["code"], "capability_denied");

    let proc = call(&host, "maw.fs.read", &json!({"path": "/proc/self/cmdline"}));
    assert_eq!(proc["ok"], false);
}

#[test]
fn fs_write_uses_nofollow_and_denies_existing_symlink() {
    let dir = temp("write-symlink");
    let outside = temp("outside").join("pwned.txt");
    write(&outside, "outside").expect("outside");
    symlink(&outside, dir.join("link.txt")).expect("symlink");
    let host = host(&dir, &["fs:write:sandbox"]);

    let denied = call(
        &host,
        "maw.fs.write",
        &json!({"path": dir.join("link.txt"), "content": "secret" , "mode": "overwrite"}),
    );
    assert_eq!(denied["ok"], false);
    assert_eq!(
        read_to_string(&outside).expect("outside unchanged"),
        "outside"
    );
}

#[test]
fn secret_bytes_are_redacted_from_audit_and_headers() {
    let dir = temp("redact");
    let host = host(&dir, &["net:https:example.com"]);
    let result = call(
        &host,
        "maw.http.request",
        &json!({
            "method": "GET",
            "url": "https://example.com/secret-token-value",
            "headers": { "Authorization": "peerKey-secret-token-value" },
            "timeoutMs": 1
        }),
    );
    assert_eq!(result["ok"], false);
    let audit = host.audit_json_lines();
    assert!(
        !audit.contains("peerKey-secret-token-value"),
        "audit leaked secret: {audit}"
    );
    assert!(
        !audit.contains("Authorization"),
        "audit leaked header name/value: {audit}"
    );
}

#[test]
fn localserver_request_is_host_pinned_and_capability_gated() {
    let dir = temp("localserver-host-direct");
    let base = spawn_localserver_once(r#"{"ok":true,"source":"maw-server"}"#);
    let actual_url = format!("{base}/api/probe");
    let wrong_port = if base.ends_with(":65535") {
        "http://127.0.0.1:65534/api/probe".to_owned()
    } else {
        "http://127.0.0.1:65535/api/probe".to_owned()
    };
    let pinned = host(&dir, &["sdk:localserver"]).with_localserver_url(&base);

    for denied_url in [
        wrong_port.as_str(),
        "http://127.0.0.2:31745/api/probe",
        "http://[::1]:31745/api/probe",
        "http://10.0.0.7:31745/api/probe",
    ] {
        let denied = call(
            &pinned,
            "maw.localserver.request",
            &json!({"method": "GET", "url": denied_url}),
        );
        assert_eq!(denied["ok"], false, "{denied_url}: {denied}");
        assert_eq!(
            denied["code"], "capability_denied",
            "{denied_url}: {denied}"
        );
    }

    let no_cap = host(&dir, &[]).with_localserver_url(&base);
    let cap_denied = call(
        &no_cap,
        "maw.localserver.request",
        &json!({"method": "GET", "url": actual_url}),
    );
    assert_eq!(cap_denied["ok"], false);
    assert_eq!(cap_denied["code"], "capability_denied");

    let allowed = call(
        &pinned,
        "maw.localserver.request",
        &json!({"method": "GET", "url": actual_url}),
    );
    assert_eq!(allowed["ok"], true, "{allowed}");
    assert_eq!(allowed["value"]["status"], 200);
    assert!(
        allowed["value"]["body"]
            .as_str()
            .unwrap_or_default()
            .contains("maw-server"),
        "{allowed}"
    );
}

#[test]
fn general_http_loopback_deny_still_applies_with_localserver_cap() {
    let dir = temp("localserver-does-not-weaken-http");
    let host = host(&dir, &["sdk:localserver", "net:http:127.0.0.1"]);
    let denied = call(
        &host,
        "maw.http.request",
        &json!({"method": "GET", "url": "http://127.0.0.1:31745/api/probe"}),
    );
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["code"], "capability_denied");
}

