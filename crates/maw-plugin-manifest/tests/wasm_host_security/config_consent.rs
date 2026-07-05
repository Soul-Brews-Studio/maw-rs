#[test]
fn capability_denied_uses_error_envelope_and_private_net_hard_deny() {
    let dir = temp("cap-deny");
    let host = host(&dir, &["fs:read:sandbox", "net:http:127.0.0.1"]);
    let fs = call(
        &host,
        "maw.fs.write",
        &json!({"path": dir.join("x"), "content": "x"}),
    );
    assert_eq!(fs["ok"], false);
    assert_eq!(fs["code"], "capability_denied");

    let http = call(
        &host,
        "maw.http.request",
        &json!({"method": "GET", "url": "http://127.0.0.1/"}),
    );
    assert_eq!(http["ok"], false);
    assert_eq!(http["code"], "capability_denied");
}

#[test]
fn config_set_writes_config_store_and_audits_key_before_mutate() {
    let dir = temp("config-set");
    let host = host(&dir, &["sdk:config:write", "sdk:config:read"]).with_fs_root("config", &dir);

    let set = call(
        &host,
        "maw.config.set",
        &json!({"key": "node", "value": "nova-node", "patch": {"node": "ignored"}}),
    );
    assert_eq!(set["ok"], true);
    assert_eq!(set["value"]["finalValue"], "nova-node");
    assert_eq!(set["value"]["audit"], "config-write");

    let stored: Value =
        serde_json::from_str(&read_to_string(dir.join("maw.config.json")).expect("written config"))
            .expect("config json");
    assert_eq!(stored["node"], "nova-node");

    let get = call(&host, "maw.config.get", &json!({"key": "node"}));
    assert_eq!(get["ok"], true);
    assert_eq!(get["value"]["value"], "nova-node");

    let audit = host.audit_json_lines();
    assert!(audit.contains("\"host_fn\":\"maw.config.set\""), "{audit}");
    assert!(
        audit.contains("\"capability\":\"sdk:config:write\""),
        "{audit}"
    );
    assert!(audit.contains("\"resource\":\"config:node\""), "{audit}");
}

#[test]
fn config_set_secret_key_is_denied_by_host_even_without_guest_censor() {
    let dir = temp("config-secret-deny");
    let host = host(&dir, &["sdk:config:write"]).with_fs_root("config", &dir);

    for key in [
        "secret",
        "federationToken",
        "apikey",
        "api_key",
        "peerkey",
        "peer_key",
        "nested.key",
        "key",
        "db_password",
        "password",
        "private_key",
        "credential",
        "passwd",
        "pwd",
        "passphrase",
        "cert",
        "tls.pem",
        "secrets.env",
        "oauth",
        "auth_token",
        "auth-token",
        "authtoken",
    ] {
        let denied = call(
            &host,
            "maw.config.set",
            &json!({"key": key, "value": "must-not-write"}),
        );
        assert_eq!(denied["ok"], false, "{key}");
        assert_eq!(denied["code"], "capability_denied", "{key}");
    }
    assert!(
        !dir.join("maw.config.json").exists(),
        "denied secret writes must not create config"
    );
}

#[test]
fn config_set_unknown_non_secret_key_is_denied_by_default() {
    let dir = temp("config-unknown-deny");
    let host = host(&dir, &["sdk:config:write"]).with_fs_root("config", &dir);

    for (key, value) in [
        ("author", json!("Ada")),
        ("authorName", json!("Ada Lovelace")),
        ("editor", json!("vim")),
        ("display.theme", json!("dark")),
    ] {
        let denied = call(
            &host,
            "maw.config.set",
            &json!({"key": key, "value": value}),
        );
        assert_eq!(denied["ok"], false, "{key}: {denied}");
        assert_eq!(denied["code"], "capability_denied", "{key}");
    }
    assert!(
        !dir.join("maw.config.json").exists(),
        "deny-by-default writes must not create config"
    );
}

#[test]
fn config_set_allowlisted_key_still_denies_nested_secret_values() {
    let dir = temp("config-nested-secret-deny");
    let host = host(&dir, &["sdk:config:write"]).with_fs_root("config", &dir);

    for value in [
        json!({"token": "must-not-write"}),
        json!({"auth": {"password": "nope"}}),
    ] {
        let denied = call(
            &host,
            "maw.config.set",
            &json!({"key": "node", "value": value}),
        );
        assert_eq!(denied["ok"], false, "{denied}");
        assert_eq!(denied["code"], "capability_denied");
    }
    assert!(
        !dir.join("maw.config.json").exists(),
        "nested secret writes must not create config"
    );
}

#[test]
fn config_set_without_write_capability_is_denied_by_host() {
    let dir = temp("config-cap-deny");
    let host = host(&dir, &["sdk:config:read"]).with_fs_root("config", &dir);

    let denied = call(
        &host,
        "maw.config.set",
        &json!({"key": "node", "value": "nova-node"}),
    );
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["code"], "capability_denied");
    assert!(
        !dir.join("maw.config.json").exists(),
        "cap-denied write must not create config"
    );
}

#[test]
fn consent_read_uses_read_capability_and_never_exposes_pin_hash() {
    let dir = temp("consent-read");
    let state = dir.join("state");
    create_dir_all(state.join("consent-pending")).expect("pending dir");
    write(
        state.join("consent-pending/req-1.json"),
        r#"{
  "id": "req-1",
  "from": "nova",
  "to": "tk",
  "action": "hey",
  "summary": "Allow Nova to say hello",
  "pinHash": "sha256:must-not-leak",
  "createdAt": "2026-06-24T09:00:00.000Z",
  "expiresAt": "2099-01-01T00:00:00.000Z",
  "status": "pending"
}
"#,
    )
    .expect("pending");
    write(
        state.join("trust.json"),
        r#"{
  "version": 1,
  "trust": {
    "tk→nova:hey": {
      "from": "tk",
      "to": "nova",
      "action": "hey",
      "approvedAt": "2026-06-20T10:00:00.000Z",
      "approvedBy": "human",
      "requestId": "req-1"
    }
  }
}
"#,
    )
    .expect("trust");
    let host = host(&dir, &["sdk:consent:read"]).with_fs_root("state", &state);

    let pending = call(&host, "maw.consent.read", &json!({"view": "pending"}));
    assert_eq!(pending["ok"], true);
    assert!(pending["value"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("req-1"));
    assert!(
        !pending.to_string().contains("must-not-leak"),
        "pin hash leaked to WASM guest: {pending}"
    );

    let trust = call(&host, "maw.consent.read", &json!({"view": "trust"}));
    assert_eq!(trust["ok"], true);
    assert!(trust["value"]["text"]
        .as_str()
        .unwrap_or_default()
        .contains("tk → nova"));
    let audit = host.audit_json_lines();
    assert!(
        audit.contains("\"host_fn\":\"maw.consent.read\""),
        "{audit}"
    );
    assert!(
        audit.contains("\"capability\":\"sdk:consent:read\""),
        "{audit}"
    );
}

