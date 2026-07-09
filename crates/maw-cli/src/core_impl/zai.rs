const DISPATCH_327: &[DispatcherEntry] = &[DispatcherEntry { command: "zai", handler: Handler::Async(zai_run_async) }];

const ZAI_USAGE: &str = "usage: maw zai <status|mon|test> [--fleet <group>]\n  status  show configured Z.AI token pool (redacted)\n  mon     monitor snapshot (status + next action)\n  test    probe each configured key with a tiny chat completion\n  --fleet <group>  read tokenPool.<group> first, falling back to tokenPool.zai\n                   (default scope comes from $MAW_ZAI_POOL when set)\n";

const ZAI_FLEET_TOKEN_USAGE: &str = "usage: maw fleet token <group> [ls|status]";

#[derive(Clone)]
struct ZaiKey { index: usize, label: String, source: String, base_url: String, token: Option<String>, status: String, error: String }

fn zai_run_async(args: Vec<String>) -> std::pin::Pin<Box<dyn std::future::Future<Output = CliOutput> + Send>> {
    Box::pin(async move { zai_dispatch(&args).await })
}

async fn zai_dispatch(args: &[String]) -> CliOutput {
    let (fleet, rest) = match zai_split_fleet(args) { Ok(parts) => parts, Err(e) => return zai_err(&e) };
    let fleet = zai_fleet_scope(fleet);
    match rest.first().map_or("status", String::as_str) {
        "help" | "--help" | "-h" => zai_ok(ZAI_USAGE.to_owned()),
        "status" | "ls" | "list" => match zai_load_pool(fleet.as_deref()) { Ok((_, keys)) => zai_ok(zai_format_status(&keys)), Err(e) => zai_err(&e) },
        "mon" => match zai_load_pool(fleet.as_deref()) { Ok((_, keys)) => zai_ok(zai_format_mon(&keys)), Err(e) => zai_err(&e) },
        "test" => match zai_test_pool().await { Ok(out) => out, Err(e) => zai_err(&e) },
        _ => zai_err(ZAI_USAGE),
    }
}

/// `maw fleet token <group> [ls|status]` — thin alias over fleet-scoped zai status.
fn zai_fleet_token(args: &[String]) -> CliOutput {
    let Some(group) = args.first().filter(|value| !value.starts_with('-')) else { return zai_err(ZAI_FLEET_TOKEN_USAGE); };
    if !zai_safe_group(group) { return zai_err(&format!("fleet token: invalid group name {group}")); }
    match args.get(1).map_or("status", String::as_str) {
        "status" | "ls" | "list" => match zai_load_pool(Some(group)) { Ok((_, keys)) => zai_ok(zai_format_status(&keys)), Err(e) => zai_err(&e) },
        _ => zai_err(ZAI_FLEET_TOKEN_USAGE),
    }
}

fn zai_split_fleet(args: &[String]) -> Result<(Option<String>, Vec<String>), String> {
    let mut fleet = None;
    let mut rest = Vec::new();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--fleet" {
            let value = iter.next().ok_or_else(|| "zai: --fleet needs a group name".to_owned())?;
            if !zai_safe_group(value) { return Err(format!("zai: invalid fleet squad name {value}")); }
            fleet = Some(value.clone());
        } else {
            rest.push(arg.clone());
        }
    }
    Ok((fleet, rest))
}

/// Scope precedence: explicit `--fleet` flag → `$MAW_ZAI_POOL` (spawn wiring) → unscoped.
fn zai_fleet_scope(cli: Option<String>) -> Option<String> {
    cli.or_else(|| std::env::var("MAW_ZAI_POOL").ok()).filter(|group| zai_safe_group(group))
}

fn zai_safe_group(group: &str) -> bool {
    !group.is_empty() && group.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
}

/// Pure lookup order: fleet pool (when a group is known) → global zai pool → hermes legacy pool.
fn zai_pool_pointers(fleet: Option<&str>) -> Vec<String> {
    let mut pointers = Vec::new();
    if let Some(group) = fleet.filter(|group| *group != "zai") { pointers.push(format!("/tokenPool/{group}")); }
    pointers.push("/tokenPool/zai".to_owned());
    pointers.push("/credential_pool/zai".to_owned());
    pointers
}

fn zai_ok(stdout: String) -> CliOutput { CliOutput { code: 0, stdout, stderr: String::new() } }
fn zai_err(message: &str) -> CliOutput { CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") } }

fn zai_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
}

fn zai_auth_paths() -> Vec<std::path::PathBuf> {
    if let Some(path) = std::env::var_os("MAW_ZAI_AUTH_JSON").map(std::path::PathBuf::from) { return vec![path]; }
    let home = zai_home();
    vec![home.join(".config/maw/maw.config.json"), home.join(".hermes/auth.json")]
}

