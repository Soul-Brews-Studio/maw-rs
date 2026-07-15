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
            max_response_bytes: None,
        };
        let result = self.run_http_transport(&request);
        self.audit("maw.http.request", &cap, &host, status_of(&result), start);
        result
    }

    fn net_fetch(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<NetFetchArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let endpoint = args.endpoint.clone();
        let method = args.method.as_deref().unwrap_or("GET").to_ascii_uppercase();
        let resource = format!("{endpoint} {method} {}", args.path);
        let cap = match self.caps.require("net", "fetch", Some(&endpoint)) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let mut result = self.net_fetch_checked(args, &method);
        if let HostResult::Ok { value, .. } = &mut result {
            if let Some(response) = value.as_object_mut() {
                let elapsed_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
                response.insert("elapsedMs".to_owned(), Value::from(elapsed_ms));
            }
        }
        self.audit("maw.net.fetch", &cap, &resource, status_of(&result), start);
        result
    }

    fn net_fetch_checked(&self, args: NetFetchArgs, method: &str) -> HostResult<Value> {
        let Some(policy) = self.endpoints.get(&args.endpoint) else {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "endpoint is not allowlisted",
            );
        };
        if !policy.methods.iter().any(|allowed| allowed == method) {
            return HostResult::err(HostErrorCode::CapabilityDenied, "endpoint method denied");
        }
        if !policy
            .paths
            .iter()
            .any(|pattern| pattern.matches(&args.path))
        {
            return HostResult::err(HostErrorCode::CapabilityDenied, "endpoint path denied");
        }
        let url = match self.endpoint_url(policy, &args.path, args.query.as_ref()) {
            Ok(url) => url,
            Err(err) => return err,
        };
        let pinned_addr = if policy.loopback_only {
            match self.loopback_endpoint_addr(&url) {
                Ok(addr) => Some(addr),
                Err(err) => return err,
            }
        } else {
            None
        };
        let headers = match self.net_fetch_headers(policy) {
            Ok(headers) => headers,
            Err(err) => return err,
        };
        let request = TransportHttpRequest {
            method: method.to_owned(),
            url: url.to_string(),
            headers,
            body: args.body,
            timeout_ms: Some(
                args.timeout_ms
                    .unwrap_or(self.http_timeout_ms)
                    .min(MAX_HTTP_TIMEOUT_MS),
            ),
            follow_redirects: false,
            pinned_addr,
            max_response_bytes: Some(MAX_NET_FETCH_RESPONSE_BYTES),
        };
        self.run_http_transport(&request)
    }

    fn net_fetch_headers(
        &self,
        policy: &PluginEndpointPolicy,
    ) -> Result<BTreeMap<String, String>, HostResult<Value>> {
        let Some(auth) = &policy.auth else {
            return Ok(BTreeMap::new());
        };
        self.caps.require("secret", "use", Some(&auth.secret))?;
        let token = self.resolve_net_fetch_secret(&auth.secret)?;
        let (name, value) = match auth.kind.as_str() {
            "bearer" => ("authorization".to_owned(), format!("Bearer {token}")),
            "discord-bot" => ("authorization".to_owned(), format!("Bot {token}")),
            "api-key-header" => {
                let header = auth.header.clone().ok_or_else(|| {
                    HostResult::err(
                        HostErrorCode::InvalidArgs,
                        "api-key-header auth requires configured header",
                    )
                })?;
                (header, token)
            }
            _ => {
                return Err(HostResult::err(
                    HostErrorCode::InvalidArgs,
                    "unsupported endpoint auth kind",
                ));
            }
        };
        Ok(BTreeMap::from([(name, value)]))
    }

    fn resolve_net_fetch_secret(&self, name: &str) -> Result<String, HostResult<Value>> {
        let policy = self.secrets.get(name).ok_or_else(|| {
            HostResult::err(
                HostErrorCode::NotFound,
                format!("secret {name} is not configured"),
            )
        })?;
        if let Some(env) = &policy.env {
            if let Ok(value) = std::env::var(env) {
                if !value.is_empty() {
                    return Ok(value);
                }
            }
        }
        if let Some(pass) = &policy.pass {
            if let Some(value) = resolve_pass_secret(pass) {
                return Ok(value);
            }
        }
        if let Some(value) = self.secret_store.get(name) {
            return Ok(value.clone());
        }
        Err(HostResult::err(
            HostErrorCode::NotFound,
            format!("secret {name} is not available"),
        ))
    }

    fn endpoint_url(
        &self,
        policy: &PluginEndpointPolicy,
        path: &str,
        query: Option<&BTreeMap<String, String>>,
    ) -> Result<Url, HostResult<Value>> {
        let base = self.endpoint_base_url(policy)?;
        let mut url = Url::parse(base.trim_end_matches('/')).map_err(|error| {
            HostResult::err(
                HostErrorCode::InvalidArgs,
                format!("invalid endpoint url: {error}"),
            )
        })?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "endpoint URL must be http/https",
            ));
        }
        url.set_query(None);
        url.set_fragment(None);
        let base_path = url.path().trim_end_matches('/');
        let suffix = path.trim_start_matches('/');
        let full_path = if suffix.is_empty() {
            if base_path.is_empty() { "/" } else { base_path }.to_owned()
        } else if base_path.is_empty() || base_path == "/" {
            format!("/{suffix}")
        } else {
            format!("{base_path}/{suffix}")
        };
        url.set_path(&full_path);
        if let Some(query) = query {
            let mut pairs = url.query_pairs_mut();
            for (key, value) in query {
                pairs.append_pair(key, value);
            }
        }
        if is_discord_gateway(&url) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "Discord gateway is hard-denied from WASM host functions",
            ));
        }
        Ok(url)
    }

    fn endpoint_base_url(
        &self,
        policy: &PluginEndpointPolicy,
    ) -> Result<String, HostResult<Value>> {
        if let Some(base) = &policy.base_url {
            return Ok(base.clone());
        }
        if let Some(key) = &policy.base_url_ref {
            let path = self.config_file_path()?;
            let config = read_config_json(&path)?;
            if let Some(base) = get_json_path(&config, key)
                .and_then(Value::as_str)
                .filter(|base| !base.is_empty())
            {
                return Ok(base.to_owned());
            }
        }
        policy
            .default_base_url
            .clone()
            .ok_or_else(|| HostResult::err(HostErrorCode::NotFound, "endpoint base URL not found"))
    }

    fn loopback_endpoint_addr(&self, url: &Url) -> Result<SocketAddr, HostResult<Value>> {
        let host = url.host_str().ok_or_else(|| {
            HostResult::err(HostErrorCode::InvalidArgs, "endpoint URL host is required")
        })?;
        let Some(port) = url.port_or_known_default() else {
            return Err(HostResult::err(
                HostErrorCode::InvalidArgs,
                "endpoint URL port is required",
            ));
        };
        let addrs = self
            .resolve_http_host_once(host, port)
            .map_err(|error| HostResult::err(HostErrorCode::NetworkError, error))?;
        if addrs.is_empty() || addrs.iter().any(|ip| !ip.to_canonical().is_loopback()) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "loopbackOnly endpoint must resolve to loopback",
            ));
        }
        Ok(SocketAddr::new(addrs[0], port))
    }

    fn run_http_transport(&self, request: &TransportHttpRequest) -> HostResult<Value> {
        let request = request.clone();
        let timeout_ms = request.timeout_ms.unwrap_or(self.http_timeout_ms);
        match std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|error| format!("tokio runtime failed: {error}"))?;
            let client = ReqwestHttpTransportIo::new(timeout_ms)?;
            runtime.block_on(client.request(&request))
        })
        .join()
        {
            Ok(Ok(resp)) => HostResult::ok(
                json!({"status": resp.status, "headers": resp.headers, "body": resp.body, "url": resp.url}),
            ),
            Ok(Err(error)) => HostResult::err(HostErrorCode::NetworkError, error),
            Err(_) => HostResult::err(HostErrorCode::NetworkError, "HTTP worker thread panicked"),
        }
    }

    fn resolve_http_pinned_addr(
        &self,
        url: &Url,
        host: &str,
    ) -> Result<SocketAddr, HostResult<Value>> {
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
            .ok_or_else(|| {
                HostResult::err(HostErrorCode::NetworkError, "host resolved no addresses")
            })
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
            max_response_bytes: None,
        };
        let result = self.run_http_transport(&request);
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

fn resolve_pass_secret(path: &str) -> Option<String> {
    let output = Command::new("pass").arg("show").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .and_then(|stdout| stdout.lines().next().map(str::to_owned))
        .filter(|value| !value.is_empty())
}
