const DISPATCH_90: &[DispatcherEntry] = &[DispatcherEntry {
    command: "oracle-workon",
    handler: Handler::Sync(oracleworkon_run_command),
}];

const ORACLEWORKON_USAGE: &str = "usage: maw oracle-workon <repo> [task] [--dry-run] [--work <repo>] [--task <slug>] [--engine <name>] [--prompt <text>] [--with <oracle>] [--all --force --no-attach --split --tiled]";

type OracleworkonFleetLoader = fn() -> Vec<NativeFleetSession>;

const ORACLEWORKON_FLAG_DRY_RUN: u8 = 1 << 0;
const ORACLEWORKON_FLAG_ALL: u8 = 1 << 1;
const ORACLEWORKON_FLAG_FORCE: u8 = 1 << 2;
const ORACLEWORKON_FLAG_NO_ATTACH: u8 = 1 << 3;
const ORACLEWORKON_FLAG_SPLIT: u8 = 1 << 4;
const ORACLEWORKON_FLAG_TILED: u8 = 1 << 5;

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleworkonOptions {
    targets: Vec<String>,
    task: Option<String>,
    flags: u8,
    engine: Option<String>,
    prompt: Option<String>,
    with: Vec<String>,
}

fn oracleworkon_run_command(argv: &[String]) -> CliOutput {
    oracleworkon_run_command_with(argv, load_native_fleet)
}

fn oracleworkon_run_command_with(argv: &[String], load_fleet: OracleworkonFleetLoader) -> CliOutput {
    match oracleworkon_run(argv, load_fleet) {
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

fn oracleworkon_run(argv: &[String], load_fleet: OracleworkonFleetLoader) -> Result<String, String> {
    let orchestrator = crate::serve_core::ServecoreCommandOrchestrator::servecore_with_root(
        oracleworkon_orchestration_root(),
    );
    oracleworkon_run_with_orchestrator(argv, load_fleet, &orchestrator)
}

fn oracleworkon_run_with_orchestrator(
    argv: &[String],
    load_fleet: OracleworkonFleetLoader,
    orchestrator: &dyn crate::serve_core::ServecoreOrchestrator,
) -> Result<String, String> {
    let mut options = oracleworkon_parse_args(argv)?;
    if oracleworkon_has_flag(&options, ORACLEWORKON_FLAG_ALL) {
        options.targets = oracleworkon_all_targets(load_fleet())?;
    }
    oracleworkon_validate_options(&options)?;
    if oracleworkon_has_flag(&options, ORACLEWORKON_FLAG_DRY_RUN) || oracleworkon_is_planning_only(&options) {
        return Ok(oracleworkon_render_plan(&options));
    }
    if oracleworkon_needs_orchestration(&options) {
        return oracleworkon_spawn_with_orchestrator(&options, orchestrator);
    }
    oracleworkon_run_single_workon(&options)
}

fn oracleworkon_parse_args(argv: &[String]) -> Result<OracleworkonOptions, String> {
    let mut options = oracleworkon_default_options();
    let mut positionals = Vec::<String>::new();
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" | "help" => return Err(ORACLEWORKON_USAGE.to_owned()),
            "--" => return Err("oracle-workon: -- separator is not allowed".to_owned()),
            "--all" => oracleworkon_set_flag(&mut options, ORACLEWORKON_FLAG_ALL),
            "--dry-run" => oracleworkon_set_flag(&mut options, ORACLEWORKON_FLAG_DRY_RUN),
            "--force" => oracleworkon_set_flag(&mut options, ORACLEWORKON_FLAG_FORCE),
            "--no-attach" => oracleworkon_set_flag(&mut options, ORACLEWORKON_FLAG_NO_ATTACH),
            "--split" => oracleworkon_set_flag(&mut options, ORACLEWORKON_FLAG_SPLIT),
            "--tiled" => oracleworkon_set_flag(&mut options, ORACLEWORKON_FLAG_TILED),
            "--work" => options.targets.push(oracleworkon_take_value(argv, &mut index, "--work")?),
            "--task" => options.task = Some(oracleworkon_take_value(argv, &mut index, "--task")?),
            "--engine" => options.engine = Some(oracleworkon_take_value(argv, &mut index, "--engine")?),
            "--prompt" => options.prompt = Some(oracleworkon_take_value(argv, &mut index, "--prompt")?),
            "--with" => options.with.push(oracleworkon_take_value(argv, &mut index, "--with")?),
            value if value.starts_with("--work=") => options.targets.push(value["--work=".len()..].to_owned()),
            value if value.starts_with("--task=") => options.task = Some(value["--task=".len()..].to_owned()),
            value if value.starts_with("--engine=") => options.engine = Some(value["--engine=".len()..].to_owned()),
            value if value.starts_with("--prompt=") => options.prompt = Some(value["--prompt=".len()..].to_owned()),
            value if value.starts_with("--with=") => options.with.push(value["--with=".len()..].to_owned()),
            value if value.starts_with('-') => return Err(oracleworkon_flag_like_value(value)),
            value => positionals.push(value.to_owned()),
        }
        index += 1;
    }
    oracleworkon_apply_positionals(&mut options, &positionals)?;
    Ok(options)
}

