const DISPATCH_65: &[DispatcherEntry] = &[DispatcherEntry { command: "doctor", handler: Handler::Sync(run_doctor_command) }];

#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct DoctorOptions {
    only: Option<String>,
    gateway: Option<String>,
    forward: Option<String>,
    manifest_path: Option<String>,
    port: Option<u16>,
    backend: Option<u16>,
    json: bool,
    allow_drift: bool,
    capture: bool,
    dry_run: bool,
    errors: bool,
    fix_sessions: bool,
    fix_stale: bool,
    fix_xdg: bool,
    migrate: bool,
    no_prompt: bool,
    plan: bool,
    release: bool,
    smoke: bool,
    version: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorCheckNative {
    name: String,
    ok: bool,
    severity: &'static str,
    message: String,
    fixes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DoctorComparisonNative {
    name: String,
    before: String,
    after: String,
    status: String,
}

fn run_doctor_command(argv: &[String]) -> CliOutput {
    match doctor_run(argv) {
        Ok((code, stdout)) => CliOutput { code, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn doctor_run(argv: &[String]) -> Result<(i32, String), String> {
    let options = doctor_parse_args(argv)?;
    if options.fix_stale { return doctor_fix_stale(&options); }
    if options.fix_sessions { return doctor_fix_sessions(&options); }
    let previous = doctor_load_last_run();
    let mut checks = doctor_collect_checks(&options);
    let hard_ok = checks.iter().all(|check| check.ok);
    let drift_only = checks.iter().all(|check| check.ok || check.name.starts_with("version:"));
    let ok = hard_ok || (options.allow_drift && drift_only);
    let comparison = doctor_compare_runs(previous.as_ref(), &checks);
    doctor_persist_last_run(&checks);
    if let Some(forward) = doctor_forward_check(&options, &checks, &comparison) { checks.push(forward); }
    let stdout = if options.json { doctor_render_json(&checks, ok, &comparison)? } else { doctor_render_text(&checks, ok, &comparison) };
    Ok((i32::from(!ok), stdout))
}

fn doctor_parse_args(argv: &[String]) -> Result<DoctorOptions, String> {
    let mut options = doctor_default_options();
    let mut positionals = Vec::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if arg == "--" { doctor_push_tail(argv, index + 1, &mut positionals)?; break; }
        if let Some(consumed) = doctor_parse_value_arg(argv, index, &mut options)? { index += consumed; continue; }
        if doctor_parse_bool_arg(arg, &mut options) { index += 1; continue; }
        if arg.starts_with('-') { return Err(format!("doctor: unknown argument {arg}")); }
        doctor_validate_arg_value(arg, "check")?;
        positionals.push(arg.clone());
        index += 1;
    }
    doctor_finalize_options(options, &positionals)
}

fn doctor_default_options() -> DoctorOptions {
    DoctorOptions {
        only: None, gateway: None, forward: None, manifest_path: None, port: None, backend: None,
        json: false, allow_drift: false, capture: false, dry_run: false, errors: false,
        fix_sessions: false, fix_stale: false, fix_xdg: false, migrate: false, no_prompt: false,
        plan: false, release: false, smoke: false, version: false,
    }
}

fn doctor_parse_value_arg(argv: &[String], index: usize, options: &mut DoctorOptions) -> Result<Option<usize>, String> {
    let arg = &argv[index];
    let consumed = match arg.as_str() {
        "--forward" => { options.forward = Some(doctor_take_value(argv, index, "--forward", doctor_validate_target)?); 2 }
        "--gateway" => { options.gateway = Some(doctor_take_value(argv, index, "--gateway", doctor_validate_gateway)?); 2 }
        "--manifest-path" => { options.manifest_path = Some(doctor_take_value(argv, index, "--manifest-path", doctor_validate_path_arg)?); 2 }
        "--port" => { options.port = Some(doctor_take_port(argv, index, "--port")?); 2 }
        "--backend" => { options.backend = Some(doctor_take_port(argv, index, "--backend")?); 2 }
        _ => return doctor_parse_equals_arg(arg, options),
    };
    Ok(Some(consumed))
}

fn doctor_parse_equals_arg(arg: &str, options: &mut DoctorOptions) -> Result<Option<usize>, String> {
    if let Some(value) = arg.strip_prefix("--forward=") { doctor_validate_target(value, "--forward")?; options.forward = Some(value.to_owned()); return Ok(Some(1)); }
    if let Some(value) = arg.strip_prefix("--gateway=") { doctor_validate_gateway(value, "--gateway")?; options.gateway = Some(value.to_owned()); return Ok(Some(1)); }
    if let Some(value) = arg.strip_prefix("--manifest-path=") { doctor_validate_path_arg(value, "--manifest-path")?; options.manifest_path = Some(value.to_owned()); return Ok(Some(1)); }
    if let Some(value) = arg.strip_prefix("--port=") { options.port = Some(doctor_parse_port(value, "--port")?); return Ok(Some(1)); }
    if let Some(value) = arg.strip_prefix("--backend=") { options.backend = Some(doctor_parse_port(value, "--backend")?); return Ok(Some(1)); }
    Ok(None)
}

fn doctor_parse_bool_arg(arg: &str, options: &mut DoctorOptions) -> bool {
    match arg {
        "--allow-drift" => options.allow_drift = true,
        "--capture" => options.capture = true,
        "--dry-run" => options.dry_run = true,
        "--errors" => options.errors = true,
        "--fix-sessions" => options.fix_sessions = true,
        "--fix-stale" => options.fix_stale = true,
        "--fix-xdg" => options.fix_xdg = true,
        "--json" => options.json = true,
        "--migrate" => options.migrate = true,
        "--no-prompt" => options.no_prompt = true,
        "--plan" => options.plan = true,
        "--release" => options.release = true,
        "--smoke" => options.smoke = true,
        "--version" => options.version = true,
        _ => return false,
    }
    true
}

fn doctor_push_tail(argv: &[String], start: usize, positionals: &mut Vec<String>) -> Result<(), String> {
    for value in &argv[start..] {
        doctor_validate_arg_value(value, "check")?;
        positionals.push(value.clone());
    }
    Ok(())
}

fn doctor_finalize_options(mut options: DoctorOptions, positionals: &[String]) -> Result<DoctorOptions, String> {
    if positionals.len() > 1 { return Err("doctor: expected at most one check name".to_owned()); }
    if let Some(only) = positionals.first() { options.only = Some(only.clone()); }
    if options.version && options.only.is_none() { options.only = Some("version".to_owned()); }
    if options.smoke && options.only.is_none() { options.only = Some("smoke".to_owned()); }
    Ok(options)
}

fn doctor_take_value(argv: &[String], index: usize, flag: &str, validate: fn(&str, &str) -> Result<(), String>) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("doctor: missing value for {flag}"))?;
    validate(value, flag)?;
    Ok(value.clone())
}

fn doctor_take_port(argv: &[String], index: usize, flag: &str) -> Result<u16, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("doctor: missing value for {flag}"))?;
    doctor_parse_port(value, flag)
}

