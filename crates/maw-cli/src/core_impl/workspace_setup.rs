const DISPATCH_93: &[DispatcherEntry] = &[
    DispatcherEntry { command: "work", handler: Handler::Sync(work_run_command) },
    DispatcherEntry { command: "awake", handler: Handler::Sync(awake_run_command) },
    DispatcherEntry { command: "scaffold", handler: Handler::Sync(scaffold_run_command) },
    DispatcherEntry { command: "new", handler: Handler::Sync(new_run_command) },
    DispatcherEntry { command: "promote", handler: Handler::Sync(promote_run_command) },
    DispatcherEntry { command: "preflight", handler: Handler::Sync(preflight_run_command) },
    DispatcherEntry { command: "snapshots", handler: Handler::Sync(snapshots_run_command) },
];

const WORK_USAGE: &str = "usage: maw work <repo> [task] [--layout nested|legacy]";
const AWAKE_USAGE: &str = "usage: maw awake <name> [wake flags...]";
const SCAFFOLD_USAGE: &str = "usage: maw scaffold <name> [--rust|--as] [--dest <path>] [--dry-run]";
const NEW_USAGE: &str = "usage: maw new <name> [--rust|--as] [--dest <path>] [--dry-run]";
const PROMOTE_USAGE: &str = "usage: maw promote <window> [--as <name>] [--attach] [--force]";
const PROMOTE_PLACEHOLDER: &str = "__promote_placeholder__";
const PREFLIGHT_USAGE: &str = "usage: maw preflight [path] [--json]";
const SNAPSHOTS_USAGE: &str = "usage: maw snapshots [list|create|show] [name] [--json]";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScaffoldLanguageNative {
    Rust,
    AssemblyScript,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ScaffoldOptionsNative {
    name: String,
    dest: std::path::PathBuf,
    language: ScaffoldLanguageNative,
    dry_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromoteOptionsNative {
    target: String,
    as_session: Option<String>,
    attach: bool,
    force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromoteResolvedNative {
    src_session: String,
    src_window: String,
    dst_session: String,
    attach: bool,
    force: bool,
}

impl PromoteResolvedNative {
    fn src_target(&self) -> String { format!("{}:{}", self.src_session, self.src_window) }
    fn dst_target(&self) -> String { format!("{}:", self.dst_session) }
    fn placeholder_target(&self) -> String { promote_placeholder_target(&self.dst_session) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromoteMutationStateNative {
    created_dst_by_this_run: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PromoteResolveResultNative {
    Resolved { session: String, window: String },
    None,
    Ambiguous(Vec<PromoteCandidateNative>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PromoteCandidateNative {
    session: String,
    window: String,
}

#[allow(dead_code)]
trait PromoteTmuxNative {
    fn promote_list_all(&mut self) -> Vec<TmuxSession>;
    fn promote_list_windows(&mut self, session: &str) -> Result<Vec<maw_tmux::TmuxWindow>, String>;
    fn promote_has_session(&mut self, name: &str) -> bool;
    fn promote_caller_in_tmux(&self) -> bool;
    fn promote_new_session(&mut self, name: &str, window: &str) -> Result<(), String>;
    fn promote_move_window(&mut self, src: &str, dst: &str) -> Result<(), String>;
    fn promote_kill_session(&mut self, name: &str) -> Result<(), String>;
    fn promote_kill_window(&mut self, target: &str) -> Result<(), String>;
    fn promote_switch_client(&mut self, session: &str) -> Result<(), String>;
}

struct PromoteSystemTmuxNative;

impl PromoteTmuxNative for PromoteSystemTmuxNative {
    fn promote_list_all(&mut self) -> Vec<TmuxSession> { TmuxClient::local().list_all() }

    fn promote_list_windows(&mut self, session: &str) -> Result<Vec<maw_tmux::TmuxWindow>, String> {
        promote_validate_tmux_name(session, "source session")?;
        TmuxClient::local().list_windows(session).map_err(|error| error.to_string())
    }

    fn promote_has_session(&mut self, name: &str) -> bool {
        if promote_validate_tmux_name(name, "destination session").is_err() { return false; }
        TmuxClient::local().has_session(name)
    }

    fn promote_caller_in_tmux(&self) -> bool { std::env::var_os("TMUX").is_some() }

    fn promote_new_session(&mut self, name: &str, window: &str) -> Result<(), String> {
        promote_validate_tmux_name(name, "destination session")?;
        promote_validate_tmux_name(window, "placeholder window")?;
        let mut runner = maw_tmux::CommandTmuxRunner::new();
        maw_tmux::TmuxRunner::run(
            &mut runner,
            "new-session",
            &["-d".to_owned(), "-s".to_owned(), name.to_owned(), "-n".to_owned(), window.to_owned()],
        )
        .map(|_| ())
        .map_err(|error| error.message)
    }

    fn promote_move_window(&mut self, src: &str, dst: &str) -> Result<(), String> {
        promote_validate_tmux_target(src, "source target")?;
        promote_validate_tmux_target(dst, "destination target")?;
        let mut runner = maw_tmux::CommandTmuxRunner::new();
        maw_tmux::TmuxRunner::run(
            &mut runner,
            "move-window",
            &["-s".to_owned(), src.to_owned(), "-t".to_owned(), dst.to_owned()],
        )
        .map(|_| ())
        .map_err(|error| error.message)
    }

    fn promote_kill_session(&mut self, name: &str) -> Result<(), String> {
        promote_validate_tmux_name(name, "rollback destination session")?;
        let mut runner = maw_tmux::CommandTmuxRunner::new();
        maw_tmux::TmuxRunner::run(&mut runner, "kill-session", &["-t".to_owned(), name.to_owned()]).map(|_| ()).map_err(|error| error.message)
    }

    fn promote_kill_window(&mut self, target: &str) -> Result<(), String> {
        promote_validate_tmux_target(target, "rollback placeholder target")?;
        let mut runner = maw_tmux::CommandTmuxRunner::new();
        maw_tmux::TmuxRunner::run(&mut runner, "kill-window", &["-t".to_owned(), target.to_owned()]).map(|_| ()).map_err(|error| error.message)
    }

    fn promote_switch_client(&mut self, _session: &str) -> Result<(), String> { Err("promote: attach deferred to #299 attach follow-up".to_owned()) }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PreflightOptionsNative {
    path: std::path::PathBuf,
    json: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SnapshotsActionNative {
    List,
    Create { name: String },
    Show { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SnapshotsOptionsNative {
    action: SnapshotsActionNative,
    json: bool,
}

fn work_run_command(argv: &[String]) -> CliOutput {
    if argv.iter().any(|arg| arg == "--") {
        return work_error("work: -- separator is not allowed");
    }
    if argv.is_empty() {
        return work_error(WORK_USAGE);
    }
    run_workon_command(argv)
}

fn work_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn awake_run_command(argv: &[String]) -> CliOutput {
    if argv.iter().any(|arg| arg == "--") {
        return awake_error("awake: -- separator is not allowed");
    }
    if argv.is_empty() {
        return awake_error(AWAKE_USAGE);
    }
    awake_dispatch_to_existing(argv)
}

fn awake_dispatch_to_existing(argv: &[String]) -> CliOutput {
    let mut forwarded = Vec::with_capacity(argv.len() + 1);
    forwarded.push("awaken".to_owned());
    forwarded.extend(argv.iter().cloned());
    run_cli(&forwarded)
}

fn awake_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn scaffold_run_command(argv: &[String]) -> CliOutput {
    match scaffold_parse_args(argv, SCAFFOLD_USAGE) {
        Ok(options) => match scaffold_apply(&options) {
            Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
            Err(message) => scaffold_error(&message),
        },
        Err(message) => scaffold_error(&message),
    }
}

fn scaffold_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn scaffold_parse_args(argv: &[String], usage: &str) -> Result<ScaffoldOptionsNative, String> {
    let mut language = ScaffoldLanguageNative::Rust;
    let mut dest = None::<std::path::PathBuf>;
    let mut dry_run = false;
    let mut name = None::<String>;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(usage.to_owned()),
            "--" => return Err("scaffold: -- separator is not allowed".to_owned()),
            "--rust" => language = ScaffoldLanguageNative::Rust,
            "--as" | "--assemblyscript" => language = ScaffoldLanguageNative::AssemblyScript,
            "--dry-run" => dry_run = true,
            "--dest" => { dest = Some(scaffold_path_value(argv, &mut index, "--dest")?); }
            value if value.starts_with("--dest=") => dest = Some(scaffold_validate_path(&value["--dest=".len()..])?),
            value if value.starts_with('-') => return Err(scaffold_flag_like(value)),
            value => scaffold_set_name(&mut name, value)?,
        }
        index += 1;
    }
    let name = name.ok_or_else(|| usage.to_owned())?;
    scaffold_validate_name(&name)?;
    let dest = dest.unwrap_or_else(|| std::path::PathBuf::from(&name));
    Ok(ScaffoldOptionsNative { name, dest, language, dry_run })
}

fn scaffold_set_name(slot: &mut Option<String>, value: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(SCAFFOLD_USAGE.to_owned());
    }
    if value.starts_with('-') {
        return Err(scaffold_flag_like(value));
    }
    *slot = Some(value.to_owned());
    Ok(())
}

fn scaffold_path_value(argv: &[String], index: &mut usize, flag: &str) -> Result<std::path::PathBuf, String> {
    let Some(value) = argv.get(*index + 1) else { return Err(format!("scaffold: {flag} requires a value")); };
    *index += 1;
    scaffold_validate_path(value)
}

fn scaffold_validate_name(name: &str) -> Result<(), String> {
    if name == "--" || name.starts_with('-') {
        return Err("scaffold name must not start with '-'".to_owned());
    }
    if let Some(error) = validate_plugin_name(name) {
        return Err(format!("scaffold: invalid plugin name: {error}"));
    }
    Ok(())
}

fn scaffold_validate_path(value: &str) -> Result<std::path::PathBuf, String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains('\0') {
        return Err("scaffold path must be non-empty, unpadded, and not start with '-'".to_owned());
    }
    if value.split('/').any(|part| part == "..") {
        return Err("scaffold path must not contain .. segments".to_owned());
    }
    Ok(std::path::PathBuf::from(value))
}

fn scaffold_flag_like(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a scaffold name.\n  {SCAFFOLD_USAGE}")
}

fn scaffold_apply(options: &ScaffoldOptionsNative) -> Result<String, String> {
    scaffold_validate_destination(&options.dest)?;
    if options.dry_run {
        return Ok(scaffold_render_plan(options));
    }
    match options.language {
        ScaffoldLanguageNative::Rust => scaffold_write_rust(options)?,
        ScaffoldLanguageNative::AssemblyScript => scaffold_write_as(options)?,
    }
    Ok(scaffold_render_created(options))
}

fn scaffold_validate_destination(path: &std::path::Path) -> Result<(), String> {
    let display = path.display().to_string();
    scaffold_validate_path(&display)?;
    if path.exists() {
        return Err(format!("scaffold: destination exists: {}", path.display()));
    }
    Ok(())
}

fn scaffold_render_plan(options: &ScaffoldOptionsNative) -> String {
    format!("scaffold plan: create {} plugin {} at {}\n", scaffold_language_name(options.language), options.name, options.dest.display())
}

fn scaffold_render_created(options: &ScaffoldOptionsNative) -> String {
    format!("created {} plugin {} at {}\n", scaffold_language_name(options.language), options.name, options.dest.display())
}

fn scaffold_language_name(language: ScaffoldLanguageNative) -> &'static str {
    match language {
        ScaffoldLanguageNative::Rust => "rust",
        ScaffoldLanguageNative::AssemblyScript => "assemblyscript",
    }
}

fn scaffold_write_rust(options: &ScaffoldOptionsNative) -> Result<(), String> {
    std::fs::create_dir_all(options.dest.join("src")).map_err(|error| format!("scaffold: create rust dirs: {error}"))?;
    std::fs::write(options.dest.join("Cargo.toml"), scaffold_rust_cargo(&options.name)).map_err(|error| format!("scaffold: write Cargo.toml: {error}"))?;
    std::fs::write(options.dest.join("src/lib.rs"), scaffold_rust_lib()).map_err(|error| format!("scaffold: write src/lib.rs: {error}"))?;
    std::fs::write(options.dest.join("README.md"), scaffold_readme(&options.name, "Rust")).map_err(|error| format!("scaffold: write README.md: {error}"))?;
    std::fs::write(options.dest.join("plugin.json"), build_manifest_json(&options.name, ScaffoldLanguage::Rust)).map_err(|error| format!("scaffold: write plugin.json: {error}"))?;
    Ok(())
}

fn scaffold_write_as(options: &ScaffoldOptionsNative) -> Result<(), String> {
    std::fs::create_dir_all(options.dest.join("assembly")).map_err(|error| format!("scaffold: create as dirs: {error}"))?;
    std::fs::write(options.dest.join("package.json"), scaffold_as_package(&options.name)).map_err(|error| format!("scaffold: write package.json: {error}"))?;
    std::fs::write(options.dest.join("assembly/index.ts"), scaffold_as_index()).map_err(|error| format!("scaffold: write assembly/index.ts: {error}"))?;
    std::fs::write(options.dest.join("README.md"), scaffold_readme(&options.name, "AssemblyScript")).map_err(|error| format!("scaffold: write README.md: {error}"))?;
    std::fs::write(options.dest.join("plugin.json"), build_manifest_json(&options.name, ScaffoldLanguage::AssemblyScript)).map_err(|error| format!("scaffold: write plugin.json: {error}"))?;
    Ok(())
}

fn scaffold_rust_cargo(name: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n")
}

fn scaffold_rust_lib() -> &'static str {
    "#[no_mangle]\npub extern \"C\" fn maw_plugin_entry() -> i32 { 0 }\n"
}

fn scaffold_as_package(name: &str) -> String {
    format!("{{\n  \"name\": \"{name}\",\n  \"version\": \"0.1.0\",\n  \"scripts\": {{\"build\": \"asc assembly/index.ts --target release\"}}\n}}\n")
}

fn scaffold_as_index() -> &'static str {
    "export function mawPluginEntry(): i32 { return 0; }\n"
}

fn scaffold_readme(name: &str, language: &str) -> String {
    format!("# {name}\n\n{language} maw plugin scaffold.\n")
}

fn new_run_command(argv: &[String]) -> CliOutput {
    match new_parse_args(argv) {
        Ok(options) => match scaffold_apply(&options) {
            Ok(stdout) => CliOutput { code: 0, stdout: new_relabel_stdout(&stdout), stderr: String::new() },
            Err(message) => new_error(&message),
        },
        Err(message) => new_error(&message),
    }
}

fn new_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") }
}

fn new_parse_args(argv: &[String]) -> Result<ScaffoldOptionsNative, String> {
    scaffold_parse_args(argv, NEW_USAGE).map_err(|message| message.replace("scaffold", "new"))
}

fn new_relabel_stdout(stdout: &str) -> String {
    stdout.replace("scaffold plan:", "new plan:").replace("created", "created new")
}

fn promote_run_command(argv: &[String]) -> CliOutput {
    let mut tmux = PromoteSystemTmuxNative;
    promote_run_command_with(argv, &mut tmux)
}

fn promote_run_command_with(argv: &[String], tmux: &mut impl PromoteTmuxNative) -> CliOutput {
    match promote_parse_args(argv).and_then(|options| promote_execute(&options, tmux)) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => promote_error(&message),
    }
}

fn promote_error(message: &str) -> CliOutput {
    CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}
") }
}

fn promote_parse_args(argv: &[String]) -> Result<PromoteOptionsNative, String> {
    let mut target = None::<String>;
    let mut as_session = None::<String>;
    let mut attach = false;
    let mut force = false;
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(PROMOTE_USAGE.to_owned()),
            "--" => return Err("promote: -- separator is not allowed".to_owned()),
            "--attach" => attach = true,
            "--force" => force = true,
            "--as" => as_session = Some(promote_take_session_value(argv, &mut index, "--as")?),
            value if value.starts_with("--as=") => as_session = Some(promote_validate_session_name(&value["--as=".len()..], "--as")?),
            value if value.starts_with('-') => return Err(promote_flag_like(value)),
            value => promote_set_target(&mut target, value)?,
        }
        index += 1;
    }
    Ok(PromoteOptionsNative { target: target.ok_or_else(|| PROMOTE_USAGE.to_owned())?, as_session, attach, force })
}

