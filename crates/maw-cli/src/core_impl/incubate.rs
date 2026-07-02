const DISPATCH_53: &[DispatcherEntry] = &[ DispatcherEntry { command: "incubate", handler: Handler::Sync(run_incubate_command) } ];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IncubateMode {
    Default,
    Flash,
    Contribute,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct IncubateOptions {
    source: String,
    stem: Option<String>,
    trigger: Option<String>,
    no_trigger: bool,
    flash: bool,
    contribute: bool,
    from: Option<String>,
    from_repo: Option<String>,
    org: Option<String>,
    issue: Option<u64>,
    note: Option<String>,
    nickname: Option<String>,
    fast: bool,
    root: bool,
    blank: bool,
    seed: bool,
    split: bool,
    dry_run: bool,
    signal_on_birth: bool,
    force: bool,
    track_vault: bool,
    sync_peers: bool,
}

fn incubate_usage() -> &'static str {
    "usage: maw incubate <source-repo> [--stem <name>] [--from <oracle>] [--root] [--seed] [--org <org>] [--note <text>] [--nickname <pretty>] [--fast] [--split] [--dry-run] [--flash | --contribute] [--no-trigger] [--trigger <text>]"
}

fn run_incubate_command(argv: &[String]) -> CliOutput {
    match incubate_run(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn incubate_run(argv: &[String]) -> Result<String, String> {
    let options = incubate_parse_args(argv)?;
    incubate_resolve_mode(options.flash, options.contribute)?;
    let stem = options
        .stem
        .clone()
        .unwrap_or_else(|| incubate_derive_stem_from_source(&options.source));
    incubate_validate_target_arg(&stem, "stem")?;
    let trigger = if options.no_trigger { None } else { Some(incubate_build_skill_command(&options)?) };

    let mut stdout = incubate_run_bud(&stem, &options)?;

    if options.dry_run {
        if let Some(trigger) = trigger {
            let _ = writeln!(stdout, "  \x1b[36m⬡\x1b[0m [dry-run] would send \x1b[33m{trigger}\x1b[0m to {stem}");
        } else {
            stdout.push_str("  \x1b[36m⬡\x1b[0m [dry-run] --no-trigger: would NOT fire /incubate\n");
        }
        return Ok(stdout);
    }

    let Some(trigger) = trigger else {
        stdout.push_str("  \x1b[90m○\x1b[0m --no-trigger: bud + wake done, skipping /incubate\n");
        return Ok(stdout);
    };

    if let Some(target) = incubate_resolve_tmux_target(&stem) {
        let _ = writeln!(stdout, "  \x1b[36m🔔\x1b[0m firing \x1b[33m{trigger}\x1b[0m → {stem}");
        let mut tmux = TmuxClient::local();
        match tmux.send_text(&target, &trigger) {
            Ok(_) => stdout.push_str("  \x1b[32m✓\x1b[0m incubation dispatched\n"),
            Err(error) => {
                let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m send-text failed: {error}");
                let _ = writeln!(stdout, "  \x1b[90m  try manually: maw send-text {stem} '{trigger}'\x1b[0m");
            }
        }
    } else {
        let _ = writeln!(stdout, "  \x1b[33m⚠\x1b[0m could not resolve {stem} after wake — skipping {trigger}");
        let _ = writeln!(stdout, "  \x1b[90m  try manually: maw send-text {stem} '{trigger}'\x1b[0m");
    }
    Ok(stdout)
}

#[allow(clippy::too_many_lines)]
fn incubate_parse_args(argv: &[String]) -> Result<IncubateOptions, String> {
    let mut options = IncubateOptions::default();
    let mut positionals = Vec::<String>::new();
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index) {
        match arg.as_str() {
            "--help" | "-h" => return Err(incubate_usage().to_owned()),
            "--stem" => {
                options.stem = Some(incubate_required_value(argv, index, "--stem")?);
                index += 2;
            }
            value if value.starts_with("--stem=") => {
                options.stem = Some(value["--stem=".len()..].to_owned());
                index += 1;
            }
            "--trigger" => {
                options.trigger = Some(incubate_required_value(argv, index, "--trigger")?);
                index += 2;
            }
            value if value.starts_with("--trigger=") => {
                options.trigger = Some(value["--trigger=".len()..].to_owned());
                index += 1;
            }
            "--no-trigger" => {
                options.no_trigger = true;
                index += 1;
            }
            "--flash" => {
                options.flash = true;
                index += 1;
            }
            "--contribute" => {
                options.contribute = true;
                index += 1;
            }
            "--from" => {
                options.from = Some(incubate_required_value(argv, index, "--from")?);
                index += 2;
            }
            value if value.starts_with("--from=") => {
                options.from = Some(value["--from=".len()..].to_owned());
                index += 1;
            }
            "--from-repo" => {
                options.from_repo = Some(incubate_required_value(argv, index, "--from-repo")?);
                index += 2;
            }
            value if value.starts_with("--from-repo=") => {
                options.from_repo = Some(value["--from-repo=".len()..].to_owned());
                index += 1;
            }
            "--org" => {
                options.org = Some(incubate_required_value(argv, index, "--org")?);
                index += 2;
            }
            value if value.starts_with("--org=") => {
                options.org = Some(value["--org=".len()..].to_owned());
                index += 1;
            }
            "--issue" => {
                let value = incubate_required_value(argv, index, "--issue")?;
                options.issue = Some(incubate_parse_issue(&value)?);
                index += 2;
            }
            value if value.starts_with("--issue=") => {
                options.issue = Some(incubate_parse_issue(&value["--issue=".len()..])?);
                index += 1;
            }
            "--note" => {
                options.note = Some(incubate_required_value(argv, index, "--note")?);
                index += 2;
            }
            value if value.starts_with("--note=") => {
                options.note = Some(value["--note=".len()..].to_owned());
                index += 1;
            }
            "--nickname" => {
                options.nickname = Some(incubate_required_value(argv, index, "--nickname")?);
                index += 2;
            }
            value if value.starts_with("--nickname=") => {
                options.nickname = Some(value["--nickname=".len()..].to_owned());
                index += 1;
            }
            "--fast" => { options.fast = true; index += 1; }
            "--root" => { options.root = true; index += 1; }
            "--blank" => { options.blank = true; index += 1; }
            "--seed" => { options.seed = true; index += 1; }
            "--split" => { options.split = true; index += 1; }
            "--dry-run" => { options.dry_run = true; index += 1; }
            "--signal-on-birth" => { options.signal_on_birth = true; index += 1; }
            "--force" => { options.force = true; index += 1; }
            "--track-vault" => { options.track_vault = true; index += 1; }
            "--sync-peers" => { options.sync_peers = true; index += 1; }
            value if value.starts_with('-') => return Err(format!("incubate: unknown argument {value}")),
            value => {
                positionals.push(value.to_owned());
                index += 1;
            }
        }
    }
    if positionals.len() != 1 {
        return Err(incubate_usage().to_owned());
    }
    options.source = positionals.remove(0);
    incubate_validate_path_arg(&options.source, "source repo")?;
    if let Some(stem) = &options.stem {
        incubate_validate_target_arg(stem, "stem")?;
    }
    incubate_validate_optional_path_arg(options.from.as_deref(), "from")?;
    incubate_validate_optional_path_arg(options.from_repo.as_deref(), "from-repo")?;
    incubate_validate_optional_path_arg(options.org.as_deref(), "org")?;
    incubate_validate_optional_path_arg(options.trigger.as_deref(), "trigger")?;
    Ok(options)
}

fn incubate_required_value(argv: &[String], index: usize, flag: &str) -> Result<String, String> {
    let Some(value) = argv.get(index + 1) else {
        return Err(format!("incubate: {flag} requires a value"));
    };
    if value.starts_with('-') {
        return Err(format!("incubate: {flag} requires a value"));
    }
    Ok(value.clone())
}

fn incubate_parse_issue(value: &str) -> Result<u64, String> {
    incubate_validate_path_arg(value, "issue")?;
    let issue = value
        .parse::<u64>()
        .map_err(|_| format!("incubate: invalid --issue value {value}"))?;
    if issue == 0 {
        return Err("incubate: invalid --issue value 0".to_owned());
    }
    Ok(issue)
}

fn incubate_resolve_mode(flash: bool, contribute: bool) -> Result<IncubateMode, String> {
    if flash && contribute {
        return Err("--flash and --contribute are mutually exclusive".to_owned());
    }
    if flash {
        Ok(IncubateMode::Flash)
    } else if contribute {
        Ok(IncubateMode::Contribute)
    } else {
        Ok(IncubateMode::Default)
    }
}

fn incubate_derive_stem_from_source(source: &str) -> String {
    let mut name = source.rsplit('/').next().unwrap_or(source).to_owned();
    if let Some(stripped) = name.strip_suffix(".git") {
        name = stripped.to_owned();
    }
    name
}

fn incubate_build_skill_command(options: &IncubateOptions) -> Result<String, String> {
    if let Some(trigger) = &options.trigger {
        incubate_validate_path_arg(trigger, "trigger")?;
        return Ok(trigger.clone());
    }
    let mode = incubate_resolve_mode(options.flash, options.contribute)?;
    let mut command = String::from("/incubate ");
    command.push_str(&options.source);
    match mode {
        IncubateMode::Default => {}
        IncubateMode::Flash => command.push_str(" --flash"),
        IncubateMode::Contribute => command.push_str(" --contribute"),
    }
    Ok(command)
}

fn incubate_run_bud(stem: &str, options: &IncubateOptions) -> Result<String, String> {
    let bud_options = incubate_bud_options(stem, options)?;
    let mut gh = BudSystemGhGit;
    let mut fs = BudSystemFs;
    let mut wake = BudSystemWake;
    let mut http = BudSystemHttp;
    bud_run_options(&bud_options, &mut gh, &mut fs, &mut wake, &mut http)
        .map_err(|message| format!("incubate: bud failed: {message}"))
}

fn incubate_bud_options(stem: &str, options: &IncubateOptions) -> Result<BudOptions, String> {
    incubate_validate_target_arg(stem, "stem")?;
    let issue = options
        .issue
        .map(|issue| {
            u32::try_from(issue)
                .map_err(|_| format!("incubate: --issue value {issue} is too large for bud"))
        })
        .transpose()?;
    Ok(BudOptions {
        name: Some(stem.to_owned()),
        from: options.from.clone(),
        from_repo: options.from_repo.clone(),
        stem: None,
        org: options.org.clone(),
        repo: Some(options.source.clone()),
        issue,
        issue_repo: None,
        note: options.note.clone(),
        nickname: options.nickname.clone(),
        fast: options.fast,
        root: options.root,
        dry_run: options.dry_run,
        pr: false,
        split: options.split,
        scaffold_only: false,
        seed: options.seed,
        blank: options.blank,
        signal_on_birth: options.signal_on_birth,
        force: options.force,
        track_vault: options.track_vault,
        sync_peers: options.sync_peers,
        parent_session_id: None,
        session_id: None,
    })
}

#[cfg(test)]
fn incubate_bud_args(stem: &str, options: &IncubateOptions) -> Result<Vec<String>, String> {
    incubate_validate_target_arg(stem, "stem")?;
    let mut args = vec!["bud".to_owned(), stem.to_owned(), "--repo".to_owned(), options.source.clone()];
    incubate_push_option(&mut args, "--from", options.from.as_deref())?;
    incubate_push_option(&mut args, "--from-repo", options.from_repo.as_deref())?;
    incubate_push_option(&mut args, "--org", options.org.as_deref())?;
    if let Some(issue) = options.issue {
        args.push("--issue".to_owned());
        args.push(issue.to_string());
    }
    incubate_push_option(&mut args, "--note", options.note.as_deref())?;
    incubate_push_option(&mut args, "--nickname", options.nickname.as_deref())?;
    incubate_push_bool(&mut args, "--fast", options.fast);
    incubate_push_bool(&mut args, "--root", options.root);
    incubate_push_bool(&mut args, "--blank", options.blank);
    incubate_push_bool(&mut args, "--seed", options.seed);
    incubate_push_bool(&mut args, "--split", options.split);
    incubate_push_bool(&mut args, "--dry-run", options.dry_run);
    incubate_push_bool(&mut args, "--signal-on-birth", options.signal_on_birth);
    incubate_push_bool(&mut args, "--force", options.force);
    incubate_push_bool(&mut args, "--track-vault", options.track_vault);
    incubate_push_bool(&mut args, "--sync-peers", options.sync_peers);
    Ok(args)
}

#[cfg(test)]
fn incubate_push_option(args: &mut Vec<String>, flag: &str, value: Option<&str>) -> Result<(), String> {
    if let Some(value) = value {
        incubate_validate_path_arg(value, flag)?;
        args.push(flag.to_owned());
        args.push(value.to_owned());
    }
    Ok(())
}

#[cfg(test)]
fn incubate_push_bool(args: &mut Vec<String>, flag: &str, enabled: bool) {
    if enabled {
        args.push(flag.to_owned());
    }
}

fn incubate_resolve_tmux_target(stem: &str) -> Option<String> {
    incubate_validate_target_arg(stem, "stem").ok()?;
    let mut tmux = TmuxClient::local();
    let config = load_hey_config();
    let sessions = route_sessions_from_tmux(&mut tmux);
    match resolve_route_target(stem, &config.route, &sessions) {
        RouteResult::Local { target } | RouteResult::SelfNode { target } => Some(target),
        RouteResult::Peer { .. } | RouteResult::Error { .. } => {
            let names = tmux.list_session_names();
            if names.iter().any(|name| name == stem) {
                Some(stem.to_owned())
            } else {
                None
            }
        }
    }
}

fn incubate_validate_optional_path_arg(value: Option<&str>, label: &str) -> Result<(), String> {
    if let Some(value) = value {
        incubate_validate_path_arg(value, label)?;
    }
    Ok(())
}

fn incubate_validate_path_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err(format!("incubate: {label} must be non-empty, unpadded, not start with '-', and contain no control characters"));
    }
    Ok(())
}

