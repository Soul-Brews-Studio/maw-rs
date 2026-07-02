const DISPATCH_92: &[DispatcherEntry] = &[DispatcherEntry {
    command: "forward-error",
    handler: Handler::Async(forwarderror_async),
}];

const FORWARDERROR_USAGE: &str = "usage: maw forward-error [--to <target>] [--last N]";
const FORWARDERROR_DEFAULT_LAST: u32 = 30;
const FORWARDERROR_MAX_LAST: u32 = 500;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ForwarderrorArgs {
    target: Option<String>,
    last: u32,
}

fn forwarderror_async(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move { forwarderror_run_async_impl(&args).await })
}

async fn forwarderror_run_async_impl(raw_args: &[String]) -> CliOutput {
    let args = match forwarderror_parse_args(raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return forwarderror_usage_error(&message),
    };
    let target = match forwarderror_resolve_target(args.target.as_deref()) {
        Ok(target) => target,
        Err(message) => return forwarderror_error(2, &message),
    };
    let captured = match forwarderror_capture_pane(args.last) {
        Ok(text) => text,
        Err(message) => return forwarderror_error(1, &message),
    };
    let message = forwarderror_message(&captured);
    let config = load_hey_config();
    let mut tmux = TmuxClient::local();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(&target, &config.route, &sessions) {
        RouteResult::Local { target: pane } | RouteResult::SelfNode { target: pane } => {
            forwarderror_local(&mut tmux, &pane, &target, &message, args.last, &config)
        }
        RouteResult::Peer { peer_url, target: peer_target, node } => {
            forwarderror_peer(&peer_url, &peer_target, Some(node.as_str()), &target, &message, args.last, &config).await
        }
        RouteResult::Error { detail, hint, .. } => forwarderror_route_error(&detail, hint.as_deref()),
    }
}

fn forwarderror_parse_args(argv: &[String]) -> Result<ForwarderrorArgs, String> {
    let mut target = None;
    let mut last = FORWARDERROR_DEFAULT_LAST;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(String::new()),
            "--" => return Err("forward-error: -- separator is not supported".to_owned()),
            "--to" => {
                let value = forwarderror_next(argv, index, "--to")?;
                target = Some(forwarderror_validate_target(value)?);
                index += 1;
            }
            "--last" => {
                last = forwarderror_parse_last(forwarderror_next(argv, index, "--last")?)?;
                index += 1;
            }
            value if value.starts_with("--to=") => {
                target = Some(forwarderror_validate_target(&value["--to=".len()..])?);
            }
            value if value.starts_with("--last=") => {
                last = forwarderror_parse_last(&value["--last=".len()..])?;
            }
            value if value.starts_with('-') => return Err(format!("forward-error: unknown argument {value}")),
            value => return Err(format!("forward-error: unknown argument {value}")),
        }
        index += 1;
    }
    Ok(ForwarderrorArgs { target, last })
}

fn forwarderror_next<'a>(argv: &'a [String], index: usize, flag: &str) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1).map(String::as_str) else {
        return Err(format!("forward-error: missing value for {flag}"));
    };
    if value.starts_with('-') { return Err(format!("forward-error: missing value for {flag}")); }
    Ok(value)
}

fn forwarderror_parse_last(value: &str) -> Result<u32, String> {
    if !value.bytes().all(|byte| byte.is_ascii_digit()) || value.is_empty() {
        return Err(format!("forward-error: invalid --last value '{value}'"));
    }
    let parsed = value.parse::<u32>().map_err(|_| format!("forward-error: invalid --last value '{value}'"))?;
    if parsed == 0 { return Err("forward-error: --last must be a positive integer".to_owned()); }
    Ok(parsed.min(FORWARDERROR_MAX_LAST))
}

fn forwarderror_validate_target(value: &str) -> Result<String, String> {
    if value.trim().is_empty()
        || value.trim() != value
        || value.starts_with('-')
        || value.contains('/')
        || value.contains("..")
        || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("forward-error: invalid target {value:?}"));
    }
    Ok(value.to_owned())
}

