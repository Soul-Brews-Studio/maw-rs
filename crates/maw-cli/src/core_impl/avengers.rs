const DISPATCH_74: &[DispatcherEntry] = &[DispatcherEntry {
    command: "avengers",
    handler: Handler::Async(run_avengers_async),
}];

const AVENGERS_HELP: &str = "usage: maw avengers [status|best|traffic|health] — ARRA-01 rate limit monitor\n\n  maw avengers status    All accounts + rate limits\n  maw avengers best      Account with most capacity\n  maw avengers traffic   Traffic stats\n  maw avengers health    Quick connectivity check\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AvengersCommand { Status, Best, Traffic, Health, Help }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AvengersOptions { command: AvengersCommand }

#[derive(Debug, Clone, PartialEq, Eq)]
struct AvengersHttpResponse { ok: bool, body: serde_json::Value, error: Option<String> }

fn run_avengers_async(argv: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { avengers_run(&argv) })
}

fn avengers_run(argv: &[String]) -> CliOutput {
    match avengers_parse_args(argv) {
        Ok(options) => avengers_execute(options),
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn avengers_execute(options: AvengersOptions) -> CliOutput {
    if options.command == AvengersCommand::Help { return CliOutput { code: 0, stdout: AVENGERS_HELP.to_owned(), stderr: String::new() }; }
    let Some(base) = avengers_config_url() else { return avengers_error("Avengers not configured. Add to maw.config.json:\n  \"avengers\": \"http://white.local:8090\""); };
    match options.command {
        AvengersCommand::Status => avengers_show_status(&base),
        AvengersCommand::Best => avengers_show_best(&base),
        AvengersCommand::Traffic => avengers_show_traffic(&base),
        AvengersCommand::Health => avengers_show_health(&base),
        AvengersCommand::Help => unreachable!(),
    }
}

fn avengers_parse_args(argv: &[String]) -> Result<AvengersOptions, String> {
    let mut positionals = Vec::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if arg == "--" { avengers_push_tail(argv, index + 1, &mut positionals)?; break; }
        match arg.as_str() {
            "--help" | "-h" => return Ok(AvengersOptions { command: AvengersCommand::Help }),
            value if value.starts_with('-') => return Err(format!("avengers: unknown argument {value}")),
            value => { avengers_validate_value(value, "subcommand")?; positionals.push(value.to_owned()); }
        }
        index += 1;
    }
    if positionals.len() > 1 { return Err("avengers: expected at most one subcommand".to_owned()); }
    Ok(AvengersOptions { command: avengers_command(positionals.first().map(String::as_str)) })
}

fn avengers_push_tail(argv: &[String], start: usize, positionals: &mut Vec<String>) -> Result<(), String> {
    for value in &argv[start..] { avengers_validate_value(value, "subcommand")?; positionals.push(value.clone()); }
    Ok(())
}

fn avengers_command(value: Option<&str>) -> AvengersCommand {
    match value.unwrap_or("status") {
        "status" | "all" => AvengersCommand::Status,
        "best" => AvengersCommand::Best,
        "traffic" => AvengersCommand::Traffic,
        "health" => AvengersCommand::Health,
        _ => AvengersCommand::Help,
    }
}

fn avengers_validate_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("avengers: empty value for {label}")); }
    if value.starts_with('-') { return Err(format!("avengers: {label} value must not start with '-'")); }
    if value.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r')) { return Err(format!("avengers: invalid control character in {label}")); }
    Ok(())
}

fn avengers_show_status(base: &str) -> CliOutput {
    let url = avengers_url(base, "all");
    match avengers_fetch_json(&url, 5_000) {
        Ok(response) if response.ok => CliOutput { code: 0, stdout: avengers_render_status(base, &response.body), stderr: String::new() },
        Ok(response) => avengers_unreachable(base, response.error.as_deref().unwrap_or("http error")),
        Err(error) => avengers_unreachable(base, &error),
    }
}

fn avengers_show_best(base: &str) -> CliOutput {
    let url = avengers_url(base, "best");
    match avengers_fetch_json(&url, 5_000) {
        Ok(response) if response.ok => CliOutput { code: 0, stdout: avengers_render_json_section("Best Account", &response.body), stderr: String::new() },
        Ok(response) => avengers_error(&format!("\x1b[31merror\x1b[0m: {}", response.error.unwrap_or_else(|| "http error".to_owned()))),
        Err(error) => avengers_error(&format!("\x1b[31merror\x1b[0m: {error}")),
    }
}

