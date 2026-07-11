fn serve_net_once(response: String, delay: u64) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind net listener");
    let port = listener.local_addr().expect("listener addr").port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept net request");
        let mut buf = [0_u8; 4096];
        let n = stream.read(&mut buf).unwrap_or(0);
        tx.send(String::from_utf8_lossy(&buf[..n]).to_string()).ok();
        if delay > 0 {
            thread::sleep(std::time::Duration::from_millis(delay));
        }
        let _ = stream.write_all(response.as_bytes());
    });
    (format!("http://127.0.0.1:{port}"), rx)
}

fn raw_response(status: &str, headers: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\ncontent-length: {}\r\nconnection: close\r\n{headers}\r\n{body}",
        body.len()
    )
}

fn has_header(request: &str, name: &str, value: &str) -> bool {
    request.lines().any(|line| {
        line.split_once(':').is_some_and(|(actual, actual_value)| {
            actual.eq_ignore_ascii_case(name) && actual_value.trim() == value
        })
    })
}

#[test]
fn net_fetch_granted_endpoint_uses_policy_and_audits_hostless_path() {
    let dir = temp("net-fetch-ok");
    let (base, rx) = serve_net_once(raw_response("200 OK", "", "ok"), 0);
    let host = endpoint_host(
        &dir,
        &["net:fetch:local-api"],
        json!({"local-api":{"baseUrl":format!("{base}/api"),"paths":["/sessions/*"]}}),
    );
    let result = call(
        &host,
        "maw.net.fetch",
        &json!({"endpoint":"local-api","path":"/sessions/abc","query":{"limit":"1"}}),
    );
    assert_eq!(result["value"]["status"], 200, "{result}");
    assert!(result["value"]["elapsedMs"].as_u64().is_some(), "{result}");
    assert!(rx
        .recv()
        .expect("captured")
        .starts_with("GET /api/sessions/abc?limit=1 "));
    let audit = host.audit_json_lines();
    assert!(
        audit.contains("\"host_fn\":\"maw.net.fetch\"")
            && audit.contains("local-api GET /sessions/abc"),
        "{audit}"
    );
    assert!(!audit.to_lowercase().contains("authorization"), "{audit}");
}

#[test]
fn net_fetch_secret_use_injects_bound_auth_without_guest_or_audit_leak() {
    let dir = temp("net-fetch-secret-auth");
    let (bearer_base, bearer_rx) = serve_net_once(raw_response("200 OK", "", "ok"), 0);
    let (discord_base, discord_rx) = serve_net_once(raw_response("200 OK", "", "ok"), 0);
    let (api_base, api_rx) = serve_net_once(raw_response("200 OK", "", "ok"), 0);
    let host = endpoint_secret_host(
        &dir,
        &[
            "net:fetch:bearer",
            "net:fetch:discord",
            "net:fetch:api",
            "secret:use:bearer-token",
            "secret:use:discord-token",
            "secret:use:api-token",
        ],
        json!({
            "bearer":{"baseUrl":bearer_base,"paths":["/ok"],"auth":{"kind":"bearer","secret":"bearer-token"}},
            "discord":{"baseUrl":discord_base,"paths":["/ok"],"auth":{"kind":"discord-bot","secret":"discord-token"}},
            "api":{"baseUrl":api_base,"paths":["/ok"],"auth":{"kind":"api-key-header","secret":"api-token","header":"x-api-key"}}
        }),
        Some(json!({
            "bearer-token":{"env":"MAW_UNUSED_BEARER_TOKEN"},
            "discord-token":{"env":"MAW_UNUSED_DISCORD_TOKEN"},
            "api-token":{"env":"MAW_UNUSED_API_TOKEN"}
        })),
    )
    .with_secret_ref("bearer-token", "BEARER_SECRET_244")
    .with_secret_ref("discord-token", "DISCORD_SECRET_244")
    .with_secret_ref("api-token", "API_SECRET_244");
    for (endpoint, rx, header, value, token) in [
        (
            "bearer",
            bearer_rx,
            "authorization",
            "Bearer BEARER_SECRET_244",
            "BEARER_SECRET_244",
        ),
        (
            "discord",
            discord_rx,
            "authorization",
            "Bot DISCORD_SECRET_244",
            "DISCORD_SECRET_244",
        ),
        (
            "api",
            api_rx,
            "x-api-key",
            "API_SECRET_244",
            "API_SECRET_244",
        ),
    ] {
        let result = call(
            &host,
            "maw.net.fetch",
            &json!({"endpoint":endpoint,"path":"/ok"}),
        );
        assert_eq!(result["value"]["status"], 200, "{result}");
        assert!(has_header(&rx.recv().expect("request"), header, value));
        assert!(!result.to_string().contains(token), "{result}");
    }
    let audit = host.audit_json_lines();
    for token in ["BEARER_SECRET_244", "DISCORD_SECRET_244", "API_SECRET_244"] {
        assert!(!audit.contains(token), "{audit}");
    }
}

