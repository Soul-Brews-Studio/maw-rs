const DISPATCH_69: &[DispatcherEntry] = &[DispatcherEntry { command: "audit", handler: Handler::Sync(run_audit_command) }];

const AUDIT_USAGE: &str = "usage: maw audit [limit] [--anomalies] [--event <name>] [--since <iso>]";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct AuditOptions {
    limit: Option<usize>,
    event: Option<String>,
    since: Option<String>,
    anomalies: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AuditRowNative {
    ts: String,
    kind: Option<String>,
    event: Option<String>,
    cmd: Option<String>,
    args: Vec<String>,
    result: Option<String>,
    input: serde_json::Value,
}

fn run_audit_command(argv: &[String]) -> CliOutput {
    match audit_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn audit_run(argv: &[String]) -> Result<String, String> {
    let options = audit_parse_args(argv)?;
    let read_count = audit_read_count(&options);
    let mut rows = audit_parse_rows(&audit_read_lines(read_count));
    audit_apply_filters(&mut rows, &options);
    if let Some(limit) = options.limit { audit_truncate_tail(&mut rows, limit); }
    Ok(audit_render(&rows, &options))
}

fn audit_parse_args(argv: &[String]) -> Result<AuditOptions, String> {
    let mut options = AuditOptions::default();
    let mut positionals = Vec::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if arg == "--" { audit_push_tail(argv, index + 1, &mut positionals)?; break; }
        if let Some(consumed) = audit_parse_value_arg(argv, index, &mut options)? { index += consumed; continue; }
        if audit_parse_bool_arg(arg, &mut options) { index += 1; continue; }
        if arg == "--help" || arg == "-h" { return Err(AUDIT_USAGE.to_owned()); }
        if arg.starts_with('-') { return Err(format!("audit: unknown argument {arg}")); }
        audit_validate_value(arg, "limit")?;
        positionals.push(arg.clone());
        index += 1;
    }
    audit_finalize_options(options, &positionals)
}

fn audit_parse_value_arg(argv: &[String], index: usize, options: &mut AuditOptions) -> Result<Option<usize>, String> {
    let arg = &argv[index];
    let consumed = match arg.as_str() {
        "--event" => { options.event = Some(audit_take_value(argv, index, "--event")?); 2 }
        "--since" => { options.since = Some(audit_take_value(argv, index, "--since")?); 2 }
        _ => return audit_parse_equals_arg(arg, options),
    };
    Ok(Some(consumed))
}

fn audit_parse_equals_arg(arg: &str, options: &mut AuditOptions) -> Result<Option<usize>, String> {
    if let Some(value) = arg.strip_prefix("--event=") { audit_validate_value(value, "--event")?; options.event = Some(value.to_owned()); return Ok(Some(1)); }
    if let Some(value) = arg.strip_prefix("--since=") { audit_validate_value(value, "--since")?; options.since = Some(value.to_owned()); return Ok(Some(1)); }
    Ok(None)
}

fn audit_parse_bool_arg(arg: &str, options: &mut AuditOptions) -> bool {
    if arg == "--anomalies" { options.anomalies = true; return true; }
    false
}

fn audit_push_tail(argv: &[String], start: usize, positionals: &mut Vec<String>) -> Result<(), String> {
    for value in &argv[start..] {
        audit_validate_value(value, "limit")?;
        positionals.push(value.clone());
    }
    Ok(())
}

fn audit_finalize_options(mut options: AuditOptions, positionals: &[String]) -> Result<AuditOptions, String> {
    if positionals.len() > 1 { return Err(AUDIT_USAGE.to_owned()); }
    if let Some(value) = positionals.first() { options.limit = Some(audit_parse_limit(value)?); }
    Ok(options)
}

fn audit_take_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("audit: missing value for {flag}"))?;
    audit_validate_value(value, flag)?;
    Ok(value.clone())
}

fn audit_validate_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("audit: empty value for {label}")); }
    if value.starts_with('-') { return Err(format!("audit: {label} value must not start with '-'")); }
    if value.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r')) { return Err(format!("audit: invalid control character in {label}")); }
    Ok(())
}

fn audit_parse_limit(value: &str) -> Result<usize, String> {
    let limit = value.parse::<usize>().map_err(|_| "audit: limit must be a positive integer".to_owned())?;
    if limit == 0 { return Err("audit: limit must be a positive integer".to_owned()); }
    Ok(limit)
}

fn audit_read_count(options: &AuditOptions) -> usize {
    if options.anomalies || options.event.is_some() || options.since.is_some() { return 10_000; }
    options.limit.unwrap_or(20)
}