fn promote_take_session_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let Some(value) = argv.get(*index + 1) else { return Err(format!("promote: {flag} requires a value")); };
    *index += 1;
    promote_validate_session_name(value, flag)
}

fn promote_set_target(slot: &mut Option<String>, value: &str) -> Result<(), String> {
    if slot.is_some() {
        return Err(PROMOTE_USAGE.to_owned());
    }
    *slot = Some(promote_validate_target(value, "target")?);
    Ok(())
}

fn promote_execute(options: &PromoteOptionsNative, tmux: &mut impl PromoteTmuxNative) -> Result<String, String> {
    let planned = promote_resolve_ready(options, tmux)?;
    let ready = promote_revalidate_ready(options, &planned, tmux)?;
    let dst_exists_now = tmux.promote_has_session(&ready.dst_session);
    if dst_exists_now && !ready.force {
        return Err(promote_destination_exists_error(&options.target, &ready.dst_session));
    }

    let mut state = PromoteMutationStateNative { created_dst_by_this_run: false };
    if !dst_exists_now {
        tmux.promote_new_session(&ready.dst_session, PROMOTE_PLACEHOLDER).map_err(|error| format!("promote: tmux new-session failed — {error}"))?;
        state.created_dst_by_this_run = true;
    }

    let src_target = ready.src_target();
    let dst_target = ready.dst_target();
    promote_validate_tmux_target(&src_target, "source target")?;
    promote_validate_tmux_target(&dst_target, "destination target")?;

    if let Err(error) = tmux.promote_move_window(&src_target, &dst_target) {
        promote_rollback_after_failure(tmux, &ready, &state, "move-window failure");
        return Err(format!("promote: tmux move failed — {error}"));
    }

    let dst_windows = match tmux.promote_list_windows(&ready.dst_session) {
        Ok(windows) => windows,
        Err(error) => {
            promote_cleanup_after_unknown_verify_failure(tmux, &ready, &state);
            return Err(format!(
                "promote: tmux move verification failed — cannot list destination session '{}': {error}; no session rollback performed because ownership cannot be verified; inspect and clean '{}' manually if needed",
                ready.dst_session, ready.dst_session
            ));
        }
    };

    if !promote_window_exists(&dst_windows, &ready.src_window) {
        promote_rollback_after_verify_miss(tmux, &ready, &state, &dst_windows);
        let suffix = if state.created_dst_by_this_run && promote_windows_only_placeholder(&dst_windows) { "; rolled back placeholder session" } else { "" };
        return Err(format!(
            "promote: tmux move verification failed — '{}' did not appear in '{}' after move-window{suffix}",
            src_target, ready.dst_session
        ));
    }

    if state.created_dst_by_this_run {
        let _ = tmux.promote_kill_window(&ready.placeholder_target());
    }

    Ok(promote_render_success(&ready))
}