fn oracleworkon_default_options() -> OracleworkonOptions {
    OracleworkonOptions {
        targets: Vec::new(),
        task: None,
        flags: 0,
        engine: None,
        prompt: None,
        with: Vec::new(),
    }
}

fn oracleworkon_set_flag(options: &mut OracleworkonOptions, flag: u8) {
    options.flags |= flag;
}

fn oracleworkon_has_flag(options: &OracleworkonOptions, flag: u8) -> bool {
    options.flags & flag != 0
}

fn oracleworkon_take_value(argv: &[String], index: &mut usize, flag: &str) -> Result<String, String> {
    let Some(value) = argv.get(*index + 1) else { return Err(format!("oracle-workon: {flag} requires a value")); };
    if value == "--" || value.starts_with('-') {
        return Err(format!("oracle-workon: {flag} value must not start with '-'"));
    }
    *index += 1;
    Ok(value.to_owned())
}

fn oracleworkon_apply_positionals(options: &mut OracleworkonOptions, positionals: &[String]) -> Result<(), String> {
    if positionals.len() > 2 {
        return Err(ORACLEWORKON_USAGE.to_owned());
    }
    if let Some(repo) = positionals.first() {
        options.targets.push(repo.clone());
    }
    if let Some(task) = positionals.get(1) {
        if options.task.is_some() {
            return Err("oracle-workon: task specified twice".to_owned());
        }
        options.task = Some(task.clone());
    }
    Ok(())
}

fn oracleworkon_validate_options(options: &OracleworkonOptions) -> Result<(), String> {
    if options.targets.is_empty() {
        return Err(ORACLEWORKON_USAGE.to_owned());
    }
    for target in &options.targets {
        oracleworkon_validate_target(target, "target")?;
    }
    if let Some(task) = &options.task {
        oracleworkon_validate_target(task, "task")?;
    }
    if let Some(engine) = &options.engine {
        oracleworkon_validate_word(engine, "engine")?;
    }
    if let Some(prompt) = &options.prompt {
        oracleworkon_validate_prompt(prompt)?;
    }
    for item in &options.with {
        oracleworkon_validate_word(item, "with")?;
    }
    Ok(())
}

