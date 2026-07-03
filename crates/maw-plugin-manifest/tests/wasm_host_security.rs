use std::fs::{create_dir_all, read_to_string, write};
use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, TcpListener};
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use maw_plugin_manifest::{parse_manifest, HostErrorCode, MawWasmHost, PluginManifest};
use serde_json::{json, Value};

fn temp(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "maw-wasm-host-{label}-{}-{nonce}",
        std::process::id()
    ));
    create_dir_all(&dir).expect("temp dir");
    dir
}

fn manifest(dir: &Path, caps: &[&str]) -> PluginManifest {
    write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("wasm");
    parse_manifest(
        &json!({
            "name": "secure-plugin",
            "version": "1.0.0",
            "sdk": "*",
            "entry": { "kind": "wasm", "path": "plugin.wasm", "export": "handle" },
            "capabilities": caps,
        })
        .to_string(),
        dir,
    )
    .expect("manifest")
}

fn host(dir: &Path, caps: &[&str]) -> MawWasmHost {
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
    MawWasmHost::new(&loaded).with_fs_root("sandbox", dir)
}

fn call(host: &MawWasmHost, name: &str, args: &Value) -> Value {
    serde_json::from_str(&host.handle_json(name, &args.to_string())).expect("host result json")
}

fn running_as_root() -> bool {
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .is_some_and(|uid| uid.trim() == "0")
}

fn spawn_localserver_once(body: &'static str) -> String {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind localserver");
    let addr = listener.local_addr().expect("localserver addr");
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept localserver request");
        let mut buf = [0_u8; 1024];
        let _ = stream.read(&mut buf);
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write localserver response");
    });
    format!("http://127.0.0.1:{}", addr.port())
}

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

#[test]
fn consent_guest_approval_and_trust_host_fns_are_hard_denied() {
    let dir = temp("consent-deny");
    let host = host(
        &dir,
        &[
            "sdk:consent:read",
            "sdk:consent:write",
            "sdk:consent:approve",
        ],
    );

    for name in [
        "maw.consent.approve",
        "maw.consent.reject",
        "maw.consent.trust",
        "maw.consent.untrust",
        "maw.state.set",
    ] {
        let denied = call(
            &host,
            name,
            &json!({"id": "req-1", "pin": "123456", "peer": "nova"}),
        );
        assert_eq!(denied["ok"], false, "{name}: {denied}");
        assert_eq!(denied["code"], "capability_denied", "{name}: {denied}");
    }
}

#[test]
fn consent_read_without_read_capability_is_denied() {
    let dir = temp("consent-cap-deny");
    let host = host(&dir, &[]);
    let denied = call(&host, "maw.consent.read", &json!({"view": "pending"}));
    assert_eq!(denied["ok"], false);
    assert_eq!(denied["code"], "capability_denied");
}

#[test]
fn http_request_rejects_dns_rebind_private_address_without_connecting() {
    let dir = temp("dns-rebind-private");
    let host = host(&dir, &["net:http:rebind.test"])
        .with_http_resolver_override("rebind.test", [IpAddr::V4(Ipv4Addr::LOCALHOST)]);

    let result = call(
        &host,
        "maw.http.request",
        &json!({"method": "GET", "url": "http://rebind.test/metadata"}),
    );
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["code"], "capability_denied", "{result}");
}

#[test]
fn http_request_rejects_rebind_address_set_with_public_then_private() {
    let dir = temp("dns-rebind-flip");
    let host = host(&dir, &["net:http:flip.test"]).with_http_resolver_override(
        "flip.test",
        [
            IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34)),
            IpAddr::V4(Ipv4Addr::new(169, 254, 169, 254)),
        ],
    );

    let result = call(
        &host,
        "maw.http.request",
        &json!({"method": "GET", "url": "http://flip.test/latest/meta-data"}),
    );
    assert_eq!(result["ok"], false, "{result}");
    assert_eq!(result["code"], "capability_denied", "{result}");
}

#[test]
fn http_request_connects_to_pinned_ip_but_preserves_original_host_header() {
    let dir = temp("dns-pin-host-header");
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind pinned server");
    let port = listener.local_addr().expect("pinned addr").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept pinned request");
        let mut buf = [0_u8; 2048];
        let n = stream.read(&mut buf).expect("read pinned request");
        let request = String::from_utf8_lossy(&buf[..n]).to_string();
        tx.send(request).expect("send captured request");
        let body = r#"{"ok":true}"#;
        let response = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream
            .write_all(response.as_bytes())
            .expect("write response");
    });

    let host = host(&dir, &["net:http:vhost.test", "net:private:vhost.test"])
        .with_http_resolver_override("vhost.test", [IpAddr::V4(Ipv4Addr::LOCALHOST)]);
    let result = call(
        &host,
        "maw.http.request",
        &json!({"method": "GET", "url": format!("http://vhost.test:{port}/probe")}),
    );
    assert_eq!(result["ok"], true, "{result}");
    let captured = rx.recv().expect("captured request");
    assert!(
        captured.contains(&format!("host: vhost.test:{port}"))
            || captured.contains(&format!("Host: vhost.test:{port}")),
        "{captured}"
    );
}

#[test]
fn http_request_rejects_loopback_linklocal_unspecified_pinned_ips() {
    let dir = temp("dns-rebind-ranges");
    for (name, ip) in [
        ("loopback.test", IpAddr::V4(Ipv4Addr::LOCALHOST)),
        ("linklocal.test", IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1))),
        ("unspecified.test", IpAddr::V4(Ipv4Addr::UNSPECIFIED)),
        ("v6loop.test", IpAddr::V6(Ipv6Addr::LOCALHOST)),
        (
            "v6link.test",
            IpAddr::V6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1)),
        ),
        ("v6unspec.test", IpAddr::V6(Ipv6Addr::UNSPECIFIED)),
    ] {
        let host =
            host(&dir, &[&format!("net:http:{name}")]).with_http_resolver_override(name, [ip]);
        let result = call(
            &host,
            "maw.http.request",
            &json!({"method": "GET", "url": format!("http://{name}/")}),
        );
        assert_eq!(result["ok"], false, "{name}: {result}");
        assert_eq!(result["code"], "capability_denied", "{name}: {result}");
    }
}

#[test]
fn ipv4_mapped_ipv6_private_hosts_are_denied() {
    let dir = temp("ipv4-mapped");
    let host = host(
        &dir,
        &[
            "net:http:::ffff:127.0.0.1",
            "net:http:::ffff:169.254.169.254",
        ],
    );

    for url in [
        "http://[::ffff:127.0.0.1]/",
        "http://[::ffff:169.254.169.254]/",
    ] {
        let result = call(
            &host,
            "maw.http.request",
            &json!({"method": "GET", "url": url}),
        );
        assert_eq!(result["ok"], false, "{url}");
        assert_eq!(result["code"], "capability_denied", "{url}");
    }
}

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