fn doctor_parse_port(value: &str, flag: &str) -> Result<u16, String> {
    doctor_validate_arg_value(value, flag)?;
    let port = value.parse::<u16>().map_err(|_| format!("doctor: {flag} must be a tcp port"))?;
    if port == 0 { return Err(format!("doctor: {flag} must be a tcp port")); }
    Ok(port)
}

fn doctor_validate_arg_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() { return Err(format!("doctor: empty value for {label}")); }
    if value.starts_with('-') { return Err(format!("doctor: {label} value must not start with '-'")); }
    if value.bytes().any(|byte| matches!(byte, 0 | b'\n' | b'\r')) { return Err(format!("doctor: invalid control character in {label}")); }
    Ok(())
}

fn doctor_validate_target(value: &str, flag: &str) -> Result<(), String> { doctor_validate_arg_value(value, flag) }

fn doctor_validate_gateway(value: &str, flag: &str) -> Result<(), String> {
    doctor_validate_arg_value(value, flag)?;
    if matches!(value, "bun" | "rust") { Ok(()) } else { Err(format!("doctor: {flag} must be bun or rust")) }
}

fn doctor_validate_path_arg(value: &str, flag: &str) -> Result<(), String> { doctor_validate_arg_value(value, flag) }

fn doctor_collect_checks(options: &DoctorOptions) -> Vec<DoctorCheckNative> {
    let only = options.only.as_deref();
    let mut checks = Vec::new();
    if only == Some("smoke") { return doctor_smoke_checks(); }
    if doctor_should_run(only, &["serve"]) { checks.push(doctor_check_serve(options)); }
    if doctor_should_run(only, &["gateway", "all"]) { checks.push(doctor_check_gateway(options)); }
    if doctor_should_run(only, &["install", "all"]) { checks.push(doctor_check_install(options)); }
    if only.is_some_and(|value| matches!(value, "xdg" | "all")) { checks.push(doctor_check_xdg(options)); }
    if doctor_should_run(only, &["version", "all"]) { checks.push(doctor_check_version(options)); }
    if doctor_should_run(only, &["plugins"]) { checks.push(doctor_check_plugins()); }
    if doctor_should_run(only, &["peers", "all"]) { checks.push(doctor_check_peer_duplicates()); checks.push(doctor_check_stale_peers()); }
    if doctor_should_run(only, &["hub"]) { checks.push(doctor_check_hub()); }
    if doctor_should_run(only, &["scout"]) { checks.push(doctor_ok("scout", "scout check is native-noop on maw-rs")); }
    if doctor_should_run(only, &["federation"]) { checks.push(doctor_ok("federation", "federation reachability skipped without configured peers")); }
    if doctor_should_run(only, &["disk"]) { checks.push(doctor_check_disk()); }
    if doctor_should_run(only, &["manifest", "all"]) { checks.push(doctor_check_manifest(options)); }
    if doctor_should_run(only, &["maw-js", "all"]) { checks.push(doctor_check_maw_js()); }
    if doctor_should_run(only, &["worktrees", "all"]) { checks.push(doctor_check_worktrees()); }
    checks
}

