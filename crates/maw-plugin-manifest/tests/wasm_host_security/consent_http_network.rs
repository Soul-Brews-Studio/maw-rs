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