#[test]
fn net_fetch_secret_use_missing_or_network_errors_do_not_leak_secret_material() {
    let dir = temp("net-fetch-secret-errors");
    let missing_path = format!("maw-rs-tests/missing-secret-{}", std::process::id());
    let missing = endpoint_secret_host(
        &dir,
        &["net:fetch:api", "secret:use:missing-token"],
        json!({"api":{"baseUrl":"http://127.0.0.1:9","paths":["/ok"],"auth":{"kind":"bearer","secret":"missing-token"}}}),
        Some(json!({"missing-token":{"pass":missing_path}})),
    );
    let missing_result = call(
        &missing,
        "maw.net.fetch",
        &json!({"endpoint":"api","path":"/ok"}),
    );
    assert_eq!(missing_result["code"], "not_found", "{missing_result}");
    assert!(missing_result.to_string().contains("missing-token"));
    assert!(!missing_result.to_string().contains("maw-rs-tests"));

    let (base, _) = serve_net_once(String::new(), 0);
    let host = endpoint_secret_host(
        &dir,
        &["net:fetch:api", "secret:use:leaky-token"],
        json!({"api":{"baseUrl":base,"paths":["/fail"],"auth":{"kind":"bearer","secret":"leaky-token"}}}),
        Some(json!({"leaky-token":{"env":"MAW_UNUSED_LEAKY_TOKEN"}})),
    )
    .with_secret_ref("leaky-token", "LEAKY_SECRET_244");
    let network_result = call(
        &host,
        "maw.net.fetch",
        &json!({"endpoint":"api","path":"/fail"}),
    );
    assert_eq!(network_result["code"], "network_error", "{network_result}");
    assert!(!network_result.to_string().contains("LEAKY_SECRET_244"));
    assert!(!host.audit_json_lines().contains("LEAKY_SECRET_244"));
}

#[test]
fn net_fetch_secret_use_grant_does_not_inject_on_unbound_endpoint() {
    let dir = temp("net-fetch-secret-unbound-call");
    let (plain_base, plain_rx) = serve_net_once(raw_response("200 OK", "", "ok"), 0);
    let host = endpoint_secret_host(
        &dir,
        &["net:fetch:plain", "secret:use:unused-token"],
        json!({
            "plain":{"baseUrl":plain_base,"paths":["/ok"]},
            "auth-only":{"baseUrl":"http://127.0.0.1:9","paths":["/ok"],"auth":{"kind":"bearer","secret":"unused-token"}}
        }),
        Some(json!({"unused-token":{"env":"MAW_UNUSED_TOKEN"}})),
    )
    .with_secret_ref("unused-token", "UNUSED_SECRET_244");
    let result = call(
        &host,
        "maw.net.fetch",
        &json!({"endpoint":"plain","path":"/ok"}),
    );
    assert_eq!(result["value"]["status"], 200, "{result}");
    let request = plain_rx.recv().expect("request");
    assert!(!request.contains("UNUSED_SECRET_244"), "{request}");
    assert!(
        !request.to_ascii_lowercase().contains("authorization:"),
        "{request}"
    );
}