fn doctor_should_run(only: Option<&str>, names: &[&str]) -> bool {
    only.is_none_or(|value| names.contains(&value))
}

fn doctor_check_serve(options: &DoctorOptions) -> DoctorCheckNative {
    let port = options.port.unwrap_or_else(doctor_default_port);
    if doctor_tcp_reachable(port) { return doctor_ok("serve", &format!("running on :{port}")); }
    doctor_warn("serve", &format!("not reachable on :{port}"), &["maw serve"])
}

fn doctor_check_gateway(options: &DoctorOptions) -> DoctorCheckNative {
    let selected = doctor_gateway_kind(options);
    if selected != "rust" { return doctor_info("gateway", &format!("gateway {selected} selected — rust probe skipped")); }
    let binary = doctor_find_on_path("maw-gateway").or_else(|| doctor_find_on_path("maw-rs"));
    match binary {
        Some(path) => doctor_info("gateway:rust", &format!("binary found at {}", path.display())),
        None => doctor_error("gateway:rust", "binary not found on PATH — build maw gateway", &["cargo build --release"]),
    }
}

fn doctor_check_install(options: &DoctorOptions) -> DoctorCheckNative {
    let exe = std::env::current_exe().ok();
    let mode = if options.release { "release" } else { "debug-or-release" };
    match exe {
        Some(path) => doctor_info("install", &format!("maw-rs executable present ({mode}) at {}", path.display())),
        None => doctor_error("install", "could not resolve current executable", &["cargo build --workspace"]),
    }
}

fn doctor_check_xdg(options: &DoctorOptions) -> DoctorCheckNative {
    if options.migrate || options.fix_xdg { return doctor_migrate_xdg(options.dry_run || options.plan); }
    let env = doctor_xdg_env();
    let config = maw_config_dir(&env);
    let data = maw_data_dir(&env);
    let state = maw_state_dir(&env);
    let enabled = is_maw_xdg_enabled(&env);
    doctor_info("xdg", &format!("enabled={enabled}; config={}; data={}; state={}", config.display(), data.display(), state.display()))
}

fn doctor_check_version(_options: &DoctorOptions) -> DoctorCheckNative {
    doctor_info("version:source", &format!("maw-rs {} (no running maw probe in native doctor)", env!("CARGO_PKG_VERSION")))
}

fn doctor_check_plugins() -> DoctorCheckNative {
    let dir = maw_data_path(&doctor_xdg_env(), &["plugins"]);
    let entries = std::fs::read_dir(&dir).map(|iter| iter.flatten().collect::<Vec<_>>());
    match entries {
        Ok(values) => doctor_info("plugins", &format!("{} loaded (broken symlink scan native)", values.len())),
        Err(error) => doctor_warn("plugins", &format!("{} ({error})", dir.display()), &["maw plugin install"]),
    }
}

fn doctor_check_peer_duplicates() -> DoctorCheckNative {
    let env = doctor_peer_env();
    let store = maw_peer::load_peer_store(&env);
    let (total, duplicates) = doctor_count_duplicate_peer_identities(&store);
    if duplicates.is_empty() { return doctor_info("peers:duplicates", &format!("no <oracle>:<node> collisions across {total} peer{}", doctor_plural(total))); }
    doctor_error("peers:duplicates", &duplicates.join("; "), &["maw peers list"])
}

fn doctor_check_stale_peers() -> DoctorCheckNative {
    let result = maw_peer::stale_peer_check(&doctor_peer_env(), doctor_now_ms());
    doctor_from_peer_check(&result, "maw doctor --fix-stale")
}

