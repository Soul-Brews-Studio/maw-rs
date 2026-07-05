fn promote_validate_tmux_target(value: &str, label: &str) -> Result<(), String> {
    promote_validate_target(value, label)?;
    if value.contains(':') {
        for (index, part) in value.split(':').enumerate() {
            if part.is_empty() && index == 1 && value.ends_with(':') {
                continue;
            }
            if !part.is_empty() {
                promote_validate_tmux_name(part, label)?;
            }
        }
    } else {
        promote_validate_tmux_name(value, label)?;
    }
    Ok(())
}

fn promote_windows_only_placeholder(windows: &[maw_tmux::TmuxWindow]) -> bool {
    windows.is_empty() || windows.iter().all(|window| window.name == PROMOTE_PLACEHOLDER)
}

fn promote_windows_have_foreign(windows: &[maw_tmux::TmuxWindow]) -> bool {
    windows.iter().any(|window| window.name != PROMOTE_PLACEHOLDER)
}

fn promote_rollback_after_failure(tmux: &mut impl PromoteTmuxNative, ready: &PromoteResolvedNative, state: &PromoteMutationStateNative, reason: &str) {
    if !state.created_dst_by_this_run {
        return;
    }
    match tmux.promote_list_windows(&ready.dst_session) {
        Ok(windows) if promote_windows_have_foreign(&windows) => {
            let _ = tmux.promote_kill_window(&ready.placeholder_target());
        }
        _ => promote_rollback_owned_placeholder_session(tmux, ready, reason),
    }
}

fn promote_rollback_after_verify_miss(
    tmux: &mut impl PromoteTmuxNative,
    ready: &PromoteResolvedNative,
    state: &PromoteMutationStateNative,
    dst_windows: &[maw_tmux::TmuxWindow],
) {
    if !state.created_dst_by_this_run {
        return;
    }
    if promote_windows_have_foreign(dst_windows) {
        let _ = tmux.promote_kill_window(&ready.placeholder_target());
    } else {
        promote_rollback_owned_placeholder_session(tmux, ready, "move verification failure");
    }
}

fn promote_cleanup_after_unknown_verify_failure(tmux: &mut impl PromoteTmuxNative, ready: &PromoteResolvedNative, state: &PromoteMutationStateNative) {
    if state.created_dst_by_this_run {
        let _ = tmux.promote_kill_window(&ready.placeholder_target());
    }
}

fn promote_rollback_owned_placeholder_session(tmux: &mut impl PromoteTmuxNative, ready: &PromoteResolvedNative, _reason: &str) {
    if tmux.promote_kill_session(&ready.dst_session).is_err() {
        let _ = tmux.promote_kill_window(&ready.placeholder_target());
    }
}

fn promote_render_success(resolved: &PromoteResolvedNative) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "  \u{001b}[32m✓\u{001b}[0m promoted — {}:{} → {}:{}", resolved.src_session, resolved.src_window, resolved.dst_session, resolved.src_window);
    let _ = writeln!(out, "      \u{001b}[90m↻ undo: tmux move-window -s {}:{} -t {}:\u{001b}[0m", resolved.dst_session, resolved.src_window, resolved.src_session);
    if resolved.attach {
        let _ = writeln!(out, "      \u{001b}[33m⚠\u{001b}[0m promote succeeded; --attach deferred (switch-client manual): tmux switch-client -t {}", resolved.dst_session);
    }
    out
}

fn promote_flag_like(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a promote target.\n  {PROMOTE_USAGE}")
}

fn preflight_run_command(argv: &[String]) -> CliOutput {
    match preflight_parse_args(argv).and_then(|options| preflight_run(&options)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => preflight_error(&message),
    }
}

fn preflight_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn preflight_parse_args(argv: &[String]) -> Result<PreflightOptionsNative, String> {
    let mut path = None::<std::path::PathBuf>;
    let mut json = false;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" | "help" => return Err(PREFLIGHT_USAGE.to_owned()),
            "--" => return Err("preflight: -- separator is not allowed".to_owned()),
            "--json" => json = true,
            value if value.starts_with('-') => return Err(preflight_flag_like(value)),
            value => preflight_set_path(&mut path, value)?,
        }
    }
    Ok(PreflightOptionsNative { path: path.unwrap_or_else(|| std::path::PathBuf::from(".")), json })
}