#[test]
fn net_fetch_secret_policy_manifest_validation() {
    let dir = temp("net-fetch-secret-manifest");
    write(dir.join("plugin.wasm"), b"\0asm\x01\0\0\0").expect("wasm");
    let missing_secret = json!({
        "name":"secure-plugin","version":"1.0.0","sdk":"*",
        "entry":{"kind":"wasm","path":"plugin.wasm","export":"handle"},
        "capabilities":["net:fetch:api"],
        "endpoints":{"api":{"baseUrl":"http://127.0.0.1:9","paths":["/ok"],"auth":{"kind":"bearer","secret":"missing-token"}}}
    });
    let err = parse_manifest(&missing_secret.to_string(), &dir).expect_err("missing secret");
    assert!(err.contains("auth references missing secret"), "{err}");

    let unbound_secret_cap = json!({
        "name":"secure-plugin","version":"1.0.0","sdk":"*",
        "entry":{"kind":"wasm","path":"plugin.wasm","export":"handle"},
        "capabilities":["secret:use:unused-token"],
        "secrets":{"unused-token":{"env":"MAW_UNUSED_TOKEN"}},
        "endpoints":{"api":{"baseUrl":"http://127.0.0.1:9","paths":["/ok"]}}
    });
    let err = parse_manifest(&unbound_secret_cap.to_string(), &dir).expect_err("unbound secret");
    assert!(err.contains("unbound secret"), "{err}");
}

#[test]
fn net_fetch_denies_ungranted_method_and_paths_before_network() {
    let dir = temp("net-fetch-deny");
    let endpoints = json!({"local-api":{"baseUrl":"http://127.0.0.1:9","methods":["GET"],"paths":["/ok"]},"wss":{"baseUrl":"wss://gateway.discord.gg","paths":["/"]}});
    let ungranted = endpoint_host(&dir, &[], endpoints.clone());
    let granted = endpoint_host(&dir, &["net:fetch:local-api", "net:fetch:wss"], endpoints);
    for (host, args) in [
        (&ungranted, json!({"endpoint":"local-api","path":"/ok"})),
        (
            &granted,
            json!({"endpoint":"local-api","method":"POST","path":"/ok"}),
        ),
        (&granted, json!({"endpoint":"local-api","path":"/ok/extra"})),
        (
            &granted,
            json!({"endpoint":"local-api","path":"/https://evil.test/x"}),
        ),
        (&granted, json!({"endpoint":"wss","path":"/"})),
    ] {
        let result = call(host, "maw.net.fetch", &args);
        assert_eq!(result["code"], "capability_denied", "{args}: {result}");
    }
}

#[test]
fn net_fetch_refuses_redirects_loopback_violations_and_caps() {
    let dir = temp("net-fetch-security");
    let (redir, _) = serve_net_once(
        raw_response(
            "302 Found",
            "location: http://127.0.0.1:9/final\r\n",
            "redir",
        ),
        0,
    );
    let (slow, _) = serve_net_once(raw_response("200 OK", "", "slow"), 200);
    let (big, _) = serve_net_once(raw_response("200 OK", "", &"x".repeat(1_048_577)), 0);
    let host = endpoint_host(
        &dir,
        &[
            "net:fetch:redir",
            "net:fetch:loop",
            "net:fetch:slow",
            "net:fetch:big",
        ],
        json!({
            "redir":{"baseUrl":redir,"paths":["/go"]},
            "loop":{"baseUrl":"http://public.test:1234","loopbackOnly":true,"paths":["/health"]},
            "slow":{"baseUrl":slow,"paths":["/slow"]},
            "big":{"baseUrl":big,"paths":["/big"]}
        }),
    )
    .with_http_resolver_override("public.test", [IpAddr::V4(Ipv4Addr::new(93, 184, 216, 34))]);
    assert_eq!(
        call(
            &host,
            "maw.net.fetch",
            &json!({"endpoint":"redir","path":"/go"})
        )["value"]["status"],
        302
    );
    assert_eq!(
        call(
            &host,
            "maw.net.fetch",
            &json!({"endpoint":"loop","path":"/health"})
        )["code"],
        "capability_denied"
    );
    for args in [
        json!({"endpoint":"slow","path":"/slow","timeoutMs":1}),
        json!({"endpoint":"big","path":"/big"}),
    ] {
        assert_eq!(
            call(&host, "maw.net.fetch", &args)["code"],
            "network_error",
            "{args}"
        );
    }
}