fn doctor_check_hub() -> DoctorCheckNative {
    let cfg = maw_config_path(&doctor_xdg_env(), &["maw.config.json"]);
    if cfg.exists() { doctor_info("hub", &format!("config readable at {}", cfg.display())) } else { doctor_warn("hub", &format!("config missing at {}", cfg.display()), &["maw init"]) }
}

fn doctor_check_disk() -> DoctorCheckNative {
    let dir = maw_state_dir(&doctor_xdg_env());
    if dir.exists() { doctor_info("disk", &format!("state dir reachable at {}", dir.display())) } else { doctor_warn("disk", &format!("state dir missing at {}", dir.display()), &["maw init"]) }
}

fn doctor_check_manifest(options: &DoctorOptions) -> DoctorCheckNative {
    let path = options.manifest_path.as_ref().map_or_else(|| maw_config_path(&doctor_xdg_env(), &["oracles.json"]), std::path::PathBuf::from);
    if !path.exists() { return doctor_info("manifest:cross-source", &format!("manifest unreadable ({}) — skipping cross-source check", path.display())); }
    match std::fs::read_to_string(&path).and_then(|raw| serde_json::from_str::<serde_json::Value>(&raw).map_err(std::io::Error::other)) {
        Ok(_) => doctor_info("manifest:cross-source", "manifest parsed; no native cross-source gaps detected"),
        Err(error) => doctor_warn("manifest:cross-source", &format!("manifest unreadable ({error}) — skipping cross-source check"), &[]),
    }
}

fn doctor_check_maw_js() -> DoctorCheckNative {
    let ghq = doctor_ghq_root();
    let path = ghq.join("github.com").join("Soul-Brews-Studio").join("maw-js");
    if path.join("package.json").exists() { doctor_info("maw-js", &format!("checkout present at {}", path.display())) } else { doctor_warn("maw-js", "maw-js checkout not found under GHQ_ROOT", &[]) }
}

fn doctor_check_worktrees() -> DoctorCheckNative {
    let ghq = doctor_ghq_root();
    let count = doctor_count_worktree_dirs(&ghq);
    doctor_info("worktrees:stillborn", &format!("scanned {count} candidate worktree dir{}", doctor_plural(count)))
}

fn doctor_smoke_checks() -> Vec<DoctorCheckNative> {
    vec![
        doctor_info("smoke:ls", "native dispatch reachable"),
        doctor_info("smoke:version", &format!("maw-rs {}", env!("CARGO_PKG_VERSION"))),
        doctor_check_plugins(),
    ]
}

fn doctor_fix_stale(options: &DoctorOptions) -> Result<(i32, String), String> {
    let result = if options.dry_run || options.plan { maw_peer::stale_peer_check(&doctor_peer_env(), doctor_now_ms()) } else { maw_peer::remove_stale_peers(&doctor_peer_env(), doctor_now_ms()).map_err(|error| error.to_string())? };
    let check = doctor_from_peer_check(&result, "maw doctor --fix-stale");
    let ok = check.ok;
    let checks = vec![check];
    let stdout = if options.json { doctor_render_json(&checks, ok, &[])? } else { doctor_render_text(&checks, ok, &[]) };
    Ok((i32::from(!ok), stdout))
}

fn doctor_fix_sessions(options: &DoctorOptions) -> Result<(i32, String), String> {
    let dry = options.dry_run || options.plan;
    let check = if dry { doctor_info("sessions:fix-doubled", "dry-run: no doubled github.com/github.com dirs scanned by native doctor") } else { doctor_warn("sessions:fix-doubled", "native doctor refuses automatic session rewrites; rerun maw-js doctor if needed", &[]) };
    let ok = check.ok;
    let checks = vec![check];
    let stdout = if options.json { doctor_render_json(&checks, ok, &[])? } else { doctor_render_text(&checks, ok, &[]) };
    Ok((i32::from(!ok), stdout))
}

fn doctor_migrate_xdg(dry_run: bool) -> DoctorCheckNative {
    let env = doctor_xdg_env();
    let items = doctor_xdg_migration_items(&env);
    let mut planned = 0_usize;
    let mut copied = 0_usize;
    let mut exists = 0_usize;
    let mut missing = 0_usize;
    let mut errors = 0_usize;
    for (source, destination) in items {
        match doctor_copy_xdg_item(&source, &destination, dry_run) {
            "dry-run" => planned += 1,
            "copied" => copied += 1,
            "exists" | "same" => exists += 1,
            "missing" => missing += 1,
            _ => errors += 1,
        }
    }
    let mode = if dry_run { "dry-run" } else { "apply" };
    let message = format!("{mode}; copied={copied}; planned={planned}; exists={exists}; missing={missing}; errors={errors}");
    if errors == 0 { doctor_info("xdg:migrate", &message) } else { doctor_error("xdg:migrate", &message, &[]) }
}