fn forwarderror_resolve_target(explicit: Option<&str>) -> Result<String, String> {
    if let Some(target) = explicit { return forwarderror_validate_target(target); }
    if let Some(target) = forwarderror_config_target() { return forwarderror_validate_target(&target); }
    Ok("doctor".to_owned())
}

fn forwarderror_config_target() -> Option<String> {
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    value
        .get("errorForward")
        .and_then(|item| item.get("target"))
        .and_then(serde_json::Value::as_str)
        .filter(|target| !target.is_empty())
        .map(ToOwned::to_owned)
}

fn forwarderror_capture_pane(last: u32) -> Result<String, String> {
    let output = std::process::Command::new("tmux")
        .args(["capture-pane", "-p", "-S", &format!("-{last}")])
        .output()
        .map_err(|error| format!("tmux capture-pane failed: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let code = output.status.code().map_or(String::new(), |code| format!(" (exit {code})"));
        let detail = if stderr.is_empty() { String::new() } else { format!(": {stderr}") };
        return Err(format!("tmux capture-pane failed{code}{detail}"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim_end().to_owned())
}

fn forwarderror_message(error: &str) -> String {
    let cwd = std::env::current_dir().map_or_else(|_| String::new(), |path| path.display().to_string());
    serde_json::json!({
        "error": error,
        "cwd": cwd,
        "exitCode": forwarderror_exit_code(),
        "timestamp": forwarderror_timestamp(),
    })
    .to_string()
}

fn forwarderror_exit_code() -> Option<i64> {
    ["MAW_FORWARD_EXIT_CODE", "MAW_LAST_EXIT_CODE", "LAST_EXIT_CODE"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|raw| forwarderror_is_integer(raw)).and_then(|raw| raw.parse::<i64>().ok()))
}

fn forwarderror_is_integer(value: &str) -> bool {
    let digits = value.strip_prefix('-').unwrap_or(value);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn forwarderror_timestamp() -> String {
    if let Ok(value) = std::env::var("MAW_RS_FORWARDERROR_NOW") {
        if !value.trim().is_empty() { return value; }
    }
    let seconds = current_epoch_seconds();
    let (year, month, day, hour, minute, second) = forwarderror_utc_from_unix(seconds);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.000Z")
}

fn forwarderror_utc_from_unix(seconds: u64) -> (i32, u32, u32, u32, u32, u32) {
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let rem = seconds % 86_400;
    let (year, month, day) = forwarderror_date_from_days(days);
    let hour = u32::try_from(rem / 3_600).unwrap_or(0);
    let minute = u32::try_from((rem % 3_600) / 60).unwrap_or(0);
    let second = u32::try_from(rem % 60).unwrap_or(0);
    (year, month, day, hour, minute, second)
}

fn forwarderror_date_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = y + i64::from(month <= 2);
    (i32::try_from(year).unwrap_or(i32::MAX), u32::try_from(month).unwrap_or(1), u32::try_from(day).unwrap_or(1))
}

fn forwarderror_local(
    tmux: &mut TmuxClient<maw_tmux::CommandTmuxRunner>,
    pane: &str,
    original_target: &str,
    message: &str,
    last: u32,
    config: &HeyConfig,
) -> CliOutput {
    if let Err(message) = forwarderror_validate_tmux_target(pane) { return forwarderror_error(2, &message); }
    let output = send_local_message("forward-error", tmux, pane, message, config, None);
    if output.code == 0 { forwarderror_success(last, original_target) } else { output }
}

async fn forwarderror_peer(peer_url: &str, peer_target: &str, node: Option<&str>, original_target: &str, message: &str, last: u32, config: &HeyConfig) -> CliOutput {
    if let Err(message) = forwarderror_validate_transport_target(peer_target) { return forwarderror_error(2, &message); }
    let send_args = SendArgs { target: peer_target.to_owned(), text: message.to_owned(), inbox: None, from: None, approve: false, trust: false };
    let output = match send_acl_gate_peer("forward-error", peer_target, &send_args, config, false) {
        SendAclGateResult::Proceed { stderr_prefix } => {
            if let Some(output) = forwarderror_fake_peer(peer_url, peer_target, node, original_target, message, last) {
                send_acl_apply_proceed_stderr(output, &stderr_prefix)
            } else {
                send_acl_deliver_peer_message("forward-error", peer_url, peer_target, &send_args, config, stderr_prefix).await
            }
        }
        SendAclGateResult::Queued(output) | SendAclGateResult::Reject(output) => return output,
    };
    if output.code == 0 { forwarderror_success(last, original_target) } else { output }
}

fn forwarderror_fake_peer(peer_url: &str, peer_target: &str, node: Option<&str>, original_target: &str, message: &str, last: u32) -> Option<CliOutput> {
    let path = std::env::var_os("MAW_RS_FORWARDERROR_FAKE_PEER_LOG")?;
    let row = serde_json::json!({"peerUrl": peer_url, "target": peer_target, "node": node, "originalTarget": original_target, "text": message});
    let result = std::fs::OpenOptions::new().create(true).append(true).open(&path).and_then(|mut file| { use std::io::Write as _; writeln!(file, "{row}") });
    if let Err(error) = result { return Some(forwarderror_error(1, &format!("forward-error: fake peer transport failed: {error}"))); }
    Some(forwarderror_success(last, original_target))
}

fn forwarderror_validate_tmux_target(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("forward-error: invalid tmux target {value:?}"));
    }
    Ok(())
}

