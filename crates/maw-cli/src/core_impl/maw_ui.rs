const DISPATCH_100: &[DispatcherEntry] = &[DispatcherEntry {
    command: "ui",
    handler: Handler::Sync(ui_run_command),
}];

const UI_USAGE: &str = "usage: maw ui [--install] [--source] [--tunnel] [--dev] [--3d] [--version]";
const UI_TUNNEL_NOTE: &str = "ui --tunnel uses protected serve orchestration route /api/orchestration/workon; execution remains behind D2 auth";
const UI_SOURCE_REPO_URL: &str = "https://github.com/Soul-Brews-Studio/maw-ui.git";
const UI_VERSION_MARKER: &str = ".maw-ui-version";
const UI_SOURCE_TIMEOUT_MS: u64 = 120_000;

const UI_FLAG_INSTALL: u8 = 1 << 0;
const UI_FLAG_SOURCE: u8 = 1 << 1;
const UI_FLAG_TUNNEL: u8 = 1 << 2;
const UI_FLAG_DEV: u8 = 1 << 3;
const UI_FLAG_THREE_D: u8 = 1 << 4;
const UI_FLAG_VERSION: u8 = 1 << 5;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct UiOptions {
    flags: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UiPlan {
    mode: &'static str,
    dist_dir: String,
    source_dir: String,
    serve_url: String,
    commands: Vec<Vec<String>>,
    notes: Vec<String>,
}

fn ui_run_command(argv: &[String]) -> CliOutput {
    ui_run_command_with(argv, &ui_default_context())
}

fn ui_run_command_with(argv: &[String], context: &UiContext) -> CliOutput {
    let mut runner = UiSystemRunner;
    match ui_run_with_runner(argv, context, &mut runner) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UiContext {
    cwd: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    serve_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct UiExecOutput {
    code: i32,
    stdout: String,
    stderr: String,
}

trait UiExecRunner {
    fn ui_exec(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
        timeout_ms: u64,
    ) -> Result<UiExecOutput, String>;
}

struct UiSystemRunner;

impl UiExecRunner for UiSystemRunner {
    fn ui_exec(
        &mut self,
        program: &str,
        args: &[String],
        cwd: Option<&std::path::Path>,
        timeout_ms: u64,
    ) -> Result<UiExecOutput, String> {
        ui_validate_program(program)?;
        ui_validate_exec_args(args)?;
        if let Some(cwd) = cwd {
            ui_validate_path(cwd, "exec cwd")?;
        }
        let mut command = std::process::Command::new(program);
        command
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if let Some(cwd) = cwd {
            command.current_dir(cwd);
        }
        let mut child = command
            .spawn()
            .map_err(|err| format!("ui: failed to start {program}: {err}"))?;
        let started = std::time::Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    let output = child
                        .wait_with_output()
                        .map_err(|err| format!("ui: failed to collect {program} output: {err}"))?;
                    return Ok(UiExecOutput {
                        code: output.status.code().unwrap_or(1),
                        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
                    });
                }
                Ok(None) if started.elapsed() >= std::time::Duration::from_millis(timeout_ms) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("ui: {program} timed out"));
                }
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(25)),
                Err(err) => return Err(format!("ui: failed to wait for {program}: {err}")),
            }
        }
    }
}

fn ui_default_context() -> UiContext {
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    UiContext {
        data_dir: cwd.join(".maw").join("ui"),
        cwd,
        serve_url: "http://127.0.0.1:1977".to_owned(),
    }
}

#[cfg(test)]
fn ui_run(argv: &[String], context: &UiContext) -> Result<String, String> {
    let mut runner = UiSystemRunner;
    ui_run_with_runner(argv, context, &mut runner)
}