fn zai_load_pool(fleet: Option<&str>) -> Result<(std::path::PathBuf, Vec<ZaiKey>), String> {
    let mut last = String::new();
    for path in zai_auth_paths() {
        match zai_load_pool_at(&path, fleet) { Ok(keys) => return Ok((path, keys)), Err(e) => last = e }
    }
    Err(if last.is_empty() { "zai: no token pool found".to_owned() } else { last })
}

fn zai_load_pool_at(path: &std::path::Path, fleet: Option<&str>) -> Result<Vec<ZaiKey>, String> {
    let raw = std::fs::read_to_string(path).map_err(|_| format!("zai: no token pool at {}", path.display()))?;
    let json: serde_json::Value = serde_json::from_str(&raw).map_err(|_| format!("zai: invalid json at {}", path.display()))?;
    let pointers = zai_pool_pointers(fleet);
    let pool = pointers.iter().find_map(|pointer| json.pointer(pointer).and_then(serde_json::Value::as_array)).ok_or_else(|| format!("zai: missing tokenPool.zai or credential_pool.zai in {}", path.display()))?;
    let mut keys = Vec::new();
    for (i, entry) in pool.iter().enumerate() {
        let label = entry.get("label").and_then(serde_json::Value::as_str).unwrap_or("key").to_owned();
        let source = entry.get("source").and_then(serde_json::Value::as_str).unwrap_or("manual").to_owned();
        let base_url = entry.get("base_url").and_then(serde_json::Value::as_str).unwrap_or("https://api.z.ai/api/coding/paas/v4").trim_end_matches('/').to_owned();
        let token = zai_resolve_token(entry, &source);
        let status = entry.get("last_status").and_then(serde_json::Value::as_str).unwrap_or("unknown").to_owned();
        let error = entry.get("last_error_message").and_then(serde_json::Value::as_str).unwrap_or("").to_owned();
        keys.push(ZaiKey { index: i + 1, label, source, base_url, token, status, error });
    }
    if keys.is_empty() { return Err("zai: credential_pool.zai is empty".to_owned()); }
    Ok(keys)
}

fn zai_resolve_token(entry: &serde_json::Value, source: &str) -> Option<String> {
    if let Some(name) = source.strip_prefix("env:") { return std::env::var(name).ok().filter(|v| !v.trim().is_empty()); }
    entry.get("access_token").and_then(serde_json::Value::as_str).map(str::trim).filter(|v| !v.is_empty()).map(str::to_owned)
}

fn zai_format_status(keys: &[ZaiKey]) -> String {
    let mut out = format!("zai token pool: {} key(s)\n", keys.len());
    for key in keys {
        let ready = if key.token.is_some() { "ready" } else { "missing-secret" };
        let err = if key.error.is_empty() { String::new() } else { format!(" — {}", zai_short(&key.error, 80)) };
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("  {}. {} [{}] {} last={}{}\n", key.index, key.label, key.source, ready, key.status, err));
    }
    out
}

fn zai_format_mon(keys: &[ZaiKey]) -> String {
    let ready = keys.iter().filter(|k| k.token.is_some()).count();
    let exhausted = keys.iter().filter(|k| k.status == "exhausted").count();
    let mut out = zai_format_status(keys);
    let action = if ready == 0 { "set GLM_API_KEY or add manual access_token to ~/.hermes/auth.json" } else if exhausted == ready { "all ready keys look exhausted; wait/reset or add another key" } else { "run `maw zai test` before dispatching a large fleet" };
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\nmonitor: {ready}/{} ready, {exhausted} exhausted · next: {action}\n", keys.len()));
    out
}

async fn zai_test_pool() -> Result<CliOutput, String> {
    let (path, keys) = zai_load_pool(None)?;
    let client = reqwest::Client::builder().timeout(std::time::Duration::from_secs(20)).build().map_err(|e| format!("zai: http client: {e}"))?;
    let mut ok = 0usize;
    let mut out = String::from("zai test:\n");
    let mut results = Vec::new();
    for key in &keys {
        let result = zai_probe_key(&client, key).await;
        if result.0 { ok += 1; }
        let mark = if result.0 { "ok" } else { "fail" };
        let _ = std::fmt::Write::write_fmt(&mut out, format_args!("  {}. {} {mark} {}\n", key.index, key.label, result.1));
        results.push(result);
    }
    let _ = zai_update_statuses(&path, &results);
    let _ = std::fmt::Write::write_fmt(&mut out, format_args!("\nsummary: {ok}/{} keys ok\n", keys.len()));
    Ok(CliOutput { code: i32::from(ok == 0), stdout: out, stderr: String::new() })
}