fn audit_read_lines(count: usize) -> Vec<String> {
    let path = audit_file_path();
    let Ok(raw) = std::fs::read_to_string(path) else { return Vec::new(); };
    let mut lines = raw.lines().filter(|line| !line.trim().is_empty()).map(str::to_owned).collect::<Vec<_>>();
    if lines.len() > count { lines = lines.split_off(lines.len() - count); }
    lines
}

fn audit_file_path() -> std::path::PathBuf {
    maw_state_path(&audit_xdg_env(), &["audit.jsonl"])
}

fn audit_parse_rows(lines: &[String]) -> Vec<AuditRowNative> {
    lines.iter().filter_map(|line| audit_parse_row(line)).collect()
}

fn audit_parse_row(line: &str) -> Option<AuditRowNative> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let ts = value.get("ts")?.as_str()?.to_owned();
    let args = value.get("args").and_then(serde_json::Value::as_array).map_or_else(Vec::new, |values| audit_parse_args_array(values));
    Some(AuditRowNative {
        ts,
        kind: audit_string_field(&value, "kind"),
        event: audit_string_field(&value, "event"),
        cmd: audit_string_field(&value, "cmd"),
        result: audit_string_field(&value, "result"),
        input: value.get("input").cloned().unwrap_or_else(|| serde_json::json!({})),
        args,
    })
}

fn audit_parse_args_array(values: &[serde_json::Value]) -> Vec<String> {
    values.iter().filter_map(serde_json::Value::as_str).map(str::to_owned).collect()
}

fn audit_string_field(value: &serde_json::Value, field: &str) -> Option<String> {
    value.get(field).and_then(serde_json::Value::as_str).map(str::to_owned)
}

fn audit_apply_filters(rows: &mut Vec<AuditRowNative>, options: &AuditOptions) {
    if options.anomalies { rows.retain(|row| row.kind.as_deref() == Some("anomaly")); }
    if let Some(event) = &options.event { rows.retain(|row| row.event.as_deref() == Some(event.as_str())); }
    if let Some(since) = &options.since { audit_filter_since(rows, since); }
}

fn audit_filter_since(rows: &mut Vec<AuditRowNative>, since: &str) {
    if !audit_valid_since(since) { return; }
    rows.retain(|row| row.ts.as_str() >= since);
}

fn audit_valid_since(value: &str) -> bool {
    value.len() >= 10 && value.as_bytes().get(4) == Some(&b'-') && value.as_bytes().get(7) == Some(&b'-')
}

fn audit_truncate_tail(rows: &mut Vec<AuditRowNative>, limit: usize) {
    if rows.len() > limit { *rows = rows.split_off(rows.len() - limit); }
}

fn audit_render(rows: &[AuditRowNative], options: &AuditOptions) -> String {
    if rows.is_empty() { return "\x1b[90mNo audit entries yet.\x1b[0m\n".to_owned(); }
    let label = if options.anomalies { "Anomaly Trail" } else { "Audit Trail" };
    let mut out = format!("\x1b[36m{label}\x1b[0m (last {})\n\n", rows.len());
    for row in rows { audit_render_row(row, &mut out); }
    out.push('\n');
    out
}

fn audit_render_row(row: &AuditRowNative, out: &mut String) {
    let time = audit_format_time(&row.ts);
    if row.kind.as_deref() == Some("anomaly") { audit_render_anomaly(row, out, &time); } else { audit_render_command(row, out, &time); }
}

fn audit_render_anomaly(row: &AuditRowNative, out: &mut String, time: &str) {
    let event = row.event.as_deref().unwrap_or("unknown");
    let input = serde_json::to_string(&row.input).unwrap_or_else(|_| "{}".to_owned());
    let _ = writeln!(out, "  \x1b[90m{time}\x1b[0m  \x1b[33m⚠\x1b[0m \x1b[35m{event}\x1b[0m  input={input}");
}

fn audit_render_command(row: &AuditRowNative, out: &mut String, time: &str) {
    let cmd = row.cmd.as_deref().unwrap_or("unknown");
    let args = audit_join_args(&row.args);
    let result = row.result.as_ref().map_or_else(String::new, |value| format!(" \x1b[90m→ {value}\x1b[0m"));
    let _ = writeln!(out, "  \x1b[90m{time}\x1b[0m  \x1b[33m{cmd}\x1b[0m{args}{result}");
}

fn audit_join_args(args: &[String]) -> String {
    if args.is_empty() { String::new() } else { format!(" {}", args.join(" ")) }
}

fn audit_format_time(ts: &str) -> String {
    let Some((date, time)) = ts.split_once('T') else { return ts.to_owned(); };
    let mut parts = date.split('-');
    let (_year, month, day) = (parts.next(), parts.next(), parts.next());
    let clock = time.get(0..8).unwrap_or(time.trim_end_matches('Z'));
    match (month, day) {
        (Some(month), Some(day)) => format!("{day} {} {clock}", audit_month_name(month)),
        _ => ts.to_owned(),
    }
}