fn ui_run_with_runner(argv: &[String], context: &UiContext, runner: &mut dyn UiExecRunner) -> Result<String, String> {
    let options = ui_parse_args(argv)?;
    if ui_has_flag(&options, UI_FLAG_VERSION) {
        return Ok(format!("maw ui {}\n", env!("CARGO_PKG_VERSION")));
    }
    if ui_has_flag(&options, UI_FLAG_SOURCE) {
        return ui_install_from_source(context, runner);
    }
    let plan = ui_build_plan(&options, context)?;
    Ok(ui_render_plan(&plan))
}

fn ui_parse_args(argv: &[String]) -> Result<UiOptions, String> {
    let mut options = UiOptions::default();
    for arg in argv {
        match arg.as_str() {
            "--help" | "-h" | "help" => return Err(UI_USAGE.to_owned()),
            "--" => return Err("ui: -- separator is not allowed".to_owned()),
            "--install" => ui_set_flag(&mut options, UI_FLAG_INSTALL),
            "--source" => ui_set_flag(&mut options, UI_FLAG_SOURCE),
            "--tunnel" => ui_set_flag(&mut options, UI_FLAG_TUNNEL),
            "--dev" => ui_set_flag(&mut options, UI_FLAG_DEV),
            "--3d" => ui_set_flag(&mut options, UI_FLAG_THREE_D),
            "--version" | "-V" => ui_set_flag(&mut options, UI_FLAG_VERSION),
            value if value.starts_with('-') => return Err(ui_flag_like_value(value)),
            value => return Err(format!("ui: unexpected argument {value:?}\n  {UI_USAGE}")),
        }
    }
    Ok(options)
}

fn ui_set_flag(options: &mut UiOptions, flag: u8) {
    options.flags |= flag;
}

fn ui_has_flag(options: &UiOptions, flag: u8) -> bool {
    options.flags & flag != 0
}

fn ui_flag_like_value(value: &str) -> String {
    format!("ui: {value:?} is not a supported flag\n  {UI_USAGE}")
}

fn ui_build_plan(options: &UiOptions, context: &UiContext) -> Result<UiPlan, String> {
    ui_validate_context(context)?;
    let dist_dir = context.data_dir.join("dist");
    let source_dir = context.cwd.join("ui");
    let mut commands = Vec::new();
    let mut notes = Vec::new();
    let mode = ui_mode(options);
    if ui_has_flag(options, UI_FLAG_INSTALL) {
        commands.push(ui_command(&["install", ui_path_text(&dist_dir)?.as_str()]));
        notes.push("install validates the dist path; native port does not execute installers in CI".to_owned());
    }
    if ui_has_flag(options, UI_FLAG_SOURCE) {
        commands.push(ui_command(&["build-source", ui_path_text(&source_dir)?.as_str()]));
        notes.push("source build runs through the validated native git/bun runner".to_owned());
    }
    if ui_has_flag(options, UI_FLAG_DEV) {
        commands.push(ui_command(&["dev", ui_path_text(&source_dir)?.as_str()]));
    }
    if ui_has_flag(options, UI_FLAG_TUNNEL) {
        commands.push(ui_command(&["tunnel", context.serve_url.as_str()]));
        notes.push(UI_TUNNEL_NOTE.to_owned());
    }
    if ui_has_flag(options, UI_FLAG_THREE_D) {
        notes.push("3d UI mode requested".to_owned());
    }
    if commands.is_empty() {
        commands.push(ui_command(&["launch", ui_path_text(&dist_dir)?.as_str()]));
    }
    Ok(UiPlan {
        mode,
        dist_dir: ui_path_text(&dist_dir)?,
        source_dir: ui_path_text(&source_dir)?,
        serve_url: context.serve_url.clone(),
        commands,
        notes,
    })
}

fn ui_mode(options: &UiOptions) -> &'static str {
    if ui_has_flag(options, UI_FLAG_INSTALL) {
        "install"
    } else if ui_has_flag(options, UI_FLAG_SOURCE) {
        "source"
    } else if ui_has_flag(options, UI_FLAG_DEV) {
        "dev"
    } else if ui_has_flag(options, UI_FLAG_TUNNEL) {
        "tunnel"
    } else {
        "launch"
    }
}