fn doctor_xdg_migration_items(env: &MawXdgEnv) -> Vec<(std::path::PathBuf, std::path::PathBuf)> {
    let config = maw_config_dir(env);
    let legacy = env.home_dir().join(".maw");
    let mut items = Vec::new();
    for name in ["plugins", "node_modules", "sessions", "state", "inbox", "schedules", "teams", "peers.json", "audit.jsonl"] {
        items.push((config.join(name), doctor_xdg_destination(env, name)));
        items.push((legacy.join(name), doctor_xdg_destination(env, name)));
    }
    items
}

fn doctor_xdg_destination(env: &MawXdgEnv, name: &str) -> std::path::PathBuf {
    match name {
        "plugins" | "inbox" => maw_data_path(env, &[name]),
        "node_modules" => maw_cache_path(env, &[name]),
        _ => maw_state_path(env, &[name]),
    }
}

fn doctor_copy_xdg_item(source: &std::path::Path, destination: &std::path::Path, dry_run: bool) -> &'static str {
    if !source.exists() { return "missing"; }
    if source == destination { return "same"; }
    if destination.exists() { return "exists"; }
    if dry_run { return "dry-run"; }
    if let Some(parent) = destination.parent() { if std::fs::create_dir_all(parent).is_err() { return "error"; } }
    if source.is_dir() { doctor_copy_dir(source, destination).map_or("error", |()| "copied") } else { std::fs::copy(source, destination).map_or("error", |_| "copied") }
}

fn doctor_copy_dir(source: &std::path::Path, destination: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let target = destination.join(entry.file_name());
        if entry.file_type()?.is_dir() { doctor_copy_dir(&entry.path(), &target)?; } else { std::fs::copy(entry.path(), target)?; }
    }
    Ok(())
}

fn doctor_forward_check(options: &DoctorOptions, checks: &[DoctorCheckNative], comparison: &[DoctorComparisonNative]) -> Option<DoctorCheckNative> {
    let target = options.forward.as_ref()?;
    let message = doctor_format_forward_message(checks, comparison, options.capture);
    if message.is_empty() { return Some(doctor_info("forward", &format!("no doctor issues to forward to {target}"))); }
    Some(doctor_warn("forward", &format!("native doctor prepared report for {target}; send manually"), &["maw hey <target> '<doctor report>'"]))
}

fn doctor_format_forward_message(checks: &[DoctorCheckNative], comparison: &[DoctorComparisonNative], capture: bool) -> String {
    let issues: Vec<&DoctorCheckNative> = checks.iter().filter(|check| !check.ok).collect();
    if issues.is_empty() { return String::new(); }
    let mut lines = vec![format!("maw doctor report: {} issue{}", issues.len(), doctor_plural(issues.len()))];
    lines.extend(issues.iter().map(|check| format!("- {}: {}", check.name, check.message)));
    for item in comparison.iter().filter(|item| item.status != "unchanged").take(6) { lines.push(format!("- {}: {} -> {} ({})", item.name, item.before, item.after, item.status)); }
    if capture { lines.push("capture: native doctor capture disabled for hermetic safety".to_owned()); }
    lines.join("\n")
}

fn doctor_render_text(checks: &[DoctorCheckNative], ok: bool, comparison: &[DoctorComparisonNative]) -> String {
    let mut out = String::new();
    let _ = writeln!(out);
    let _ = writeln!(out, "  {} maw doctor", if ok { "✓" } else { "✗" });
    for check in checks { doctor_write_check(&mut out, check); }
    doctor_write_comparison(&mut out, comparison);
    let remaining = checks.iter().filter(|check| !check.ok || check.severity == "warn").count();
    if remaining > 0 { let _ = writeln!(out, "\n  {remaining} issue{} remaining. Run suggested commands above to resolve.", doctor_plural(remaining)); }
    let _ = writeln!(out);
    out
}

fn doctor_write_check(out: &mut String, check: &DoctorCheckNative) {
    let icon = match check.severity { "info" => "✓", "warn" => "⚠", _ => "✗" };
    let _ = writeln!(out, "    {icon} {}: {}", check.name, check.message);
    for fix in &check.fixes { let _ = writeln!(out, "       → {fix}"); }
}

