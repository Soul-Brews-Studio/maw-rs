impl MawWasmHost {
    fn http_request(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<HttpArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let url = match Url::parse(&args.url) {
            Ok(url) => url,
            Err(error) => {
                return HostResult::err(HostErrorCode::InvalidArgs, format!("invalid url: {error}"))
            }
        };
        if is_discord_gateway(&url) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "Discord gateway is hard-denied from WASM host functions",
            );
        }
        let scheme = url.scheme();
        if !matches!(scheme, "http" | "https") {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "only http/https URLs are supported",
            );
        }
        let host = match url.host_str() {
            Some(host) => host.to_owned(),
            None => return HostResult::err(HostErrorCode::InvalidArgs, "url host is required"),
        };
        let pinned_addr = match self.resolve_http_pinned_addr(&url, &host) {
            Ok(addr) => addr,
            Err(err) => return err,
        };
        if private_ip(pinned_addr.ip()) && !self.caps.contains("net", "private", Some(&host)) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "private network access denied",
            );
        }
        let cap = match self.caps.require("net", scheme, Some(&host)) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let headers = redact_headers(args.headers.unwrap_or_default());
        let request = TransportHttpRequest {
            method: args.method,
            url: args.url,
            headers,
            body: args.body,
            timeout_ms: Some(
                args.timeout_ms
                    .unwrap_or(self.http_timeout_ms)
                    .min(MAX_HTTP_TIMEOUT_MS),
            ),
            follow_redirects: args.follow_redirects.unwrap_or(false),
            pinned_addr: Some(pinned_addr),
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(error) => {
                return HostResult::err(
                    HostErrorCode::NetworkError,
                    format!("tokio runtime failed: {error}"),
                )
            }
        };
        let client =
            match ReqwestHttpTransportIo::new(request.timeout_ms.unwrap_or(self.http_timeout_ms)) {
                Ok(client) => client,
                Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
            };
        let result = match runtime.block_on(client.request(&request)) {
            Ok(resp) => HostResult::ok(
                json!({"status": resp.status, "headers": resp.headers, "body": resp.body, "url": resp.url}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        };
        self.audit("maw.http.request", &cap, &host, status_of(&result), start);
        result
    }


    fn resolve_http_pinned_addr(&self, url: &Url, host: &str) -> Result<SocketAddr, HostResult<Value>> {
        let Some(port) = url.port_or_known_default() else {
            return Err(HostResult::err(
                HostErrorCode::InvalidArgs,
                "url port is required",
            ));
        };
        if private_host_name(host) && !self.caps.contains("net", "private", Some(host)) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "private network access denied",
            ));
        }
        let addrs = match self.resolve_http_host_once(host, port) {
            Ok(addrs) => addrs,
            Err(error) => return Err(HostResult::err(HostErrorCode::NetworkError, error)),
        };
        if addrs.iter().any(|addr| private_ip(*addr))
            && !self.caps.contains("net", "private", Some(host))
        {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "private network access denied",
            ));
        }
        addrs
            .first()
            .map(|ip| SocketAddr::new(*ip, port))
            .ok_or_else(|| HostResult::err(HostErrorCode::NetworkError, "host resolved no addresses"))
    }

    fn resolve_http_host_once(&self, host: &str, port: u16) -> Result<Vec<IpAddr>, String> {
        if let Some(addrs) = self.http_resolver_overrides.get(host) {
            return Ok(addrs.clone());
        }
        let literal_host = host.trim_start_matches('[').trim_end_matches(']');
        if let Ok(ip) = literal_host.parse::<IpAddr>() {
            return Ok(vec![ip]);
        }
        (host, port)
            .to_socket_addrs()
            .map(|addrs| addrs.map(|addr| addr.ip()).collect::<Vec<_>>())
            .map_err(|error| format!("failed to resolve {host}: {error}"))
    }


    fn localserver_request(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<LocalserverArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self.caps.require("sdk", "localserver", None) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let base = match self.resolve_localserver_url() {
            Ok(base) => base,
            Err(err) => return err,
        };
        let url = match pinned_localserver_url(&base, args.path.as_deref(), args.url.as_deref()) {
            Ok(url) => url,
            Err(err) => return err,
        };
        let headers = redact_headers(args.headers.unwrap_or_default());
        let request = TransportHttpRequest {
            method: args.method,
            url: url.to_string(),
            headers,
            body: args.body,
            timeout_ms: Some(
                args.timeout_ms
                    .unwrap_or(self.http_timeout_ms)
                    .min(MAX_HTTP_TIMEOUT_MS),
            ),
            follow_redirects: false,
            pinned_addr: None,
        };
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(error) => {
                return HostResult::err(
                    HostErrorCode::NetworkError,
                    format!("tokio runtime failed: {error}"),
                )
            }
        };
        let client = match ReqwestHttpTransportIo::new(request.timeout_ms.unwrap_or(self.http_timeout_ms)) {
            Ok(client) => client,
            Err(error) => return HostResult::err(HostErrorCode::NetworkError, error),
        };
        let result = match runtime.block_on(client.request(&request)) {
            Ok(resp) => HostResult::ok(
                json!({"status": resp.status, "headers": resp.headers, "body": resp.body, "url": resp.url}),
            ),
            Err(error) => HostResult::err(HostErrorCode::NetworkError, error),
        };
        self.audit(
            "maw.localserver.request",
            &cap,
            base.as_str(),
            status_of(&result),
            start,
        );
        result
    }

}