fn forwarderror_validate_transport_target(value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') || value.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err(format!("forward-error: invalid transport target {value:?}"));
    }
    Ok(())
}

fn forwarderror_route_error(detail: &str, hint: Option<&str>) -> CliOutput {
    let reason = hint.map_or_else(|| detail.to_owned(), |hint| format!("{detail}; {hint}"));
    forwarderror_error(2, &format!("forward-error: {reason}"))
}

fn forwarderror_usage_error(message: &str) -> CliOutput {
    let prefix = if message.is_empty() { String::new() } else { format!("{message}\n") };
    CliOutput { code: 2, stdout: String::new(), stderr: format!("{prefix}{FORWARDERROR_USAGE}\n") }
}

fn forwarderror_error(code: i32, message: &str) -> CliOutput {
    CliOutput { code, stdout: String::new(), stderr: format!("{message}\n") }
}

fn forwarderror_success(last: u32, target: &str) -> CliOutput {
    CliOutput { code: 0, stdout: format!("forwarded last {last} line(s) to {target}\n"), stderr: String::new() }
}

#[cfg(test)]
mod forwarderror_tests {
    use super::*;

    fn forwarderror_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn forwarderror_parse_flags() {
        assert_eq!(forwarderror_parse_args(&forwarderror_strings(&[])).unwrap(), ForwarderrorArgs { target: None, last: 30 });
        assert_eq!(forwarderror_parse_args(&forwarderror_strings(&["--to", "doctor-alpha", "--last", "12"])).unwrap(), ForwarderrorArgs { target: Some("doctor-alpha".to_owned()), last: 12 });
        assert_eq!(forwarderror_parse_args(&forwarderror_strings(&["--to=doctor-alpha", "--last=9"])).unwrap(), ForwarderrorArgs { target: Some("doctor-alpha".to_owned()), last: 9 });
        assert!(forwarderror_parse_args(&forwarderror_strings(&["--last", "nope"])).unwrap_err().contains("invalid --last"));
    }

    #[test]
    fn forwarderror_guards_target_and_separator() {
        assert!(forwarderror_parse_args(&forwarderror_strings(&["--", "doctor"])).unwrap_err().contains("separator"));
        assert!(forwarderror_parse_args(&forwarderror_strings(&["--to", "--doctor"])).unwrap_err().contains("missing value"));
        assert!(forwarderror_parse_args(&forwarderror_strings(&["--to=../doctor"])).unwrap_err().contains("invalid target"));
    }

    #[test]
    fn forwarderror_timestamp_formats_epoch() {
        assert_eq!(forwarderror_utc_from_unix(1_780_884_184), (2026, 6, 8, 2, 3, 4));
    }
}