fn preflight_set_path(slot: &mut Option<std::path::PathBuf>, value: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(PREFLIGHT_USAGE.to_owned());
    }
    *slot = Some(preflight_validate_path(value)?);
    Ok(())
}

fn preflight_validate_path(value: &str) -> Result<std::path::PathBuf, String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains('\0') {
        return Err("preflight path must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.split('/').any(|part| part == "..") {
        return Err("preflight path must not contain .. segments".to_owned());
    }
    Ok(std::path::PathBuf::from(value))
}

fn preflight_flag_like(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a preflight path.\n  {PREFLIGHT_USAGE}")
}

fn preflight_run(options: &PreflightOptionsNative) -> Result<String, String> {
    if !options.path.is_dir() {
        return Err(format!("preflight: not a directory: {}", options.path.display()));
    }
    let inside = preflight_git(&options.path, &["rev-parse", "--is-inside-work-tree"]).unwrap_or_default();
    let clean = preflight_git(&options.path, &["status", "--porcelain"]).unwrap_or_else(|_| "dirty".to_owned()).trim().is_empty();
    let ok = inside.trim() == "true" && clean;
    if options.json {
        return Ok(format!("{{\"command\":\"preflight\",\"path\":{},\"git\":{},\"clean\":{},\"ok\":{ok}}}\n", json_string(&options.path.display().to_string()), inside.trim() == "true", clean));
    }
    Ok(format!("preflight {}: git={} clean={} ok={}\n", options.path.display(), inside.trim() == "true", clean, ok))
}

fn preflight_git(path: &std::path::Path, args: &[&str]) -> Result<String, String> {
    preflight_validate_git_args(args)?;
    let output = std::process::Command::new("git").arg("-C").arg(path).args(args).output().map_err(|error| format!("preflight: git failed: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn preflight_validate_git_args(args: &[&str]) -> Result<(), String> {
    match args {
        ["rev-parse", "--is-inside-work-tree"] | ["status", "--porcelain"] => Ok(()),
        _ => Err("preflight: refused unexpected git argument shape".to_owned()),
    }
}

fn snapshots_run_command(argv: &[String]) -> CliOutput {
    match snapshots_parse_args(argv).and_then(|options| snapshots_run(&options)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => snapshots_error(&message),
    }
}

fn snapshots_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn snapshots_parse_args(argv: &[String]) -> Result<SnapshotsOptionsNative, String> {
    let mut words = Vec::<String>::new();
    let mut json = false;
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" | "help" => return Err(SNAPSHOTS_USAGE.to_owned()),
            "--" => return Err("snapshots: -- separator is not allowed".to_owned()),
            "--json" => json = true,
            value if value.starts_with('-') => return Err(snapshots_flag_like(value)),
            value => words.push(snapshots_validate_name(value)?),
        }
    }
    let action = snapshots_action(&words)?;
    Ok(SnapshotsOptionsNative { action, json })
}

fn snapshots_action(words: &[String]) -> Result<SnapshotsActionNative, String> {
    match words {
        [] => Ok(SnapshotsActionNative::List),
        [one] if one == "list" => Ok(SnapshotsActionNative::List),
        [one] if one == "create" => Ok(SnapshotsActionNative::Create { name: snapshots_default_name() }),
        [one] => Ok(SnapshotsActionNative::Show { name: one.clone() }),
        [cmd, name] if cmd == "create" => Ok(SnapshotsActionNative::Create { name: name.clone() }),
        [cmd, name] if cmd == "show" => Ok(SnapshotsActionNative::Show { name: name.clone() }),
        _ => Err(SNAPSHOTS_USAGE.to_owned()),
    }
}

fn snapshots_validate_name(value: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains("..") {
        return Err("snapshots name must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_')) {
        return Err("snapshots name must contain only ascii letters, digits, - or _".to_owned());
    }
    Ok(value.to_owned())
}

fn snapshots_flag_like(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a snapshot name.\n  {SNAPSHOTS_USAGE}")
}

fn snapshots_run(options: &SnapshotsOptionsNative) -> Result<String, String> {
    let dir = snapshots_dir();
    std::fs::create_dir_all(&dir).map_err(|error| format!("snapshots: create state dir: {error}"))?;
    match &options.action {
        SnapshotsActionNative::List => snapshots_list(&dir, options.json),
        SnapshotsActionNative::Create { name } => snapshots_create(&dir, name, options.json),
        SnapshotsActionNative::Show { name } => snapshots_show(&dir, name, options.json),
    }
}