fn ui_validate_context(context: &UiContext) -> Result<(), String> {
    ui_validate_path(&context.cwd, "cwd")?;
    ui_validate_path(&context.data_dir, "data dir")?;
    ui_validate_url(&context.serve_url)?;
    Ok(())
}

fn ui_install_from_source(context: &UiContext, runner: &mut dyn UiExecRunner) -> Result<String, String> {
    ui_validate_context(context)?;
    ui_validate_repo_url(UI_SOURCE_REPO_URL)?;
    let temp_root = ui_temp_root(context)?;
    let source_dir = temp_root.join("src");
    let dist_dir = context.data_dir.join("dist");
    let result = ui_install_from_source_inner(context, runner, &temp_root, &source_dir, &dist_dir);
    let cleanup = ui_bounded_remove_dir_all(&temp_root, &temp_root, true);
    match (result, cleanup) {
        (Ok(stdout), Ok(_)) => Ok(stdout),
        (Ok(_), Err(err)) | (Err(err), Ok(_)) => Err(err),
        (Err(err), Err(cleanup_err)) => Err(format!("{err}; cleanup failed: {cleanup_err}")),
    }
}

fn ui_install_from_source_inner(
    context: &UiContext,
    runner: &mut dyn UiExecRunner,
    temp_root: &std::path::Path,
    source_dir: &std::path::Path,
    dist_dir: &std::path::Path,
) -> Result<String, String> {
    std::fs::create_dir_all(temp_root).map_err(|err| format!("ui: failed to create temp root: {err}"))?;
    ui_validate_child_path(temp_root, source_dir, "source checkout")?;
    let source_arg = ui_path_text(source_dir)?;
    ui_exec_checked(
        runner,
        "git",
        &ui_strings(&["clone", "--depth", "1", UI_SOURCE_REPO_URL, source_arg.as_str()]),
        None,
        "git clone maw-ui",
    )?;
    ui_exec_checked(runner, "bun", &ui_strings(&["install"]), Some(source_dir), "bun install")?;
    ui_exec_checked(
        runner,
        "bun",
        &ui_strings(&["run", "build"]),
        Some(source_dir),
        "bun run build",
    )?;
    let built_dist = source_dir.join("dist");
    let built_index = built_dist.join("index.html");
    if !built_index.is_file() {
        return Err(format!("ui: maw-ui build did not produce {}", built_index.display()));
    }
    ui_install_built_dist(context, &built_dist, dist_dir)?;
    if let Some(marker) = ui_source_marker_ref(runner, source_dir)? {
        std::fs::write(dist_dir.join(UI_VERSION_MARKER), format!("{marker}\n"))
            .map_err(|err| format!("ui: failed to write version marker: {err}"))?;
    }
    let count = ui_count_top_level_entries(dist_dir)?;
    Ok(format!(
        "⚡ building maw-ui default branch from source {UI_SOURCE_REPO_URL}...\n✓ maw-ui default branch built and installed → {} ({count} top-level entries)\n  → restart maw server to serve the new UI: pm2 restart maw OR maw serve\n",
        dist_dir.display()
    ))
}

fn ui_exec_checked(
    runner: &mut dyn UiExecRunner,
    program: &str,
    args: &[String],
    cwd: Option<&std::path::Path>,
    label: &str,
) -> Result<UiExecOutput, String> {
    let output = runner.ui_exec(program, args, cwd, UI_SOURCE_TIMEOUT_MS)?;
    if output.code == 0 {
        return Ok(output);
    }
    let detail = if output.stderr.trim().is_empty() {
        ui_redact_exec_output(&output.stdout)
    } else {
        ui_redact_exec_output(&output.stderr)
    };
    Err(format!("ui: {label} failed (exit {}): {detail}", output.code))
}