fn avengers_show_traffic(base: &str) -> CliOutput {
    let url = avengers_url(base, "traffic-stats");
    match avengers_fetch_json(&url, 5_000) {
        Ok(response) if response.ok => CliOutput { code: 0, stdout: avengers_render_json_section("Traffic Stats", &response.body), stderr: String::new() },
        Ok(response) => avengers_error(&format!("\x1b[31merror\x1b[0m: {}", response.error.unwrap_or_else(|| "http error".to_owned()))),
        Err(error) => avengers_error(&format!("\x1b[31merror\x1b[0m: {error}")),
    }
}

fn avengers_show_health(base: &str) -> CliOutput {
    let start = std::time::Instant::now();
    let url = avengers_url(base, "all");
    match avengers_fetch_json(&url, 3_000) {
        Ok(response) if response.ok => CliOutput { code: 0, stdout: avengers_render_health_online(base, start.elapsed().as_millis(), &response.body), stderr: String::new() },
        _ => CliOutput { code: 0, stdout: avengers_render_health_offline(base, start.elapsed().as_millis()), stderr: String::new() },
    }
}

fn avengers_fetch_json(url: &str, timeout_ms: u64) -> Result<AvengersHttpResponse, String> {
    let parsed = avengers_parse_http_url(url)?;
    let timeout = std::time::Duration::from_millis(timeout_ms);
    let mut stream = std::net::TcpStream::connect((&*parsed.host, parsed.port)).map_err(|error| error.to_string())?;
    stream.set_read_timeout(Some(timeout)).map_err(|error| error.to_string())?;
    stream.set_write_timeout(Some(timeout)).map_err(|error| error.to_string())?;
    avengers_write_http_request(&mut stream, &parsed)?;
    avengers_read_http_response(stream)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AvengersUrl { host: String, port: u16, path: String }

fn avengers_parse_http_url(url: &str) -> Result<AvengersUrl, String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| "avengers: only http:// URLs are supported natively".to_owned())?;
    let (authority, path_tail) = rest.split_once('/').unwrap_or((rest, ""));
    avengers_validate_value(authority, "url host")?;
    let (host, port) = avengers_parse_authority(authority)?;
    Ok(AvengersUrl { host, port, path: format!("/{path_tail}") })
}

fn avengers_parse_authority(authority: &str) -> Result<(String, u16), String> {
    let (host, port) = authority.rsplit_once(':').map_or((authority, 80), |(host, raw)| (host, raw.parse::<u16>().unwrap_or(0)));
    if host.is_empty() || port == 0 { return Err("avengers: invalid http url".to_owned()); }
    Ok((host.to_owned(), port))
}

fn avengers_write_http_request(stream: &mut std::net::TcpStream, parsed: &AvengersUrl) -> Result<(), String> {
    use std::io::Write as _;
    let request = format!("GET {} HTTP/1.0\r\nHost: {}\r\nAccept: application/json\r\nConnection: close\r\n\r\n", parsed.path, parsed.host);
    stream.write_all(request.as_bytes()).map_err(|error| error.to_string())
}

