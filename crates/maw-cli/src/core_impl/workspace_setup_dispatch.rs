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