fn ui_source_marker_ref(runner: &mut dyn UiExecRunner, source_dir: &std::path::Path) -> Result<Option<String>, String> {
    let source_arg = ui_path_text(source_dir)?;
    let output = ui_exec_checked(
        runner,
        "git",
        &ui_strings(&["-C", source_arg.as_str(), "rev-parse", "--short", "HEAD"]),
        None,
        "git rev-parse",
    )?;
    let rev = output.stdout.trim();
    if rev.is_empty() {
        return Ok(None);
    }
    if !rev.chars().all(|ch| ch.is_ascii_hexdigit()) || rev.len() > 40 {
        return Err("ui: git rev-parse returned an invalid revision".to_owned());
    }
    Ok(Some(format!("source:{rev}")))
}

fn ui_install_built_dist(context: &UiContext, built_dist: &std::path::Path, dist_dir: &std::path::Path) -> Result<(), String> {
    ui_validate_path(built_dist, "built dist")?;
    ui_validate_path(dist_dir, "dist dir")?;
    std::fs::create_dir_all(&context.data_dir).map_err(|err| format!("ui: failed to create ui data dir: {err}"))?;
    let _ = ui_bounded_remove_dir_all(&context.data_dir, dist_dir, false)?;
    std::fs::create_dir_all(dist_dir).map_err(|err| format!("ui: failed to create dist dir: {err}"))?;
    ui_copy_dir_recursive(built_dist, dist_dir)
}

fn ui_copy_dir_recursive(source: &std::path::Path, target: &std::path::Path) -> Result<(), String> {
    for entry in std::fs::read_dir(source).map_err(|err| format!("ui: failed to read built dist: {err}"))? {
        let entry = entry.map_err(|err| format!("ui: failed to read built dist entry: {err}"))?;
        let file_type = entry.file_type().map_err(|err| format!("ui: failed to inspect built dist entry: {err}"))?;
        if file_type.is_symlink() {
            return Err("ui: built dist symlink is rejected".to_owned());
        }
        let next_target = target.join(entry.file_name());
        if file_type.is_dir() {
            std::fs::create_dir_all(&next_target).map_err(|err| format!("ui: failed to create dist subdir: {err}"))?;
            ui_copy_dir_recursive(&entry.path(), &next_target)?;
        } else if file_type.is_file() {
            std::fs::copy(entry.path(), &next_target).map_err(|err| format!("ui: failed to copy dist file: {err}"))?;
        }
    }
    Ok(())
}

fn ui_bounded_remove_dir_all(
    root: &std::path::Path,
    target: &std::path::Path,
    allow_root: bool,
) -> Result<bool, String> {
    if !target.exists() {
        return Ok(false);
    }
    let canonical_root = root
        .canonicalize()
        .map_err(|err| format!("ui: failed to canonicalize cleanup root: {err}"))?;
    let canonical_target = target
        .canonicalize()
        .map_err(|err| format!("ui: failed to canonicalize cleanup target: {err}"))?;
    if canonical_target == canonical_root && !allow_root {
        return Err("ui: refusing to remove cleanup root".to_owned());
    }
    if !canonical_target.starts_with(&canonical_root) {
        return Err("ui: cleanup target escaped root".to_owned());
    }
    std::fs::remove_dir_all(&canonical_target).map_err(|err| format!("ui: failed to remove cleanup target: {err}"))?;
    Ok(true)
}

fn ui_count_top_level_entries(path: &std::path::Path) -> Result<usize, String> {
    Ok(std::fs::read_dir(path)
        .map_err(|err| format!("ui: failed to count dist entries: {err}"))?
        .count())
}

fn ui_validate_path(path: &std::path::Path, label: &str) -> Result<(), String> {
    let text = ui_path_text(path)?;
    if text == "--" || text.starts_with('-') || text.contains('\0') || text.contains('\n') || text.contains('\r') {
        return Err(format!("ui: {label} path is rejected"));
    }
    if path.components().any(ui_rejected_component) {
        return Err(format!("ui: {label} path contains a rejected segment"));
    }
    Ok(())
}