fn oracleworkon_validate_target(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') || value.contains("..") {
        return Err(format!("oracle-workon {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("oracle-workon {label} must not contain whitespace or control characters"));
    }
    Ok(())
}

fn oracleworkon_validate_word(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value == "--" || value.starts_with('-') {
        return Err(format!("oracle-workon {label} must be non-empty, unpadded, and not start with '-'"));
    }
    if value.chars().any(|ch| ch.is_control() || ch.is_whitespace()) {
        return Err(format!("oracle-workon {label} must not contain whitespace or control characters"));
    }
    Ok(())
}

fn oracleworkon_validate_prompt(value: &str) -> Result<(), String> {
    if value.is_empty() || value == "--" || value.starts_with('-') || value.chars().any(char::is_control) {
        return Err("oracle-workon prompt must be non-empty, not start with '-', and contain no control characters".to_owned());
    }
    Ok(())
}

fn oracleworkon_flag_like_value(value: &str) -> String {
    format!("\"{value}\" looks like a flag, not a target.\n  {ORACLEWORKON_USAGE}")
}

fn oracleworkon_all_targets(fleet: Vec<NativeFleetSession>) -> Result<Vec<String>, String> {
    let mut out = Vec::<String>::new();
    for session in fleet {
        for repo in session.project_repos {
            oracleworkon_validate_target(&repo, "project repo")?;
            out.push(repo);
        }
        for window in session.windows {
            if window.repo.is_empty() {
                continue;
            }
            oracleworkon_validate_target(&window.repo, "window repo")?;
            out.push(window.repo);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn oracleworkon_is_planning_only(options: &OracleworkonOptions) -> bool {
    options.targets.len() != 1 || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_ALL)
}

fn oracleworkon_needs_orchestration(options: &OracleworkonOptions) -> bool {
    oracleworkon_has_flag(options, ORACLEWORKON_FLAG_FORCE)
        || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_NO_ATTACH)
        || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_SPLIT)
        || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_TILED)
        || options.engine.is_some()
        || options.prompt.is_some()
        || !options.with.is_empty()
}

fn oracleworkon_render_plan(options: &OracleworkonOptions) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "oracle-workon plan:");
    for target in &options.targets {
        let _ = writeln!(out, "  - {}", oracleworkon_workon_command(target, options.task.as_deref()));
    }
    if options.engine.is_some()
        || options.prompt.is_some()
        || !options.with.is_empty()
        || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_NO_ATTACH)
        || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_SPLIT)
        || oracleworkon_has_flag(options, ORACLEWORKON_FLAG_TILED)
    {
        out.push_str("  note: simple workon execution is live; advanced daemon orchestration is available through the native serve orchestrator.\n");
    }
    out
}

fn oracleworkon_workon_command(target: &str, task: Option<&str>) -> String {
    match task {
        Some(task) => format!("maw workon {target} {task} --layout nested"),
        None => format!("maw workon {target} --layout nested"),
    }
}


fn oracleworkon_spawn_with_orchestrator(
    options: &OracleworkonOptions,
    orchestrator: &dyn crate::serve_core::ServecoreOrchestrator,
) -> Result<String, String> {
    let target = options.targets.first().ok_or_else(|| ORACLEWORKON_USAGE.to_owned())?;
    let request = crate::serve_core::ServecoreWorkonRequest {
        repo: target.clone(),
        task: options.task.clone(),
        engine: options.engine.clone(),
        target: (!oracleworkon_has_flag(options, ORACLEWORKON_FLAG_NO_ATTACH)).then(|| target.clone()),
        prompt: options.prompt.clone(),
        with_oracles: options.with.clone(),
        attach: !oracleworkon_has_flag(options, ORACLEWORKON_FLAG_NO_ATTACH),
        split: oracleworkon_has_flag(options, ORACLEWORKON_FLAG_SPLIT),
        tiled: oracleworkon_has_flag(options, ORACLEWORKON_FLAG_TILED),
    };
    let handle = orchestrator.spawn_workon(
        request,
        std::sync::Arc::new(crate::serve_core::ServecoreNativeEngine),
    )?;
    Ok(oracleworkon_render_spawn(&handle))
}

fn oracleworkon_render_spawn(handle: &crate::serve_core::ServecoreWorkonHandle) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "oracle-workon spawned:");
    let _ = writeln!(out, "  repo: {}", handle.repo);
    let _ = writeln!(out, "  cwd: {}", handle.cwd);
    let _ = writeln!(out, "  engine: {}", handle.engine);
    let _ = writeln!(out, "  argv: {}", handle.argv.join(" "));
    out
}