fn promote_resolve_ready(options: &PromoteOptionsNative, tmux: &mut impl PromoteTmuxNative) -> Result<PromoteResolvedNative, String> {
    let sessions = tmux.promote_list_all();
    promote_resolve_ready_from_sessions(options, tmux, &sessions, "promote planning")
}

fn promote_revalidate_ready(options: &PromoteOptionsNative, planned: &PromoteResolvedNative, tmux: &mut impl PromoteTmuxNative) -> Result<PromoteResolvedNative, String> {
    let sessions = tmux.promote_list_all();
    let fresh = promote_resolve_ready_from_sessions(options, tmux, &sessions, "promote mutation")?;
    if fresh.src_session != planned.src_session || fresh.src_window != planned.src_window {
        return Err(format!(
            "promote: source changed before mutation (planned {}:{}, now {}:{})",
            planned.src_session, planned.src_window, fresh.src_session, fresh.src_window
        ));
    }
    if fresh.dst_session != planned.dst_session {
        return Err(format!("promote: destination changed before mutation (planned {}, now {})", planned.dst_session, fresh.dst_session));
    }
    Ok(fresh)
}

fn promote_resolve_ready_from_sessions(
    options: &PromoteOptionsNative,
    tmux: &mut impl PromoteTmuxNative,
    sessions: &[TmuxSession],
    phase: &str,
) -> Result<PromoteResolvedNative, String> {
    let resolved = promote_resolve_target(&options.target, sessions)?;
    let PromoteResolveResultNative::Resolved { session: src_session, window: src_window } = resolved else {
        return Err(promote_resolution_error_message(&options.target, resolved));
    };
    promote_validate_tmux_name(&src_session, "source session")?;
    promote_validate_tmux_name(&src_window, "source window")?;
    let source_windows = tmux.promote_list_windows(&src_session).map_err(|error| format!("promote: cannot list windows in source session '{src_session}': {error}"))?;
    if source_windows.len() <= 1 {
        return Err(promote_only_window_error(&src_session, &src_window));
    }
    if !promote_window_exists(&source_windows, &src_window) {
        return Err(format!("promote: source '{src_session}:{src_window}' disappeared before {phase}"));
    }
    let dst_session = promote_destination_session(options, &src_window)?;
    Ok(PromoteResolvedNative { src_session, src_window, dst_session, attach: options.attach, force: options.force })
}