fn ui_path_text(path: &std::path::Path) -> Result<String, String> {
    let text = path.to_string_lossy();
    if text.is_empty() {
        return Err("ui: empty path is rejected".to_owned());
    }
    Ok(text.into_owned())
}

fn ui_rejected_component(component: std::path::Component<'_>) -> bool {
    matches!(component, std::path::Component::ParentDir)
        || component
            .as_os_str()
            .to_str()
            .is_some_and(|segment| segment == "--" || segment.starts_with('-') || segment.chars().any(char::is_control))
}

fn ui_validate_url(value: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("ui: serve url is rejected".to_owned());
    }
    let Some(rest) = value.strip_prefix("http://").or_else(|| value.strip_prefix("https://")) else {
        return Err("ui: serve url must be http(s)".to_owned());
    };
    if rest.is_empty() || rest.contains(' ') || rest.contains("..") {
        return Err("ui: serve url host/path is rejected".to_owned());
    }
    Ok(())
}

fn ui_validate_repo_url(value: &str) -> Result<(), String> {
    if value != UI_SOURCE_REPO_URL {
        return Err("ui: source repo url is rejected".to_owned());
    }
    if !value.starts_with("https://github.com/") || value.chars().any(char::is_control) {
        return Err("ui: source repo url is rejected".to_owned());
    }
    Ok(())
}

fn ui_validate_child_path(root: &std::path::Path, child: &std::path::Path, label: &str) -> Result<(), String> {
    ui_validate_path(root, "path root")?;
    ui_validate_path(child, label)?;
    if child == root || !child.starts_with(root) {
        return Err(format!("ui: {label} path escaped root"));
    }
    Ok(())
}

fn ui_validate_program(program: &str) -> Result<(), String> {
    if matches!(program, "git" | "bun") {
        Ok(())
    } else {
        Err("ui: unsupported exec program".to_owned())
    }
}

fn ui_validate_exec_args(args: &[String]) -> Result<(), String> {
    if args.is_empty() {
        return Err("ui: empty exec argv is rejected".to_owned());
    }
    for arg in args {
        if arg == "--" || arg.contains('\0') || arg.chars().any(|ch| matches!(ch, '\n' | '\r')) {
            return Err("ui: exec argv contains a rejected argument".to_owned());
        }
    }
    Ok(())
}

fn ui_redact_exec_output(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return "<no output>".to_owned();
    }
    if trimmed.to_ascii_lowercase().contains("token")
        || trimmed.to_ascii_lowercase().contains("secret")
        || trimmed.to_ascii_lowercase().contains("authorization")
    {
        return "<redacted>".to_owned();
    }
    trimmed.to_owned()
}

fn ui_temp_root(context: &UiContext) -> Result<std::path::PathBuf, String> {
    static TEMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| format!("ui: invalid system time: {err}"))?
        .as_nanos();
    let seq = TEMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("maw-ui-src-{}-{nanos}-{seq}", std::process::id()));
    ui_validate_path(&root, "temp root")?;
    ui_validate_path(&context.data_dir, "data dir")?;
    Ok(root)
}

fn ui_strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn ui_command(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

fn ui_render_plan(plan: &UiPlan) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "ui plan:");
    let _ = writeln!(out, "  mode: {}", plan.mode);
    let _ = writeln!(out, "  dist: {}", plan.dist_dir);
    let _ = writeln!(out, "  source: {}", plan.source_dir);
    let _ = writeln!(out, "  serve: {}", plan.serve_url);
    for command in &plan.commands {
        let _ = writeln!(out, "  - {}", ui_shell_words(command));
    }
    for note in &plan.notes {
        let _ = writeln!(out, "  note: {note}");
    }
    out
}