fn incubate_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    incubate_validate_path_arg(value, label)?;
    if value.chars().any(char::is_whitespace) {
        return Err(format!("incubate: {label} must not contain whitespace"));
    }
    Ok(())
}

#[cfg(test)]
mod incubate_tests {
    use super::{
        incubate_bud_args, incubate_bud_options, incubate_build_skill_command,
        incubate_derive_stem_from_source, incubate_parse_args, incubate_resolve_mode,
        IncubateMode,
    };

    fn incubate_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn incubate_parser_matches_plugin_flags_and_builds_bud_args() {
        let options = incubate_parse_args(&incubate_strings(&[
            "https://github.com/org/source.git",
            "--stem", "custom",
            "--from", "nova",
            "--from-repo", "org/template",
            "--org", "org",
            "--issue", "133",
            "--note", "hello",
            "--nickname", "Pretty",
            "--fast",
            "--root",
            "--blank",
            "--seed",
            "--split",
            "--dry-run",
            "--signal-on-birth",
            "--force",
            "--track-vault",
            "--sync-peers",
            "--flash",
        ])).expect("parse incubate");
        assert_eq!(incubate_resolve_mode(options.flash, options.contribute), Ok(IncubateMode::Flash));
        assert_eq!(incubate_build_skill_command(&options).expect("skill command"), "/incubate https://github.com/org/source.git --flash");
        assert_eq!(
            incubate_bud_args("custom", &options).expect("bud args"),
            incubate_strings(&[
                "bud", "custom", "--repo", "https://github.com/org/source.git", "--from", "nova",
                "--from-repo", "org/template", "--org", "org", "--issue", "133", "--note",
                "hello", "--nickname", "Pretty", "--fast", "--root", "--blank", "--seed",
                "--split", "--dry-run", "--signal-on-birth", "--force", "--track-vault", "--sync-peers",
            ])
        );
    }