fn avengers_read_http_response(mut stream: std::net::TcpStream) -> Result<AvengersHttpResponse, String> {
    use std::io::Read as _;
    let mut raw = String::new();
    stream.read_to_string(&mut raw).map_err(|error| error.to_string())?;
    let (head, body) = raw.split_once("

").ok_or_else(|| "avengers: malformed http response".to_owned())?;
    let status = avengers_status_code(head)?;
    let value = serde_json::from_str::<serde_json::Value>(body.trim()).map_err(|error| error.to_string())?;
    Ok(AvengersHttpResponse { ok: (200..300).contains(&status), body: value, error: (!(200..300).contains(&status)).then(|| format!("HTTP {status}")) })
}

fn avengers_status_code(head: &str) -> Result<u16, String> {
    head.lines().next().and_then(|line| line.split_whitespace().nth(1)).and_then(|code| code.parse::<u16>().ok()).ok_or_else(|| "avengers: malformed http status".to_owned())
}

fn avengers_config_url() -> Option<String> {
    let raw = std::fs::read_to_string(maw_config_path(&current_xdg_env(), &["maw.config.json"])).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    value.get("avengers").and_then(serde_json::Value::as_str).filter(|value| !value.is_empty()).map(ToOwned::to_owned)
}

fn avengers_url(base: &str, path: &str) -> String { format!("{}/{path}", base.trim_end_matches('/')) }

fn avengers_unreachable(base: &str, message: &str) -> CliOutput {
    avengers_error(&format!("\x1b[31merror\x1b[0m: avengers unreachable at {base}: {message}"))
}

fn avengers_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn avengers_render_status(base: &str, accounts: &serde_json::Value) -> String {
    let mut out = format!("\n\x1b[36;1mAvengers Status\x1b[0m  \x1b[90m{base}\x1b[0m\n\n");
    if let Some(items) = accounts.as_array() { for account in items { avengers_render_account(account, &mut out); } } else if let Ok(text) = serde_json::to_string_pretty(accounts) { out.push_str(&text); out.push('\n'); }
    out.push('\n');
    out
}

fn avengers_render_account(account: &serde_json::Value, out: &mut String) {
    let name = avengers_account_name(account);
    let remaining = avengers_number_field(account, &["remaining", "requests_remaining"]);
    let limit = avengers_number_field(account, &["limit", "requests_limit"]);
    let pct = remaining.zip(limit).and_then(avengers_percent);
    let color = avengers_capacity_color(pct);
    let bar = avengers_capacity_bar(remaining, limit, pct, color);
    let _ = writeln!(out, "  {color}●\x1b[0m  {:<30}  {bar}", avengers_clip(&name, 30));
}

fn avengers_account_name(account: &serde_json::Value) -> String {
    ["name", "email", "id"].iter().find_map(|key| account.get(*key).and_then(serde_json::Value::as_str)).unwrap_or("?").to_owned()
}

fn avengers_number_field(account: &serde_json::Value, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|key| account.get(*key).and_then(serde_json::Value::as_i64))
}

fn avengers_percent((remaining, limit): (i64, i64)) -> Option<i64> {
    if limit <= 0 { return None; }
    Some(((i128::from(remaining) * 100) + (i128::from(limit) / 2)).div_euclid(i128::from(limit)).try_into().unwrap_or(i64::MAX))
}

fn avengers_capacity_color(pct: Option<i64>) -> &'static str {
    match pct { Some(value) if value > 50 => "\x1b[32m", Some(value) if value > 20 => "\x1b[33m", Some(_) => "\x1b[31m", None => "\x1b[37m" }
}

fn avengers_capacity_bar(remaining: Option<i64>, limit: Option<i64>, pct: Option<i64>, color: &str) -> String {
    match (remaining, limit, pct) { (Some(remaining), Some(limit), Some(pct)) => format!("{color}{remaining}/{limit} ({pct}%)\x1b[0m"), (Some(remaining), _, _) => remaining.to_string(), _ => "?".to_owned() }
}

fn avengers_render_json_section(title: &str, value: &serde_json::Value) -> String {
    let pretty = serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".to_owned());
    format!("\n\x1b[36;1m{title}\x1b[0m\n\n  {pretty}\n\n")
}

fn avengers_render_health_online(base: &str, latency_ms: u128, accounts: &serde_json::Value) -> String {
    let count = accounts.as_array().map_or(0, Vec::len);
    let plural = if count == 1 { "" } else { "s" };
    format!("\n\x1b[32m●\x1b[0m  Avengers \x1b[32monline\x1b[0m  \x1b[90m{latency_ms}ms · {count} account{plural}\x1b[0m\n   \x1b[90m{base}\x1b[0m\n\n")
}

fn avengers_render_health_offline(base: &str, latency_ms: u128) -> String {
    format!("\n\x1b[31m●\x1b[0m  Avengers \x1b[31moffline\x1b[0m  \x1b[90m{latency_ms}ms\x1b[0m\n   \x1b[90m{base}\x1b[0m\n\n")
}

fn avengers_clip(value: &str, width: usize) -> String { value.chars().take(width).collect() }