fn audit_month_name(month: &str) -> &'static str {
    match month { "01" => "Jan", "02" => "Feb", "03" => "Mar", "04" => "Apr", "05" => "May", "06" => "Jun", "07" => "Jul", "08" => "Aug", "09" => "Sep", "10" => "Oct", "11" => "Nov", "12" => "Dec", _ => "???" }
}

fn audit_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = ["MAW_HOME", "MAW_STATE_DIR", "MAW_XDG", "XDG_STATE_HOME"];
    MawXdgEnv::with_vars(home, vars.into_iter().filter_map(|key| std::env::var(key).ok().map(|value| (key, value))))
}

#[cfg(test)]
mod audit_tests {
    use super::{audit_parse_args, run_audit_command, AUDIT_USAGE, DISPATCH_69};
    use std::fs;

    struct AuditEnvRestore { key: &'static str, value: Option<std::ffi::OsString> }

    impl AuditEnvRestore { fn audit_capture(key: &'static str) -> Self { Self { key, value: std::env::var_os(key) } } }

    impl Drop for AuditEnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.value.take() { std::env::set_var(self.key, value); } else { std::env::remove_var(self.key); }
        }
    }

    fn audit_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn audit_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-audit-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn audit_seed_env(name: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf, Vec<AuditEnvRestore>) {
        let lock = super::env_test_lock().lock().expect("env lock");
        let root = audit_temp_root(name);
        let restores = ["HOME", "XDG_STATE_HOME", "MAW_STATE_DIR", "MAW_HOME", "MAW_XDG", "TMUX"]
            .into_iter().map(AuditEnvRestore::audit_capture).collect::<Vec<_>>();
        let home = root.join("home");
        let state = root.join("state");
        fs::create_dir_all(state.join("maw")).expect("state");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_STATE_HOME", &state);
        std::env::set_var("MAW_XDG", "1");
        std::env::remove_var("MAW_HOME");
        std::env::remove_var("MAW_STATE_DIR");
        std::env::remove_var("TMUX");
        (lock, root, restores)
    }

    fn audit_write_log(root: &std::path::Path) {
        let file = root.join("state/maw/audit.jsonl");
        fs::write(file, include_str!("../../tests/fixtures/native-audit/audit.jsonl")).expect("audit log");
    }

    #[test]
    fn audit_dispatch_fragment_registers_native_audit_once() {
        assert_eq!(DISPATCH_69.len(), 1);
        assert_eq!(DISPATCH_69[0].command, "audit");
    }

    #[test]
    fn audit_parse_limit_flags_and_rejects_leading_dash_values() {
        let parsed = audit_parse_args(&audit_strings(&["7", "--event=bad-input", "--since", "2026-06-01T00:00:00.000Z"])).expect("parse");
        assert_eq!(parsed.limit, Some(7));
        assert_eq!(parsed.event.as_deref(), Some("bad-input"));
        assert_eq!(parsed.since.as_deref(), Some("2026-06-01T00:00:00.000Z"));
        let err = audit_parse_args(&audit_strings(&["--event", "--bad"])).expect_err("guard");
        assert!(err.contains("must not start"));
    }

    #[test]
    fn audit_missing_log_matches_maw_js_empty_message() {
        let (_lock, _root, _restore) = audit_seed_env("empty");
        let output = run_audit_command(&audit_strings(&[]));
        assert_eq!(output.code, 0);
        assert_eq!(output.stderr, "");
        assert_eq!(output.stdout, "\x1b[90mNo audit entries yet.\x1b[0m\n");
    }

    #[test]
    fn audit_default_tail_is_hermetic_and_matches_golden() {
        let (_lock, root, _restore) = audit_seed_env("tail");
        audit_write_log(&root);
        let output = run_audit_command(&audit_strings(&["2"]));
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, include_str!("../../tests/fixtures/native-audit/audit-tail.stdout"));
    }

    #[test]
    fn audit_anomaly_filter_is_hermetic_and_matches_golden() {
        let (_lock, root, _restore) = audit_seed_env("anomaly");
        audit_write_log(&root);
        let output = run_audit_command(&audit_strings(&["--anomalies"]));
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, include_str!("../../tests/fixtures/native-audit/audit-anomalies.stdout"));
    }

    #[test]
    fn audit_help_reports_usage_without_reading_real_state() {
        let output = run_audit_command(&audit_strings(&["--help"]));
        assert_eq!(output.code, 1);
        assert_eq!(output.stderr, format!("{AUDIT_USAGE}\n"));
    }
}
