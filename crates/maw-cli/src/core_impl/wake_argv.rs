// The wake command line: parsing it, and refusing what must not be accepted.
//
// Every value that reaches tmux or the filesystem is validated here -- session
// and window names, repo slugs, layouts, cwds -- so a target can never smuggle a
// flag or a path traversal through. Kept apart from resolution so it is obvious
// which checks a new option has to pass.

type WakeEqualsSetter = fn(&mut WakeOptionsNative, &str) -> Result<(), String>;

fn wake_parse_args(argv: &[String]) -> Result<WakeOptionsNative, String> {
    let mut options = wake_default_options();
    let mut positionals = Vec::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        if let Some(consumed) = wake_parse_value_arg(argv, index, &mut options)? { index += consumed; continue; }
        if wake_parse_bool_arg(arg, &mut options)? { index += 1; continue; }
        if arg.starts_with('-') { return Err(format!("wake: unknown argument {arg}")); }
        wake_validate_target_value(arg, "target")?;
        positionals.push(arg.clone());
        index += 1;
    }
    wake_finalize_options(options, &positionals)
}

fn wake_default_options() -> WakeOptionsNative {
    WakeOptionsNative {
        target: String::new(), task: None, wt: None, prompt: None, repo: None, issue: None, pr: None,
        incubate: None, parent: None, peer: None, layout: None, from: None, snapshot: None, engine: None,
        engine_command: None,
        name: None, repo_path: None, on_ready: Vec::new(), all: false, all_local: false, attach: true, dry_run: false, fresh: false,
        from_snapshot: false, kill: false, list: false, main: false, new_window: false, no_attach: false,
        pick: false, resume: false, solo: false, split: false, bud: false, channels: false, wait: false, yes: false,
    }
}

fn wake_parse_value_arg(argv: &[String], index: usize, options: &mut WakeOptionsNative) -> Result<Option<usize>, String> {
    let arg = &argv[index];
    let consumed = match arg.as_str() {
        "--task" => { options.task = Some(wake_take_text(argv, index, "--task")?); 2 }
        "--wt" => { options.wt = Some(wake_take_value(argv, index, "--wt", wake_validate_slug)?); 2 }
        "--prompt" => { options.prompt = Some(wake_take_text(argv, index, "--prompt")?); 2 }
        "--repo" => { options.repo = Some(wake_take_value(argv, index, "--repo", wake_validate_repo)?); 2 }
        "--issue" => { options.issue = Some(wake_take_value(argv, index, "--issue", wake_validate_issue)?); 2 }
        "--pr" => { options.pr = Some(wake_take_value(argv, index, "--pr", wake_validate_issue)?); 2 }
        "--incubate" => { options.incubate = Some(wake_take_value(argv, index, "--incubate", wake_validate_repo)?); 2 }
        "--parent" | "--session" => { options.parent = Some(wake_take_value(argv, index, arg, wake_validate_target_value)?); 2 }
        "--peer" | "--from" => { wake_set_peer_or_from(options, arg, &wake_take_value(argv, index, arg, wake_validate_target_value)?); 2 }
        "--layout" => { options.layout = Some(wake_take_value(argv, index, "--layout", wake_validate_layout)?); 2 }
        "--snapshot" => { options.snapshot = Some(wake_take_value(argv, index, "--snapshot", wake_validate_target_value)?); 2 }
        "-e" | "--engine" => { options.engine = Some(wake_take_value(argv, index, arg, wake_validate_target_value)?); 2 }
        "--engine-cmd" => { options.engine_command = Some(wake_take_value(argv, index, "--engine-cmd", wake_validate_command)?); 2 }
        "--name" => { options.name = Some(wake_take_value(argv, index, "--name", wake_validate_slug)?); 2 }
        "--repo-path" => { options.repo_path = Some(std::path::PathBuf::from(wake_take_value(argv, index, "--repo-path", wake_validate_target_value)?)); 2 }
        "--on-ready" => { options.on_ready.push(wake_take_text(argv, index, "--on-ready")?); 2 }
        _ => return wake_parse_equals_arg(arg, options),
    };
    Ok(Some(consumed))
}

