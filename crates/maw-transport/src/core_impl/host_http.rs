use std::{collections::BTreeMap, net::SocketAddr};

/// Generic bounded HTTP request used by the WASM host-function layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: BTreeMap<String, String>,
    pub body: Option<String>,
    pub timeout_ms: Option<u64>,
    pub follow_redirects: bool,
    pub pinned_addr: Option<SocketAddr>,
}

/// Generic HTTP response returned to WASM plugins.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
    pub url: String,
}

impl ReqwestHttpTransportIo {
    /// Execute a generic HTTP request using the existing reqwest/rustls transport client.
    ///
    /// # Errors
    ///
    /// Returns request-construction, network, or body-read errors as strings.
    pub async fn request(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        let method = reqwest::Method::from_bytes(request.method.as_bytes())
            .map_err(|error| format!("invalid HTTP method: {error}"))?;
        let timeout = request.timeout_ms.unwrap_or(self.timeout_ms());
        let client = if request.pinned_addr.is_some() || !request.follow_redirects {
            let mut builder = reqwest::Client::builder().timeout(std::time::Duration::from_millis(timeout));
            if !request.follow_redirects {
                builder = builder.redirect(reqwest::redirect::Policy::none());
            }
            if let Some(addr) = request.pinned_addr {
                let host = url_host(&request.url)?;
                builder = builder.resolve(&host, addr);
            }
            builder
                .build()
                .map_err(|error| format!("http client build failed: {error}"))?
        } else {
            self.client.clone()
        };
        let mut builder = client
            .request(method, &request.url)
            .timeout(std::time::Duration::from_millis(timeout));
        for (name, value) in &request.headers {
            builder = builder.header(name, value);
        }
        if let Some(body) = &request.body {
            builder = builder.body(body.clone());
        }
        let response = builder
            .send()
            .await
            .map_err(|error| format!("network error requesting {}: {error}", request.url))?;
        let status = response.status().as_u16();
        let url = response.url().to_string();
        let headers = response
            .headers()
            .iter()
            .filter_map(|(name, value)| {
                value
                    .to_str()
                    .ok()
                    .map(|value| (name.as_str().to_owned(), value.to_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        let body = response
            .text()
            .await
            .map_err(|error| format!("network error reading {}: {error}", request.url))?;
        Ok(HttpResponse {
            status,
            headers,
            body,
            url,
        })
    }
}

fn url_host(url: &str) -> Result<String, String> {
    let parsed = reqwest::Url::parse(url).map_err(|error| format!("invalid url: {error}"))?;
    parsed
        .host_str()
        .map(str::to_owned)
        .ok_or_else(|| "url host is required".to_owned())
}