fn promote_resolve_target(target: &str, sessions: &[TmuxSession]) -> Result<PromoteResolveResultNative, String> {
    promote_validate_target(target, "target")?;
    let explicit_session = promote_target_session(target)?;
    if let Some(explicit_window) = promote_target_window(target)? {
        return promote_resolve_explicit(&explicit_session, &explicit_window, sessions);
    }
    let mut matches = promote_exact_window_matches(target, sessions);
    if matches.is_empty() {
        if let Some(canonical) = promote_strip_tmux_display_suffix(target) { matches = promote_exact_window_matches(canonical, sessions); }
    }
    Ok(match matches.len() {
        0 => PromoteResolveResultNative::None,
        1 => {
            let candidate = matches.remove(0);
            PromoteResolveResultNative::Resolved { session: candidate.session, window: candidate.window }
        }
        _ => PromoteResolveResultNative::Ambiguous(matches),
    })
}

fn promote_resolve_explicit(session: &str, window: &str, sessions: &[TmuxSession]) -> Result<PromoteResolveResultNative, String> {
    promote_validate_tmux_name(session, "source session")?;
    promote_validate_tmux_name(window, "source window")?;
    let Some(src_session) = sessions.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(session)) else {
        return Ok(PromoteResolveResultNative::Resolved { session: session.to_owned(), window: window.to_owned() });
    };
    if let Some(exact) = src_session.windows.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(window)) {
        return Ok(PromoteResolveResultNative::Resolved { session: src_session.name.clone(), window: exact.name.clone() });
    }
    if let Some(canonical) = promote_strip_tmux_display_suffix(window) {
        if let Some(exact) = src_session.windows.iter().find(|candidate| candidate.name.eq_ignore_ascii_case(canonical)) {
            return Ok(PromoteResolveResultNative::Resolved { session: src_session.name.clone(), window: exact.name.clone() });
        }
    }
    Ok(PromoteResolveResultNative::Resolved { session: src_session.name.clone(), window: window.to_owned() })
}