async fn zai_probe_key(client: &reqwest::Client, key: &ZaiKey) -> (bool, String) {
    let Some(token) = key.token.as_deref() else { return (false, "missing secret".to_owned()); };
    let url = format!("{}/chat/completions", key.base_url);
    let started = std::time::Instant::now();
    let body = serde_json::json!({"model":"glm-4.5-flash","messages":[{"role":"user","content":"reply ok"}],"max_tokens":4,"temperature":0});
    match client.post(url).bearer_auth(token).json(&body).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            if resp.status().is_success() { (true, format!("{}ms", started.elapsed().as_millis())) }
            else { (false, format!("http {code}: {}", zai_short(&resp.text().await.unwrap_or_default(), 120))) }
        }
        Err(e) => (false, zai_short(&e.to_string(), 120)),
    }
}

fn zai_update_statuses(path: &std::path::Path, results: &[(bool, String)]) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|_| "read failed".to_owned())?;
    let mut json: serde_json::Value = serde_json::from_str(&raw).map_err(|_| "parse failed".to_owned())?;
    let Some(pool) = json.pointer_mut("/credential_pool/zai").and_then(serde_json::Value::as_array_mut) else { return Ok(()); };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |d| d.as_secs_f64());
    for (entry, (ok, msg)) in pool.iter_mut().zip(results) {
        entry["last_status"] = serde_json::json!(if *ok { "ok" } else { "exhausted" });
        entry["last_status_at"] = serde_json::json!(now);
        entry["last_error_message"] = if *ok { serde_json::Value::Null } else { serde_json::json!(msg) };
    }
    std::fs::write(path, serde_json::to_string_pretty(&json).map_err(|_| "encode failed".to_owned())?).map_err(|_| "write failed".to_owned())
}

fn zai_short(value: &str, limit: usize) -> String {
    let clean = value.replace(['\n', '\r'], " ");
    if clean.chars().count() <= limit { clean } else { format!("{}…", clean.chars().take(limit).collect::<String>()) }
}

#[cfg(test)]
mod zai_tests {
    use super::*;
    #[test]
    fn zai_status_redacts_tokens() {
        let keys = vec![ZaiKey { index: 1, label: "k".to_owned(), source: "manual".to_owned(), base_url: "u".to_owned(), token: Some("SECRET".to_owned()), status: "ok".to_owned(), error: String::new() }];
        let out = zai_format_status(&keys);
        assert!(out.contains("ready"));
        assert!(!out.contains("SECRET"));
    }

    #[test]
    fn zai_pool_pointer_order_puts_fleet_scope_first() {
        assert_eq!(zai_pool_pointers(Some("3e")), vec!["/tokenPool/3e", "/tokenPool/zai", "/credential_pool/zai"]);
        assert_eq!(zai_pool_pointers(None), vec!["/tokenPool/zai", "/credential_pool/zai"]);
        assert_eq!(zai_pool_pointers(Some("zai")), vec!["/tokenPool/zai", "/credential_pool/zai"]);
    }

    #[test]
    fn zai_scoped_pool_reads_fleet_group_and_falls_back_to_global() {
        let dir = std::env::temp_dir().join(format!("maw-rs-zai-293-scoped-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("maw.config.json");
        let key = |label: &str| serde_json::json!({"label": label, "access_token": format!("tok-{label}")});
        let json = serde_json::json!({"tokenPool": {"3e": [key("a"), key("b")], "zai": [key("c"), key("d"), key("e"), key("f")]}});
        std::fs::write(&path, json.to_string()).unwrap();
        assert_eq!(zai_load_pool_at(&path, Some("3e")).unwrap().len(), 2);
        assert_eq!(zai_load_pool_at(&path, None).unwrap().len(), 4);
        assert_eq!(zai_load_pool_at(&path, Some("missing-group")).unwrap().len(), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn zai_split_fleet_extracts_flag_and_rejects_unsafe_groups() {
        let args = vec!["status".to_owned(), "--fleet".to_owned(), "3e".to_owned()];
        let (fleet, rest) = zai_split_fleet(&args).unwrap();
        assert_eq!(fleet.as_deref(), Some("3e"));
        assert_eq!(rest, vec!["status".to_owned()]);
        assert!(zai_split_fleet(&["--fleet".to_owned(), "a/b".to_owned()]).is_err());
        assert!(zai_split_fleet(&["--fleet".to_owned()]).is_err());
    }

    #[test]
    fn zai_fleet_token_requires_group() {
        let out = zai_fleet_token(&[]);
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("usage: maw fleet token"));
        let bad = zai_fleet_token(&["a;b".to_owned()]);
        assert_eq!(bad.code, 1);
        assert!(bad.stderr.contains("invalid group"));
    }
}