fn oracleworkon_orchestration_root() -> std::path::PathBuf {
    std::env::var_os("GHQ_ROOT").map_or_else(
        || std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
        std::path::PathBuf::from,
    )
}

fn oracleworkon_run_single_workon(options: &OracleworkonOptions) -> Result<String, String> {
    let target = options.targets.first().ok_or_else(|| ORACLEWORKON_USAGE.to_owned())?;
    let mut args = vec![target.clone()];
    if let Some(task) = &options.task {
        args.push(task.clone());
    }
    args.extend(["--layout".to_owned(), "nested".to_owned()]);
    let output = run_workon_command(&args);
    if output.code == 0 {
        Ok(output.stdout)
    } else {
        Err(output.stderr.trim_end().to_owned())
    }
}

#[cfg(test)]
mod oracleworkon_tests {
    use super::*;


    #[derive(Default)]
    struct OracleworkonFakeOrchestrator {
        calls: std::sync::Mutex<Vec<crate::serve_core::ServecoreWorkonRequest>>,
    }

    impl crate::serve_core::ServecoreOrchestrator for OracleworkonFakeOrchestrator {
        fn spawn_workon(
            &self,
            request: crate::serve_core::ServecoreWorkonRequest,
            _engine: std::sync::Arc<dyn crate::serve_core::ServecoreEngine>,
        ) -> Result<crate::serve_core::ServecoreWorkonHandle, String> {
            self.calls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(request.clone());
            Ok(crate::serve_core::ServecoreWorkonHandle {
                ok: true,
                repo: request.repo,
                cwd: "/tmp/fake".to_owned(),
                engine: request.engine.unwrap_or_else(|| "stub".to_owned()),
                target: request.target,
                argv: vec!["workon".to_owned(), "demo".to_owned()],
                status: "fake".to_owned(),
                message: None,
                leader_argv: None,
                swarm_argv: None,
                pane: None,
                swarm_skipped: None,
            })
        }
    }

    struct OracleworkonEnvGuard {
        saved: Vec<(&'static str, Option<std::ffi::OsString>)>,
    }

    impl OracleworkonEnvGuard {
        fn oracleworkon_new() -> Self {
            let keys = ["HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME", "XDG_DATA_HOME", "XDG_CACHE_HOME", "TMUX", "PATH", "GHQ_ROOT"];
            let saved = keys.into_iter().map(|key| (key, std::env::var_os(key))).collect::<Vec<_>>();
            let root = std::env::temp_dir().join(format!("maw-oracleworkon-test-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&root);
            std::fs::create_dir_all(root.join("bin")).expect("bin");
            std::fs::create_dir_all(root.join("ghq/github.com/acme/demo")).expect("repo");
            std::fs::create_dir_all(root.join("xdg-config/maw")).expect("config");
            std::fs::write(root.join("xdg-config/maw/maw.config.json"), r#"{"commands":{"default":"echo launch"}}"#).expect("config");
            oracleworkon_write_fake_tmux(&root);
            std::env::set_var("HOME", root.join("home"));
            std::env::set_var("XDG_CONFIG_HOME", root.join("xdg-config"));
            std::env::set_var("XDG_STATE_HOME", root.join("xdg-state"));
            std::env::set_var("XDG_DATA_HOME", root.join("xdg-data"));
            std::env::set_var("XDG_CACHE_HOME", root.join("xdg-cache"));
            std::env::set_var("TMUX", "/tmp/tmux,1,0");
            std::env::set_var("PATH", root.join("bin"));
            std::env::set_var("GHQ_ROOT", root.join("ghq"));
            Self { saved }
        }
    }

    impl Drop for OracleworkonEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved.drain(..) {
                if let Some(value) = value { std::env::set_var(key, value); } else { std::env::remove_var(key); }
            }
        }
    }