fn wake_parse_equals_arg(arg: &str, options: &mut WakeOptionsNative) -> Result<Option<usize>, String> {
    for (flag, setter) in wake_equals_setters() {
        if let Some(value) = arg.strip_prefix(flag) {
            setter(options, value)?;
            return Ok(Some(1));
        }
    }
    Ok(None)
}

fn wake_equals_setters() -> Vec<(&'static str, WakeEqualsSetter)> {
    vec![
        ("--task=", |o, v| { wake_validate_text(v, "--task")?; o.task = Some(v.to_owned()); Ok(()) }),
        ("--wt=", |o, v| { wake_validate_slug(v, "--wt")?; o.wt = Some(v.to_owned()); Ok(()) }),
        ("--prompt=", |o, v| { wake_validate_text(v, "--prompt")?; o.prompt = Some(v.to_owned()); Ok(()) }),
        ("--repo=", |o, v| { wake_validate_repo(v, "--repo")?; o.repo = Some(v.to_owned()); Ok(()) }),
        ("--issue=", |o, v| { wake_validate_issue(v, "--issue")?; o.issue = Some(v.to_owned()); Ok(()) }),
        ("--pr=", |o, v| { wake_validate_issue(v, "--pr")?; o.pr = Some(v.to_owned()); Ok(()) }),
        ("--incubate=", |o, v| { wake_validate_repo(v, "--incubate")?; o.incubate = Some(v.to_owned()); Ok(()) }),
        ("--parent=", |o, v| { wake_validate_target_value(v, "--parent")?; o.parent = Some(v.to_owned()); Ok(()) }),
        ("--peer=", |o, v| { wake_validate_target_value(v, "--peer")?; o.peer = Some(v.to_owned()); Ok(()) }),
        ("--from=", |o, v| { wake_validate_target_value(v, "--from")?; o.from = Some(v.to_owned()); Ok(()) }),
        ("--engine-cmd=", |o, v| { wake_validate_command(v, "--engine-cmd")?; o.engine_command = Some(v.to_owned()); Ok(()) }),
        ("--layout=", |o, v| { wake_validate_layout(v, "--layout")?; o.layout = Some(v.to_owned()); Ok(()) }),
        ("--snapshot=", |o, v| { wake_validate_target_value(v, "--snapshot")?; o.snapshot = Some(v.to_owned()); Ok(()) }),
        ("--engine=", |o, v| { wake_validate_target_value(v, "--engine")?; o.engine = Some(v.to_owned()); Ok(()) }),
        ("--name=", |o, v| { wake_validate_slug(v, "--name")?; o.name = Some(v.to_owned()); Ok(()) }),
        ("--on-ready=", |o, v| { wake_validate_text(v, "--on-ready")?; o.on_ready.push(v.to_owned()); Ok(()) }),
    ]
}

fn wake_parse_bool_arg(arg: &str, options: &mut WakeOptionsNative) -> Result<bool, String> {
    match arg {
        "--all" => options.all = true,
        "all" => { options.all = true; if options.target.is_empty() { "all".clone_into(&mut options.target); } }
        "--all-local" => options.all_local = true,
        "--attach" | "-a" => { options.attach = true; options.no_attach = false; }
        "--no-attach" => { options.attach = false; options.no_attach = true; }
        "--dry-run" => options.dry_run = true,
        "--fresh" => options.fresh = true,
        "--from-snapshot" => options.from_snapshot = true,
        "--kill" => options.kill = true,
        "--list" => options.list = true,
        "--main" => { options.main = true; options.solo = true; }
        "--new" => options.new_window = true,
        "--pick" => options.pick = true,
        "--resume" => options.resume = true,
        "--solo" => options.solo = true,
        "--split" => options.split = true,
        "--bud" => options.bud = true,
        "--channels" => options.channels = true,
        "--wait" => options.wait = true,
        "--yes" | "-y" => options.yes = true,
        "-h" | "--help" => return Err(wake_usage()),
        _ => return Ok(false),
    }
    Ok(true)
}