#[cfg(test)]
mod avengers_tests {
    use super::*;

    const ENV_KEYS: &[&str] = &["HOME", "MAW_HOME", "MAW_CONFIG_DIR", "MAW_XDG", "XDG_CONFIG_HOME", "TMUX"];

    struct AvengersEnv { config: std::path::PathBuf, saved: Vec<(&'static str, Option<std::ffi::OsString>)> }

    impl AvengersEnv {
        fn avengers_new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!("maw-rs-avengers-{name}-{}", SystemTime::now().duration_since(UNIX_EPOCH).expect("clock").as_nanos()));
            let home = root.join("home");
            let config_home = root.join("config");
            std::fs::create_dir_all(config_home.join("maw")).expect("config dir");
            std::fs::create_dir_all(&home).expect("home");
            let saved = ENV_KEYS.iter().map(|key| (*key, std::env::var_os(key))).collect::<Vec<_>>();
            for key in ENV_KEYS { std::env::remove_var(key); }
            std::env::set_var("HOME", &home);
            std::env::set_var("MAW_XDG", "1");
            std::env::set_var("XDG_CONFIG_HOME", &config_home);
            Self { config: config_home.join("maw/maw.config.json"), saved }
        }

        fn avengers_write_config(&self, base: &str) { std::fs::write(&self.config, format!(r#"{{"avengers":"{base}"}}"#)).expect("config"); }
    }

    impl Drop for AvengersEnv {
        fn drop(&mut self) {
            for key in ENV_KEYS { std::env::remove_var(key); }
            for (key, value) in self.saved.drain(..) { if let Some(value) = value { std::env::set_var(key, value); } }
        }
    }

    fn avengers_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn avengers_dispatch_fragment_registers_native_avengers_once() {
        let commands = DISPATCH_74.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, vec!["avengers"]);
    }

    #[test]
    fn avengers_parse_subcommands_and_guard_leading_dash_values() {
        assert_eq!(avengers_parse_args(&avengers_args(&[])).expect("default").command, AvengersCommand::Status);
        assert_eq!(avengers_parse_args(&avengers_args(&["all"])).expect("all").command, AvengersCommand::Status);
        assert_eq!(avengers_parse_args(&avengers_args(&["--help"])).expect("help").command, AvengersCommand::Help);
        assert!(avengers_parse_args(&avengers_args(&["--bad"])).expect_err("dash").contains("unknown argument"));
        assert!(avengers_parse_args(&avengers_args(&["--", "-bad"])).expect_err("tail dash").contains("must not start"));
    }

    #[test]
    fn avengers_missing_config_is_hermetic() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let _env = AvengersEnv::avengers_new("missing");
        let output = avengers_run(&avengers_args(&["status"]));
        assert_eq!(output.code, 1);
        assert!(output.stderr.contains("Avengers not configured"));
    }

    #[test]
    fn avengers_reads_seeded_config_only() {
        let _lock = super::env_test_lock().lock().expect("lock");
        let env = AvengersEnv::avengers_new("config");
        env.avengers_write_config("http://127.0.0.1:7777/");
        assert_eq!(avengers_config_url().as_deref(), Some("http://127.0.0.1:7777/"));
        assert_eq!(avengers_url("http://127.0.0.1:7777/", "all"), "http://127.0.0.1:7777/all");
    }

    #[test]
    fn avengers_status_render_matches_rate_limit_colors() {
        let accounts = serde_json::json!([
            {"name":"alpha","remaining":80,"limit":100},
            {"email":"beta@example.test","requests_remaining":10,"requests_limit":100}
        ]);
        let out = avengers_render_status("http://avengers", &accounts);
        assert!(out.contains("Avengers Status"));
        assert!(out.contains("80/100 (80%)"));
        assert!(out.contains("10/100 (10%)"));
    }

    #[test]
    fn avengers_json_and_health_render_are_stable() {
        let value = serde_json::json!({"account":"alpha","remaining":42});
        assert!(avengers_render_json_section("Best Account", &value).contains("\"remaining\": 42"));
        assert!(avengers_render_health_online("http://avengers", 7, &serde_json::json!([{}, {}])).contains("2 accounts"));
        assert!(avengers_render_health_offline("http://avengers", 9).contains("offline"));
    }
}
