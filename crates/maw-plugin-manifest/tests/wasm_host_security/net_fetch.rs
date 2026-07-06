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
fn net_fetch_denies_ungranted_method_and_paths_before_network() {
    let dir = temp("net-fetch-deny");
    let endpoints =
        json!({"local-api":{"baseUrl":"http://127.0.0.1:9","methods":["GET"],"paths":["/ok"]},"wss":{"baseUrl":"wss://gateway.discord.gg","paths":["/"]}});
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