fn wake_set_peer_or_from(options: &mut WakeOptionsNative, flag: &str, value: &str) {
    if flag == "--peer" { options.peer = Some(value.to_owned()); } else { options.from = Some(value.to_owned()); }
}

fn wake_take_value(
    argv: &[String],
    index: usize,
    flag: &str,
    validate: fn(&str, &str) -> Result<(), String>,
) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("wake: missing {flag} value"))?;
    validate(value, flag)?;
    Ok(value.clone())
}

fn wake_take_text(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let value = argv.get(index + 1).ok_or_else(|| format!("wake: missing {flag} value"))?;
    wake_validate_text(value, flag)?;
    Ok(value.clone())
}

fn wake_finalize_options(mut options: WakeOptionsNative, positionals: &[String]) -> Result<WakeOptionsNative, String> {
    if options.all && positionals.is_empty() { return Ok(options); }
    if positionals.len() != 1 { return Err(wake_usage()); }
    options.target.clone_from(&positionals[0]);
    Ok(options)
}

fn wake_usage() -> String {
    "usage: maw wake <target|all> [--task <slug>|--wt <slug>] [--repo <org/repo>] [--prompt <text>] [--on-ready <cmd>] [--all --all-local --attach|-a --no-attach --dry-run --fresh --from-snapshot --kill --layout <nested|legacy> --list --main --new --parent <session> --peer <node> --pick --resume --snapshot <id> --solo --split --yes|-y]".to_owned()
}

fn wake_help_value_flags() -> &'static [&'static str] {
    &[
        "--task",
        "--wt",
        "--prompt",
        "--repo",
        "--issue",
        "--pr",
        "--incubate",
        "--parent",
        "--session",
        "--peer",
        "--from",
        "--layout",
        "--snapshot",
        "-e",
        "--engine",
        "--engine-cmd",
        "--name",
        "--repo-path",
        "--on-ready",
    ]
}

fn wake_validate_target_value(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') { return Err(format!("wake: {label} must not start with '-'")); }
    if value.contains('\0') || value.contains('\n') || value.contains('\r') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

/// A launch line: spaces and shell-ish characters are the point, so only the
/// separators that would let it break out of the single command are rejected.
fn wake_validate_command(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() { return Err(format!("wake: {label} must not be empty")); }
    if value.starts_with('-') { return Err(format!("wake: {label} must not start with '-'")); }
    if value.contains('\0') || value.contains('\n') || value.contains('\r') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.starts_with('-') { return Err(format!("wake: {label} must not start with '-'")); }
    if value.contains('\0') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_slug(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if !value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.' | '/')) {
        return Err(format!("wake: invalid {label}"));
    }
    Ok(())
}

fn wake_validate_repo(value: &str, label: &str) -> Result<(), String> {
    wake_validate_slug(value, label)?;
    if value.contains("..") { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_issue(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if !value.chars().all(|ch| ch.is_ascii_digit() || ch == '#') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_layout(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if matches!(value, "nested" | "legacy") { Ok(()) } else { Err(format!("wake: invalid {label}")) }
}

fn wake_validate_tmux_name(value: &str, label: &str) -> Result<(), String> {
    wake_validate_target_value(value, label)?;
    if value.contains(':') { return Err(format!("wake: invalid {label}")); }
    Ok(())
}

fn wake_validate_tmux_target(value: &str) -> Result<(), String> {
    wake_validate_target_value(value, "tmux target")?;
    if !value.contains(':') { return Err("wake: invalid tmux target".to_owned()); }
    Ok(())
}

fn wake_validate_cwd(path: &std::path::Path) -> Result<(), String> {
    if !path.is_dir() { return Err(format!("wake: cwd missing: {}", path.display())); }
    Ok(())
}