fn promote_exact_window_matches(target: &str, sessions: &[TmuxSession]) -> Vec<PromoteCandidateNative> {
    sessions.iter().flat_map(|session| {
        session.windows.iter().filter(move |window| window.name == target).map(move |window| PromoteCandidateNative { session: session.name.clone(), window: window.name.clone() })
    }).collect()
}

fn promote_resolution_error_message(target: &str, resolved: PromoteResolveResultNative) -> String {
    match resolved {
        PromoteResolveResultNative::None => format!("promote: no window matches '{target}'"),
        PromoteResolveResultNative::Ambiguous(candidates) => {
            let mut message = format!("promote: '{target}' matches {} windows", candidates.len());
            for candidate in candidates {
                let _ = write!(message, "
  [90m• {}:{}[0m", candidate.session, candidate.window);
            }
            let _ = write!(message, "
  [90muse: maw promote <session>:<window>[0m");
            message
        }
        PromoteResolveResultNative::Resolved { .. } => unreachable!("resolved handled by caller"),
    }
}

fn promote_destination_session(options: &PromoteOptionsNative, src_window: &str) -> Result<String, String> {
    let destination = if let Some(value) = &options.as_session { promote_validate_session_name(value, "--as")? } else { wake_session_name(src_window) };
    promote_validate_session_name(&destination, "destination session")
}

fn promote_validate_target(value: &str, label: &str) -> Result<String, String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains('\0') {
        return Err(format!("promote {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("promote {label} must not contain whitespace or control characters"));
    }
    Ok(value.to_owned())
}

fn promote_validate_session_name(value: &str, label: &str) -> Result<String, String> {
    promote_validate_target(value, label)?;
    promote_validate_tmux_name(value, label)?;
    Ok(value.to_owned())
}

fn promote_validate_tmux_name(value: &str, label: &str) -> Result<(), String> {
    wake_validate_tmux_name(value, label).map_err(|_| format!("promote: invalid {label}"))
}

fn promote_target_session(target: &str) -> Result<String, String> {
    let session = target.split(':').next().unwrap_or(target);
    promote_validate_tmux_name(session, "source session")?;
    Ok(session.to_owned())
}

fn promote_target_window(target: &str) -> Result<Option<String>, String> {
    let window = target.split(':').skip(1).collect::<Vec<_>>().join(":");
    let trimmed = window.trim();
    if trimmed.is_empty() { return Ok(None); }
    promote_validate_tmux_name(trimmed, "source window")?;
    Ok(Some(trimmed.to_owned()))
}

fn promote_strip_tmux_display_suffix(window: &str) -> Option<&str> {
    if window.ends_with('-') && window.len() > 1 { Some(&window[..window.len() - 1]) } else { None }
}

fn promote_window_exists(windows: &[maw_tmux::TmuxWindow], name: &str) -> bool {
    windows.iter().any(|window| window.name == name)
}

fn promote_only_window_error(src_session: &str, src_window: &str) -> String {
    format!("promote refused — '{src_window}' is the only window in session '{src_session}'.
  [90mthat would just be a session rename, not an eject.[0m
  [90muse: tmux rename-session -t {src_session} <new-name>[0m")
}

fn promote_destination_exists_error(target: &str, dst_session: &str) -> String {
    format!("promote refused — session '{dst_session}' already exists.
  [90muse: maw promote {target} --as <new-name>[0m
  [90mor:  maw promote {target} --force[0m  (merges into existing)")
}

fn promote_placeholder_target(dst_session: &str) -> String {
    format!("{dst_session}:{PROMOTE_PLACEHOLDER}")
}

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

fn snapshots_dir() -> std::path::PathBuf {
    maw_state_dir(&snapshots_xdg_env()).join("work-snapshots")
}

fn snapshots_xdg_env() -> MawXdgEnv {
    let home = std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from);
    let keys = ["MAW_HOME", "MAW_STATE_DIR", "MAW_XDG", "XDG_STATE_HOME"];
    MawXdgEnv::with_vars(home, keys.into_iter().filter_map(|key| std::env::var(key).ok().map(|value| (key, value))))
}

fn snapshots_default_name() -> String {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |duration| duration.as_secs());
    format!("snapshot-{seconds}")
}

fn snapshots_file(dir: &std::path::Path, name: &str) -> Result<std::path::PathBuf, String> {
    snapshots_validate_name(name)?;
    Ok(dir.join(format!("{name}.json")))
}

fn snapshots_list(dir: &std::path::Path, json: bool) -> Result<String, String> {
    let mut names = Vec::<String>::new();
    for entry in std::fs::read_dir(dir).map_err(|error| format!("snapshots: list: {error}"))?.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("json") {
            if let Some(stem) = path.file_stem().and_then(std::ffi::OsStr::to_str) {
                names.push(stem.to_owned());
            }
        }
    }
    names.sort();
    if json {
        let body = names.iter().map(|name| json_string(name)).collect::<Vec<_>>().join(",");
        return Ok(format!("{{\"command\":\"snapshots\",\"snapshots\":[{body}]}}\n"));
    }
    Ok(if names.is_empty() { "no snapshots\n".to_owned() } else { format!("{}\n", names.join("\n")) })
}

fn snapshots_create(dir: &std::path::Path, name: &str, json: bool) -> Result<String, String> {
    let file = snapshots_file(dir, name)?;
    if file.exists() {
        return Err(format!("snapshots: snapshot exists: {name}"));
    }
    let cwd = std::env::current_dir().map_err(|error| format!("snapshots: cwd: {error}"))?;
    let body = format!("{{\"name\":{},\"cwd\":{},\"createdBy\":\"maw snapshots\"}}\n", json_string(name), json_string(&cwd.display().to_string()));
    std::fs::write(&file, &body).map_err(|error| format!("snapshots: write: {error}"))?;
    if json { Ok(body) } else { Ok(format!("created snapshot {name}\n")) }
}

fn snapshots_show(dir: &std::path::Path, name: &str, json: bool) -> Result<String, String> {
    let file = snapshots_file(dir, name)?;
    let body = std::fs::read_to_string(&file).map_err(|_| format!("snapshots: snapshot not found: {name}"))?;
    if json { Ok(body) } else { Ok(format!("{name}: {body}")) }
}

#[cfg(test)]
mod work_bundle_tests {
    use super::*;

    struct WorkBundleEnvGuard {
        root: std::path::PathBuf,
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl WorkBundleEnvGuard {
        fn work_new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-work-bundle-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("home")).expect("home");
            std::fs::create_dir_all(root.join("state")).expect("state");
            std::env::set_var("HOME", root.join("home"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("config"));
            std::env::set_var("XDG_STATE_HOME", root.join("state"));
            std::env::set_var("XDG_DATA_HOME", root.join("data"));
            std::env::set_var("XDG_CACHE_HOME", root.join("cache"));
            Self { root, saved }
        }
    }

    impl Drop for WorkBundleEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn work_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[derive(Default)]
    #[allow(clippy::struct_excessive_bools)]
    struct PromoteFakeTmux {
        sessions: Vec<TmuxSession>,
        existing: std::collections::BTreeSet<String>,
        caller_in_tmux: bool,
        calls: Vec<String>,
        mutation_calls: Vec<String>,
        move_should_fail: bool,
        kill_session_should_fail: bool,
        verify_missing: bool,
        list_windows_fail_for: std::collections::BTreeSet<String>,
        foreign_before_rollback: bool,
    }

    impl PromoteFakeTmux {
        fn promote_fixture() -> Self {
            Self {
                sessions: vec![
                    TmuxSession {
                        name: "77-mawjs".to_owned(),
                        windows: vec![promote_test_window("mawjs-oracle"), promote_test_window("test-cli")],
                    },
                    TmuxSession { name: "scratch".to_owned(), windows: vec![promote_test_window("scratch")] },
                ],
                caller_in_tmux: true,
                ..Self::default()
            }
        }

        fn promote_session_mut(&mut self, name: &str) -> Option<&mut TmuxSession> {
            self.sessions.iter_mut().find(|item| item.name == name)
        }
    }

    impl PromoteTmuxNative for PromoteFakeTmux {
        fn promote_list_all(&mut self) -> Vec<TmuxSession> {
            self.calls.push("list-all".to_owned());
            self.sessions.clone()
        }

        fn promote_list_windows(&mut self, session: &str) -> Result<Vec<maw_tmux::TmuxWindow>, String> {
            self.calls.push(format!("list-windows {session}"));
            if self.list_windows_fail_for.contains(session) {
                return Err("tmux list failed".to_owned());
            }
            self.sessions.iter().find(|item| item.name == session).map(|item| item.windows.clone()).ok_or_else(|| "no such session".to_owned())
        }

        fn promote_has_session(&mut self, name: &str) -> bool {
            self.calls.push(format!("has-session {name}"));
            self.existing.contains(name) || self.sessions.iter().any(|item| item.name == name)
        }

        fn promote_caller_in_tmux(&self) -> bool { self.caller_in_tmux }

        fn promote_new_session(&mut self, name: &str, window: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("new-session -d -s {name} -n {window}"));
            self.existing.insert(name.to_owned());
            self.sessions.push(TmuxSession { name: name.to_owned(), windows: vec![promote_test_window(window)] });
            Ok(())
        }

        fn promote_move_window(&mut self, src: &str, dst: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("move-window -s {src} -t {dst}"));
            let (src_session, src_window) = src.split_once(':').ok_or_else(|| "bad source".to_owned())?;
            let dst_session = dst.trim_end_matches(':');
            if self.move_should_fail {
                if self.foreign_before_rollback {
                    if let Some(dst) = self.promote_session_mut(dst_session) {
                        dst.windows.push(promote_test_window("foreign"));
                    }
                }
                return Err("move failed".to_owned());
            }
            if let Some(src_session_item) = self.promote_session_mut(src_session) {
                src_session_item.windows.retain(|window| window.name != src_window);
            }
            if !self.verify_missing {
                if let Some(dst_session_item) = self.promote_session_mut(dst_session) {
                    dst_session_item.windows.push(promote_test_window(src_window));
                }
            }
            Ok(())
        }

        fn promote_kill_session(&mut self, name: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("kill-session -t {name}"));
            if self.kill_session_should_fail {
                return Err("kill session failed".to_owned());
            }
            self.existing.remove(name);
            self.sessions.retain(|session| session.name != name);
            Ok(())
        }

        fn promote_kill_window(&mut self, target: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("kill-window -t {target}"));
            let Some((session, window)) = target.split_once(':') else { return Err("bad target".to_owned()); };
            if let Some(session_item) = self.promote_session_mut(session) {
                session_item.windows.retain(|item| item.name != window);
            }
            Ok(())
        }

        fn promote_switch_client(&mut self, session: &str) -> Result<(), String> {
            self.mutation_calls.push(format!("switch-client -t {session}"));
            Ok(())
        }
    }

    fn promote_test_window(name: &str) -> maw_tmux::TmuxWindow {
        maw_tmux::TmuxWindow { index: 0, name: name.to_owned(), active: false, cwd: None }
    }

    #[test]
    fn work_dispatch_registers_seven_commands() {
        assert_eq!(DISPATCH_93.len(), 7);
        let commands = DISPATCH_93.iter().map(|entry| entry.command).collect::<Vec<_>>();
        assert_eq!(commands, ["work", "awake", "scaffold", "new", "promote", "preflight", "snapshots"]);
    }

    #[test]
    fn scaffold_and_new_create_hermetic_plugins() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let env = WorkBundleEnvGuard::work_new();
        let rust_dest = env.root.join("hello-rust");
        let args = work_args(&["hello-rust", "--dest", rust_dest.to_str().expect("utf8")]);
        let out = scaffold_run_command(&args);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(rust_dest.join("plugin.json").exists());
        let as_dest = env.root.join("hello-as");
        let args = work_args(&["hello-as", "--as", "--dest", as_dest.to_str().expect("utf8")]);
        let out = new_run_command(&args);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(as_dest.join("assembly/index.ts").exists());
    }

    #[test]
    fn work_guards_reject_separator_and_leading_dash_values() {
        assert!(work_run_command(&work_args(&["--"])).stderr.contains("separator"));
        assert!(awake_run_command(&work_args(&["--"])).stderr.contains("separator"));
        assert!(scaffold_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(new_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(promote_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(preflight_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
        assert!(snapshots_run_command(&work_args(&["-bad"])).stderr.contains("looks like a flag"));
    }

    #[test]
    fn promote_mutates_missing_destination_with_golden_and_exact_argv() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated", "--attach"]), &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, include_str!("../../tests/fixtures/native-promote/promote-success.stdout"));
        assert_eq!(
            tmux.calls,
            ["list-all", "list-windows 77-mawjs", "list-all", "list-windows 77-mawjs", "has-session isolated", "list-windows isolated"]
        );
        assert_eq!(
            tmux.mutation_calls,
            [
                "new-session -d -s isolated -n __promote_placeholder__",
                "move-window -s 77-mawjs:test-cli -t isolated:",
                "kill-window -t isolated:__promote_placeholder__",
            ]
        );
    }

    #[test]
    fn promote_refuses_ambiguous_and_destination_exists_without_mutation() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.sessions.push(TmuxSession { name: "other".to_owned(), windows: vec![promote_test_window("test-cli")] });
        let ambiguous = promote_run_command_with(&work_args(&["test-cli"]), &mut tmux);
        assert_eq!(ambiguous.code, 1);
        assert_eq!(ambiguous.stderr, include_str!("../../tests/fixtures/native-promote/promote-ambiguous.stderr"));
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.existing.insert("isolated".to_owned());
        let exists = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(exists.code, 1);
        assert_eq!(exists.stderr, include_str!("../../tests/fixtures/native-promote/promote-dst-exists.stderr"));
        assert!(tmux.mutation_calls.is_empty());
    }

    #[test]
    fn promote_refuses_only_window_and_bad_inputs_before_mutation() {
        let mut tmux = PromoteFakeTmux::promote_fixture();
        let solo = promote_run_command_with(&work_args(&["scratch:scratch", "--as", "isolated"]), &mut tmux);
        assert_eq!(solo.code, 1);
        assert!(solo.stderr.contains("only window in session 'scratch'"));
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        let bad_as = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "-bad"]), &mut tmux);
        assert_eq!(bad_as.code, 1);
        assert!(bad_as.stderr.contains("not start with '-'") || bad_as.stderr.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
        assert!(tmux.mutation_calls.is_empty());

        let mut tmux = PromoteFakeTmux::promote_fixture();
        let old_shape = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--base", "alpha"]), &mut tmux);
        assert_eq!(old_shape.code, 1);
        assert!(old_shape.stderr.contains("looks like a flag"));
        assert!(tmux.calls.is_empty());
        assert!(tmux.mutation_calls.is_empty());
    }

    #[test]
    fn promote_force_merge_existing_destination_never_kills_existing_dst() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture("MAW_JS_REF_DIR");
        std::env::set_var("MAW_JS_REF_DIR", "/nonexistent");
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.sessions.push(TmuxSession { name: "isolated".to_owned(), windows: vec![promote_test_window("existing")] });
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated", "--force"]), &mut tmux);
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, include_str!("../../tests/fixtures/native-promote/promote-existing-force.stdout"));
        assert_eq!(tmux.mutation_calls, ["move-window -s 77-mawjs:test-cli -t isolated:"]);
        assert!(tmux.mutation_calls.iter().all(|call| !call.starts_with("kill-")));
    }

    #[test]
    fn promote_rolls_back_created_placeholder_but_not_foreign_windows() {
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.verify_missing = true;
        let verify = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(verify.code, 1);
        assert!(verify.stderr.contains("rolled back placeholder session"));
        assert!(tmux.mutation_calls.contains(&"kill-session -t isolated".to_owned()));

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.verify_missing = true;
        tmux.move_should_fail = false;
        let out = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("rolled back placeholder session"));
    }

    #[test]
    fn promote_q1_foreign_window_safe_and_q2_list_fail_conservative_no_kill_session() {
        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.move_should_fail = true;
        tmux.foreign_before_rollback = true;
        let move_fail = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(move_fail.code, 1);
        assert!(move_fail.stderr.contains("move failed"));
        assert!(!tmux.mutation_calls.contains(&"kill-session -t isolated".to_owned()));
        assert!(tmux.mutation_calls.contains(&"kill-window -t isolated:__promote_placeholder__".to_owned()));

        let mut tmux = PromoteFakeTmux::promote_fixture();
        tmux.list_windows_fail_for.insert("isolated".to_owned());
        let list_fail = promote_run_command_with(&work_args(&["77-mawjs:test-cli", "--as", "isolated"]), &mut tmux);
        assert_eq!(list_fail.code, 1);
        assert!(list_fail.stderr.contains("no session rollback performed because ownership cannot be verified"));
        assert!(!tmux.mutation_calls.contains(&"kill-session -t isolated".to_owned()));
        assert!(tmux.mutation_calls.contains(&"kill-window -t isolated:__promote_placeholder__".to_owned()));
    }

    #[test]
    fn snapshots_create_list_show_are_hermetic() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = WorkBundleEnvGuard::work_new();
        let create = snapshots_run_command(&work_args(&["create", "alpha_snap"]));
        assert_eq!(create.code, 0, "{}", create.stderr);
        let list = snapshots_run_command(&work_args(&["list"]));
        assert!(list.stdout.contains("alpha_snap"));
        let show = snapshots_run_command(&work_args(&["show", "alpha_snap", "--json"]));
        assert!(show.stdout.contains("\"name\":\"alpha_snap\""));
    }

    #[test]
    fn preflight_json_reports_temp_git_repo_clean() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let env = WorkBundleEnvGuard::work_new();
        std::process::Command::new("git").arg("init").arg(&env.root).output().expect("git init");
        let out = preflight_run_command(&work_args(&[env.root.to_str().expect("utf8"), "--json"]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert!(out.stdout.contains("\"git\":true"));
    }
}
