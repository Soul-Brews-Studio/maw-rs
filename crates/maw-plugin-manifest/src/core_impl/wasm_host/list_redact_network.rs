fn file_kind(file_type: std::fs::FileType) -> &'static str {
    if file_type.is_dir() {
        "dir"
    } else if file_type.is_symlink() {
        "symlink"
    } else {
        "file"
    }
}

fn list_dir(path: &Path, recursive: bool, include_dirs: bool, max: usize, out: &mut Vec<Value>) {
    if out.len() >= max {
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= max {
            break;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let kind = file_kind(meta.file_type());
        if include_dirs || kind != "dir" {
            out.push(json!({
                "path": entry.path().display().to_string(),
                "kind": kind,
                "bytes": meta.len()
            }));
        }
        if recursive && kind == "dir" {
            list_dir(&entry.path(), true, include_dirs, max, out);
        }
    }
}
fn redact_headers(headers: BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .map(|(key, value)| {
            let lower = key.to_lowercase();
            if [
                "authorization",
                "token",
                "secret",
                "peerkey",
                "cookie",
                "api-key",
                "x-api-key",
                "bearer",
            ]
            .iter()
            .any(|marker| lower.contains(marker))
            {
                (key, "[REDACTED]".to_owned())
            } else {
                (key, value)
            }
        })
        .collect()
}
fn redact(value: &str) -> String {
    let mut out = value.to_owned();
    for marker in ["peerKey", "token", "secret", "authorization"] {
        if out.to_lowercase().contains(&marker.to_lowercase()) {
            "[REDACTED]".clone_into(&mut out);
        }
    }
    out
}

fn json_u16(value: &Value) -> Option<u16> {
    if let Some(port) = value.as_u64() {
        return u16::try_from(port).ok();
    }
    value.as_str()?.parse::<u16>().ok()
}

fn parse_localserver_base_url(raw: &str) -> Result<Url, HostResult<Value>> {
    let mut url = Url::parse(raw.trim_end_matches('/')).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("invalid localserver url: {error}"),
        )
    })?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "localserver URL must be http/https",
        ));
    }
    let host = url.host_str().ok_or_else(|| {
        HostResult::err(HostErrorCode::InvalidArgs, "localserver URL host is required")
    })?;
    if !is_localserver_host(host) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "localserver URL must resolve to loopback",
        ));
    }
    url.set_path("");
    url.set_query(None);
    url.set_fragment(None);
    Ok(url)
}

fn pinned_localserver_url(
    base: &Url,
    path: Option<&str>,
    requested_url: Option<&str>,
) -> Result<Url, HostResult<Value>> {
    let url = if let Some(raw) = requested_url {
        Url::parse(raw).map_err(|error| {
            HostResult::err(HostErrorCode::InvalidArgs, format!("invalid url: {error}"))
        })?
    } else {
        let path = path.unwrap_or("/");
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "localserver path must be absolute and hostless",
            ));
        }
        base.join(path.trim_start_matches('/')).map_err(|error| {
            HostResult::err(HostErrorCode::InvalidArgs, format!("invalid path: {error}"))
        })?
    };
    if !same_origin(base, &url) {
        return Err(HostResult::err(
            HostErrorCode::CapabilityDenied,
            "localserver request denied: URL is not the host-pinned maw server endpoint",
        ));
    }
    Ok(url)
}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

fn is_localserver_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<IpAddr>()
        .is_ok_and(|ip| ip.to_canonical().is_loopback())
}

fn is_discord_gateway(url: &Url) -> bool {
    url.host_str()
        .is_some_and(|host| host.contains("discord") && url.path().contains("gateway"))
}
fn private_host_name(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.to_lowercase().ends_with(".local")
}

fn private_ip(ip: IpAddr) -> bool {
    match ip.to_canonical() {
        IpAddr::V4(ip) => {
            ip.is_private() || ip.is_loopback() || ip.is_link_local() || ip.is_unspecified()
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
                || ip.is_unspecified()
        }
    }
}

fn tmux_sessions_json(sessions: Vec<maw_tmux::TmuxSession>) -> Vec<Value> {
    sessions
        .into_iter()
        .map(|session| {
            json!({
                "name": session.name,
                "windows": session.windows.into_iter().map(|window| json!({
                    "index": window.index,
                    "name": window.name,
                    "active": window.active,
                    "cwd": window.cwd,
                })).collect::<Vec<_>>()
            })
        })
        .collect()
}