fn doctor_write_comparison(out: &mut String, comparison: &[DoctorComparisonNative]) {
    let changed: Vec<&DoctorComparisonNative> = comparison.iter().filter(|item| item.status != "unchanged").take(12).collect();
    if changed.is_empty() { return; }
    let _ = writeln!(out, "\n  ─── Before / After (since last doctor run) ───");
    for item in changed { let _ = writeln!(out, "    {}: {} → {} ({})", item.name, item.before, item.after, item.status); }
}

fn doctor_render_json(checks: &[DoctorCheckNative], ok: bool, comparison: &[DoctorComparisonNative]) -> Result<String, String> {
    let checks_json: Vec<serde_json::Value> = checks.iter().map(doctor_check_json).collect();
    let comparison_json: Vec<serde_json::Value> = comparison.iter().map(|item| serde_json::json!({"name": item.name, "before": item.before, "after": item.after, "status": item.status})).collect();
    serde_json::to_string_pretty(&serde_json::json!({"ok": ok, "checks": checks_json, "comparison": comparison_json})).map(|mut value| { value.push('\n'); value }).map_err(|error| error.to_string())
}

fn doctor_check_json(check: &DoctorCheckNative) -> serde_json::Value {
    serde_json::json!({"name": check.name, "ok": check.ok, "severity": check.severity, "message": check.message, "fix": check.fixes})
}

fn doctor_load_last_run() -> Option<BTreeMap<String, String>> {
    let raw = std::fs::read_to_string(doctor_last_path()).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let checks = parsed.get("checks")?.as_object()?;
    Some(checks.iter().map(|(key, value)| (key.clone(), value.as_str().unwrap_or("issue").to_owned())).collect())
}

fn doctor_persist_last_run(checks: &[DoctorCheckNative]) {
    let path = doctor_last_path();
    let Some(parent) = path.parent() else { return; };
    let snapshot = serde_json::json!({"timestamp": doctor_now_ms().to_string(), "checks": checks.iter().map(|check| (check.name.clone(), doctor_status(check))).collect::<BTreeMap<_, _>>()});
    if std::fs::create_dir_all(parent).is_ok() { let _ = std::fs::write(path, serde_json::to_string_pretty(&snapshot).unwrap_or_default()); }
}

fn doctor_compare_runs(previous: Option<&BTreeMap<String, String>>, checks: &[DoctorCheckNative]) -> Vec<DoctorComparisonNative> {
    let current: BTreeMap<String, String> = checks.iter().map(|check| (check.name.clone(), doctor_status(check))).collect();
    let Some(previous) = previous else { return Vec::new(); };
    let names: BTreeSet<String> = previous.keys().chain(current.keys()).cloned().collect();
    names.into_iter().map(|name| doctor_compare_one(&name, previous, &current)).collect()
}

fn doctor_compare_one(name: &str, previous: &BTreeMap<String, String>, current: &BTreeMap<String, String>) -> DoctorComparisonNative {
    let before = previous.get(name).cloned().unwrap_or_else(|| "missing".to_owned());
    let after = current.get(name).cloned().unwrap_or_else(|| "missing".to_owned());
    let status = if before == after { "unchanged" } else if before != "ok" && after == "ok" { "fixed" } else if before == "ok" && after != "ok" { "regressed" } else { "changed" };
    DoctorComparisonNative { name: name.to_owned(), before, after, status: status.to_owned() }
}

fn doctor_status(check: &DoctorCheckNative) -> String {
    if check.ok { "ok".to_owned() } else if check.severity == "error" { "error".to_owned() } else { "issue".to_owned() }
}

fn doctor_last_path() -> std::path::PathBuf { maw_state_path(&doctor_xdg_env(), &["doctor-last.json"]) }

fn doctor_from_peer_check(result: &maw_peer::PeerDoctorCheck, fix: &str) -> DoctorCheckNative {
    if result.ok { doctor_info(&result.name, &result.message) } else { doctor_warn(&result.name, &result.message, &[fix]) }
}

fn doctor_info(name: &str, message: &str) -> DoctorCheckNative { DoctorCheckNative { name: name.to_owned(), ok: true, severity: "info", message: message.to_owned(), fixes: Vec::new() } }

fn doctor_ok(name: &str, message: &str) -> DoctorCheckNative { doctor_info(name, message) }

fn doctor_warn(name: &str, message: &str, fixes: &[&str]) -> DoctorCheckNative { DoctorCheckNative { name: name.to_owned(), ok: false, severity: "warn", message: message.to_owned(), fixes: fixes.iter().map(|value| (*value).to_owned()).collect() } }