    #[test]
    fn incubate_bud_options_are_structural_not_parser_rescanned() {
        let options = incubate_parse_args(&incubate_strings(&[
            "org/source",
            "--stem",
            "custom",
            "--note=--split --trust --from=mallory",
            "--nickname=-leading-opaque",
            "--dry-run",
        ]))
        .expect("parse incubate");
        let bud_options = incubate_bud_options("custom", &options).expect("bud options");
        assert_eq!(bud_options.name.as_deref(), Some("custom"));
        assert_eq!(bud_options.repo.as_deref(), Some("org/source"));
        assert_eq!(bud_options.note.as_deref(), Some("--split --trust --from=mallory"));
        assert_eq!(bud_options.nickname.as_deref(), Some("-leading-opaque"));
        assert!(!bud_options.split, "flag-like note text must not enable --split");
        assert!(bud_options.dry_run);
    }

    #[test]
    fn incubate_derive_stem_and_trigger_override_match_maw_js() {
        assert_eq!(incubate_derive_stem_from_source("Soul-Brews-Studio/foo"), "foo");
        assert_eq!(incubate_derive_stem_from_source("https://github.com/org/foo.git"), "foo");
        assert_eq!(incubate_derive_stem_from_source("foo"), "foo");
        let options = incubate_parse_args(&incubate_strings(&["org/foo", "--trigger", "/foo-custom"])).expect("parse");
        assert_eq!(incubate_build_skill_command(&options).expect("trigger"), "/foo-custom");
    }

    #[test]
    fn incubate_option_injection_guard_blocks_exec_path_and_target_args() {
        assert!(incubate_parse_args(&incubate_strings(&["--bad"])).expect_err("source guard").contains("unknown argument"));
        assert!(incubate_parse_args(&incubate_strings(&["org/foo", "--stem", "-bad"])).expect_err("stem guard").contains("requires a value"));
        assert!(incubate_parse_args(&incubate_strings(&["org/foo", "--from", "-bad"])).expect_err("from guard").contains("requires a value"));
        assert!(incubate_parse_args(&incubate_strings(&["org/foo", "--issue", "-1"])).expect_err("issue guard").contains("requires a value"));
        assert_eq!(incubate_resolve_mode(true, true), Err("--flash and --contribute are mutually exclusive".to_owned()));
    }
}