fn ui_shell_words(words: &[String]) -> String {
    words.iter().map(|word| ui_shell_word(word)).collect::<Vec<_>>().join(" ")
}

fn ui_shell_word(word: &str) -> String {
    if word.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '/' | '.' | '_' | '-' | ':' )) {
        word.to_owned()
    } else {
        format!("'{}'", word.replace('\'', "'\\''"))
    }
}

#[cfg(test)]
mod ui_tests {
    use super::*;

    fn ui_context() -> UiContext {
        UiContext {
            cwd: std::path::PathBuf::from("/tmp/maw-ui-test/repo"),
            data_dir: std::path::PathBuf::from("/tmp/maw-ui-test/data/ui"),
            serve_url: "http://127.0.0.1:1977".to_owned(),
        }
    }

    fn ui_temp_context(name: &str) -> UiContext {
        let root = std::env::temp_dir().join(format!(
            "maw-ui-source-test-{}-{name}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        let cwd = root.join("repo");
        std::fs::create_dir_all(&cwd).expect("cwd");
        UiContext {
            cwd,
            data_dir: root.join("data").join("ui"),
            serve_url: "http://127.0.0.1:1977".to_owned(),
        }
    }

    #[derive(Debug, Default)]
    struct UiFakeRunner {
        calls: Vec<(String, Vec<String>, Option<std::path::PathBuf>)>,
        skip_dist: bool,
        fail_bun_install: bool,
    }

    impl UiExecRunner for UiFakeRunner {
        fn ui_exec(
            &mut self,
            program: &str,
            args: &[String],
            cwd: Option<&std::path::Path>,
            _timeout_ms: u64,
        ) -> Result<UiExecOutput, String> {
            self.calls
                .push((program.to_owned(), args.to_vec(), cwd.map(std::path::Path::to_path_buf)));
            if self.fail_bun_install && program == "bun" && args == ui_strings(&["install"]) {
                return Ok(UiExecOutput {
                    code: 17,
                    stdout: String::new(),
                    stderr: "token should not leak".to_owned(),
                });
            }
            if program == "git" && args.len() == 5 && args[0] == "clone" {
                std::fs::create_dir_all(args.last().expect("dest")).expect("clone dest");
                return Ok(UiExecOutput {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            }
            if program == "bun" && args == ui_strings(&["run", "build"]) && !self.skip_dist {
                let dist = cwd.expect("cwd").join("dist");
                std::fs::create_dir_all(&dist).expect("dist");
                std::fs::write(dist.join("index.html"), "<html></html>").expect("index");
                std::fs::write(dist.join("app.js"), "console.log('ok');").expect("asset");
            }
            if program == "git" && args.len() == 5 && args[2] == "rev-parse" {
                return Ok(UiExecOutput {
                    code: 0,
                    stdout: "abc1234\n".to_owned(),
                    stderr: String::new(),
                });
            }
            Ok(UiExecOutput {
                code: 0,
                stdout: String::new(),
                stderr: String::new(),
            })
        }
    }

    #[test]
    fn ui_dispatch_registers_command() {
        assert_eq!(DISPATCH_100.len(), 1);
        assert_eq!(DISPATCH_100[0].command, "ui");
        assert_eq!(dispatcher_status("ui"), DispatchKind::Native);
    }

    #[test]
    fn ui_parse_flags_and_renders_plan_without_exec() {
        let output = ui_run(&ui_strings(&["--install", "--dev", "--3d"]), &ui_context()).expect("plan");
        assert!(output.contains("mode: install"));
        assert!(output.contains("install /tmp/maw-ui-test/data/ui/dist"));
        assert!(output.contains("dev /tmp/maw-ui-test/repo/ui"));
        assert!(output.contains("3d UI mode requested"));
    }

    #[test]
    fn ui_source_build_uses_fake_runner_and_real_end_state() {
        let context = ui_temp_context("success");
        let mut runner = UiFakeRunner::default();
        let output = ui_run_with_runner(&ui_strings(&["--source"]), &context, &mut runner).expect("source build");
        assert!(output.contains("maw-ui default branch built and installed"));
        let dist = context.data_dir.join("dist");
        assert!(dist.join("index.html").is_file());
        assert!(dist.join("app.js").is_file());
        assert_eq!(
            std::fs::read_to_string(dist.join(UI_VERSION_MARKER)).expect("marker"),
            "source:abc1234\n"
        );
        assert_eq!(runner.calls.len(), 4);
        assert_eq!(runner.calls[0].0, "git");
        assert_eq!(&runner.calls[0].1[..4], ["clone", "--depth", "1", UI_SOURCE_REPO_URL]);
        let source_dir = std::path::PathBuf::from(runner.calls[0].1.last().expect("source dir"));
        assert!(!source_dir.parent().expect("temp root").exists());
        assert_eq!(
            runner.calls[1],
            ("bun".to_owned(), ui_strings(&["install"]), Some(source_dir.clone()))
        );
        assert_eq!(
            runner.calls[2],
            (
                "bun".to_owned(),
                ui_strings(&["run", "build"]),
                Some(source_dir.clone())
            )
        );
        assert_eq!(
            runner.calls[3],
            (
                "git".to_owned(),
                ui_strings(&["-C", source_dir.to_string_lossy().as_ref(), "rev-parse", "--short", "HEAD"]),
                None
            )
        );
    }

    #[test]
    fn ui_source_build_requires_index_and_does_not_use_network_in_tests() {
        let context = ui_temp_context("missing-index");
        let mut runner = UiFakeRunner {
            skip_dist: true,
            ..UiFakeRunner::default()
        };
        let err = ui_run_with_runner(&ui_strings(&["--source"]), &context, &mut runner).expect_err("missing index");
        assert!(err.contains("did not produce"));
        assert!(!context.data_dir.join("dist").join("index.html").exists());
        assert!(runner.calls.iter().all(|(program, _, _)| program == "git" || program == "bun"));
    }

    #[test]
    fn ui_source_build_redacts_exec_failure_output() {
        let context = ui_temp_context("redact");
        let mut runner = UiFakeRunner {
            fail_bun_install: true,
            ..UiFakeRunner::default()
        };
        let err = ui_run_with_runner(&ui_strings(&["--source"]), &context, &mut runner).expect_err("bun failed");
        assert!(err.contains("<redacted>"));
        assert!(!err.contains("token should not leak"));
    }

    #[test]
    fn ui_tunnel_is_stubbed_until_live_daemon_contract() {
        let output = ui_run(&ui_strings(&["--tunnel"]), &ui_context()).expect("plan");
        assert!(output.contains("tunnel http://127.0.0.1:1977"));
        assert!(output.contains("/api/orchestration/workon"));
    }

    #[test]
    fn ui_version_is_native_and_does_not_need_context() {
        let output = ui_run(&ui_strings(&["--version"]), &ui_context()).expect("version");
        assert!(output.starts_with("maw ui "));
    }

    #[test]
    fn ui_rejects_separator_unknown_flags_and_bad_context() {
        let err = ui_run(&ui_strings(&["--"]), &ui_context()).expect_err("separator");
        assert!(err.contains("separator"));
        let err = ui_run(&ui_strings(&["--bad"]), &ui_context()).expect_err("flag");
        assert!(err.contains("not a supported flag"));
        let mut context = ui_context();
        context.serve_url = "file:///tmp/app".to_owned();
        let err = ui_run(&ui_strings(&["--tunnel"]), &context).expect_err("url");
        assert!(err.contains("http(s)"));
        context.serve_url = "http://127.0.0.1:1977".to_owned();
        context.cwd = std::path::PathBuf::from("-bad");
        let err = ui_run(&ui_strings(&[]), &context).expect_err("path");
        assert!(err.contains("path"));
    }
}