fn doctor_error(name: &str, message: &str, fixes: &[&str]) -> DoctorCheckNative { DoctorCheckNative { name: name.to_owned(), ok: false, severity: "error", message: message.to_owned(), fixes: fixes.iter().map(|value| (*value).to_owned()).collect() } }

fn doctor_gateway_kind(options: &DoctorOptions) -> String {
    options.gateway.clone().or_else(|| std::env::var("MAW_GATEWAY").ok()).or_else(doctor_config_gateway).unwrap_or_else(|| "bun".to_owned())
}

fn doctor_config_gateway() -> Option<String> {
    let raw = std::fs::read_to_string(maw_config_path(&doctor_xdg_env(), &["maw.config.json"])).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    parsed.get("gateway")?.as_str().map(str::to_owned)
}

fn doctor_default_port() -> u16 {
    std::env::var("MAW_PORT").ok().and_then(|value| value.parse::<u16>().ok()).filter(|value| *value > 0).unwrap_or(3456)
}

fn doctor_tcp_reachable(port: u16) -> bool {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_millis(150)).is_ok()
}

fn doctor_find_on_path(name: &str) -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).map(|dir| dir.join(name)).find(|candidate| candidate.is_file())
}

fn doctor_count_duplicate_peer_identities(store: &maw_peer::PeerStoreFile) -> (usize, Vec<String>) {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut duplicates = Vec::new();
    for (alias, peer) in &store.peers {
        let Some(identity) = &peer.identity else { continue; };
        let key = format!("{}:{}", identity.oracle, identity.node);
        if let Some(first) = seen.insert(key.clone(), alias.clone()) { duplicates.push(format!("{key} shared by {first} and {alias}")); }
    }
    (store.peers.len(), duplicates)
}

fn doctor_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = ["MAW_HOME", "MAW_CONFIG_DIR", "MAW_DATA_DIR", "MAW_STATE_DIR", "MAW_CACHE_DIR", "MAW_XDG", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME"];
    MawXdgEnv::with_vars(home, vars.into_iter().filter_map(|key| std::env::var(key).ok().map(|value| (key, value))))
}

fn doctor_peer_env() -> maw_peer::PeerStoreEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let vars = ["MAW_HOME", "MAW_STATE_DIR", "MAW_XDG", "XDG_STATE_HOME", "PEERS_FILE", "MAW_PEER_STALE_TTL_MS"];
    maw_peer::PeerStoreEnv::with_vars(home, vars.into_iter().filter_map(|key| std::env::var(key).ok().map(|value| (key, value))))
}

fn doctor_ghq_root() -> std::path::PathBuf {
    std::env::var_os("GHQ_ROOT").map_or_else(|| std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from), std::path::PathBuf::from)
}

fn doctor_count_worktree_dirs(ghq: &std::path::Path) -> usize {
    let mut count = 0_usize;
    for part in ["github.com", "gitlab.com"] {
        let root = ghq.join(part);
        if let Ok(orgs) = std::fs::read_dir(root) { count += orgs.flatten().filter(|entry| entry.path().is_dir()).count(); }
    }
    count
}

fn doctor_now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
}

fn doctor_plural(count: usize) -> &'static str { if count == 1 { "" } else { "s" } }

#[cfg(test)]
mod doctor_tests {
    use super::{doctor_parse_args, run_doctor_command, CliOutput, DISPATCH_65};
    use std::fs;

    struct DoctorEnvRestore { key: &'static str, value: Option<std::ffi::OsString> }