    fn oracleworkon_write_fake_tmux(root: &std::path::Path) {
        let tmux = root.join("bin/tmux");
        std::fs::write(
            &tmux,
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$HOME/tmux.log"
case "$1" in
  display-message) printf '50-mawjs\n' ;;
  list-windows) printf 'demo\n' ;;
  select-window|new-window|send-keys) exit 0 ;;
  *) exit 9 ;;
esac
"#,
        )
        .expect("tmux");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&tmux).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&tmux, permissions).expect("chmod");
        }
    }

    fn oracleworkon_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    fn oracleworkon_session(name: &str, repos: &[&str]) -> NativeFleetSession {
        NativeFleetSession {
            name: name.to_owned(),
            project_repos: repos.iter().map(|repo| (*repo).to_owned()).collect(),
            windows: repos
                .iter()
                .map(|repo| NativeFleetWindow { name: repo.rsplit('/').next().unwrap_or(repo).to_owned(), repo: (*repo).to_owned() })
                .collect(),
            ..NativeFleetSession::default()
        }
    }

    fn oracleworkon_fleet() -> Vec<NativeFleetSession> {
        vec![oracleworkon_session("01-wish", &["acme/demo", "acme/other"])]
    }

    fn oracleworkon_bad_fleet() -> Vec<NativeFleetSession> {
        vec![oracleworkon_session("bad", &["../bad"])]
    }

    #[test]
    fn oracleworkon_dispatch_registers_command() {
        assert_eq!(DISPATCH_90.len(), 1);
        assert_eq!(DISPATCH_90[0].command, "oracle-workon");
    }

    #[test]
    fn oracleworkon_parse_flags_and_dry_run_plan() {
        let args = oracleworkon_strings(&["demo", "--task", "feat", "--dry-run", "--engine", "codex", "--prompt", "ship it", "--with", "nova", "--split"]);
        let output = oracleworkon_run(&args, oracleworkon_fleet).expect("plan");
        assert!(output.contains("maw workon demo feat --layout nested"));
        assert!(output.contains("advanced daemon orchestration is available through the native serve orchestrator"));
    }

    #[test]
    fn oracleworkon_all_uses_seeded_fleet_without_real_config() {
        let output = oracleworkon_run(&oracleworkon_strings(&["--all", "--dry-run"]), oracleworkon_fleet).expect("all");
        assert!(output.contains("maw workon acme/demo --layout nested"));
        assert!(output.contains("maw workon acme/other --layout nested"));
    }

    #[test]
    fn oracleworkon_rejects_guards_before_fleet_or_workon() {
        let err = oracleworkon_run(&oracleworkon_strings(&["--", "demo"]), oracleworkon_bad_fleet).expect_err("sep");
        assert!(err.contains("-- separator"));
        let err = oracleworkon_run(&oracleworkon_strings(&["-bad"]), oracleworkon_bad_fleet).expect_err("flag");
        assert!(err.contains("looks like a flag"));
        let err = oracleworkon_run(&oracleworkon_strings(&["--all"]), oracleworkon_bad_fleet).expect_err("bad fleet");
        assert!(err.contains("project repo"));
    }

    #[test]
    fn oracleworkon_advanced_flags_use_orchestrator_instead_of_plan_only() {
        let fake = OracleworkonFakeOrchestrator::default();
        let output = oracleworkon_run_with_orchestrator(
            &oracleworkon_strings(&[
                "demo",
                "feat-219",
                "--engine",
                "codex-flex",
                "--with",
                "nova",
                "--split",
            ]),
            oracleworkon_fleet,
            &fake,
        )
        .expect("spawn");
        assert!(output.contains("oracle-workon spawned"));
        assert!(!output.contains("plan-only"));
        let calls = fake
            .calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].engine.as_deref(), Some("codex-flex"));
        assert_eq!(calls[0].with_oracles, vec!["nova"]);
        assert!(calls[0].split);
    }

    #[test]
    fn oracleworkon_single_target_delegates_to_native_workon_hermetically() {
        let _lock = super::env_test_lock().lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let _env = OracleworkonEnvGuard::oracleworkon_new();
        let output = oracleworkon_run(&oracleworkon_strings(&["demo"]), oracleworkon_fleet).expect("run");
        assert!(output.contains("reusing existing window 'demo'"));
    }
}