    impl DoctorEnvRestore { fn capture(key: &'static str) -> Self { Self { key, value: std::env::var_os(key) } } }

    impl Drop for DoctorEnvRestore {
        fn drop(&mut self) {
            if let Some(value) = self.value.take() { std::env::set_var(self.key, value); } else { std::env::remove_var(self.key); }
        }
    }

    fn doctor_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    fn doctor_temp_root(name: &str) -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let seq = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("maw-rs-doctor-{name}-{}-{seq}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("temp root");
        path
    }

    fn doctor_seed_env(name: &str) -> (std::sync::MutexGuard<'static, ()>, std::path::PathBuf, Vec<DoctorEnvRestore>) {
        let lock = super::env_test_lock().lock().expect("env lock");
        let temp = doctor_temp_root(name);
        let restores = ["HOME", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "XDG_CACHE_HOME", "MAW_CONFIG_DIR", "MAW_DATA_DIR", "MAW_STATE_DIR", "MAW_CACHE_DIR", "MAW_HOME", "MAW_XDG", "GHQ_ROOT", "TMUX", "PEERS_FILE", "MAW_PORT", "MAW_GATEWAY"]
            .into_iter().map(DoctorEnvRestore::capture).collect::<Vec<_>>();
        let home = temp.join(name);
        let config = home.join("xdg-config");
        let data = home.join("xdg-data");
        let state = home.join("xdg-state");
        let cache = home.join("xdg-cache");
        fs::create_dir_all(data.join("maw/plugins")).expect("plugins");
        fs::create_dir_all(state.join("maw")).expect("state");
        fs::create_dir_all(config.join("maw")).expect("config");
        fs::write(config.join("maw/maw.config.json"), "{\"node\":\"doctor-test\",\"gateway\":\"bun\"}\n").expect("config write");
        std::env::set_var("HOME", &home);
        std::env::set_var("XDG_CONFIG_HOME", &config);
        std::env::set_var("XDG_DATA_HOME", &data);
        std::env::set_var("XDG_STATE_HOME", &state);
        std::env::set_var("XDG_CACHE_HOME", &cache);
        std::env::set_var("MAW_XDG", "1");
        std::env::set_var("GHQ_ROOT", temp.join("ghq"));
        std::env::remove_var("TMUX");
        std::env::remove_var("MAW_HOME");
        std::env::remove_var("MAW_CONFIG_DIR");
        std::env::remove_var("MAW_DATA_DIR");
        std::env::remove_var("MAW_STATE_DIR");
        std::env::remove_var("MAW_CACHE_DIR");
        (lock, temp, restores)
    }

    #[test]
    fn doctor_dispatch_fragment_registers_native_doctor_once() {
        assert_eq!(DISPATCH_65.len(), 1);
        assert_eq!(DISPATCH_65[0].command, "doctor");
    }

    #[test]
    fn doctor_parse_flags_and_rejects_leading_dash_values() {
        let parsed = doctor_parse_args(&doctor_strings(&["--json", "--gateway=rust", "--port", "3457", "version"])).expect("parse");
        assert!(parsed.json);
        assert_eq!(parsed.gateway.as_deref(), Some("rust"));
        assert_eq!(parsed.port, Some(3457));
        assert_eq!(parsed.only.as_deref(), Some("version"));
        let err = doctor_parse_args(&doctor_strings(&["--forward", "--bad"])).expect_err("guard");
        assert!(err.contains("must not start"));
    }

    #[test]
    fn doctor_json_uses_seeded_xdg_without_real_home() {
        let (_lock, _temp, _restore) = doctor_seed_env("json-xdg");
        let output = run_doctor_command(&doctor_strings(&["--json", "xdg"]));
        assert_eq!(output.stderr, "");
        assert_eq!(output.code, 0);
        let parsed: serde_json::Value = serde_json::from_str(&output.stdout).expect("json");
        assert_eq!(parsed["ok"], true);
        assert_eq!(parsed["checks"][0]["name"], "xdg");
        assert!(parsed["checks"][0]["message"].as_str().expect("message").contains("json-xdg"));
    }

    #[test]
    fn doctor_fix_stale_is_hermetic_with_seeded_peer_store() {
        let (_lock, temp, _restore) = doctor_seed_env("stale-peers");
        let peers = temp.join("peers.json");
        std::env::set_var("PEERS_FILE", &peers);
        std::env::set_var("MAW_PEER_STALE_TTL_MS", "1");
        fs::write(&peers, r#"{"version":1,"peers":{"old":{"url":"http://127.0.0.1:1","addedAt":"1970-01-01T00:00:00.000Z","lastSeen":"1970-01-01T00:00:00.000Z"}}}"#).expect("peers");
        let output = run_doctor_command(&doctor_strings(&["--json", "--fix-stale"]));
        assert_eq!(output, CliOutput { code: 0, stdout: output.stdout.clone(), stderr: String::new() });
        let parsed: serde_json::Value = serde_json::from_str(&output.stdout).expect("json");
        assert_eq!(parsed["checks"][0]["name"], "peers:fix-stale");
        assert!(parsed["checks"][0]["message"].as_str().expect("message").contains("removed 1 stale peer"));
    }

    #[test]
    fn doctor_xdg_migrate_dry_run_has_golden_shape() {
        let (_lock, _temp, _restore) = doctor_seed_env("xdg-plan");
        let output = run_doctor_command(&doctor_strings(&["--json", "--fix-xdg", "--dry-run", "xdg"]));
        assert_eq!(output.code, 0);
        assert_eq!(output.stdout, include_str!("../../tests/fixtures/native-doctor/xdg-migrate-dry-run.json"));
    }
}
