const DISPATCH_67: &[DispatcherEntry] = &[DispatcherEntry {
    command: "setup",
    handler: Handler::Sync(run_setup_command),
}];

const SETUP_USAGE: &str =
    "maw setup auto-wake [--dry-run] [--user <name>] [--repo <path>] [--only <pm2-app>]";
const SETUP_DEFAULT_ONLY: &str = "maw-boot";

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupOptions {
    dry_run: bool,
    user: Option<String>,
    repo_root: Option<std::path::PathBuf>,
    only: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SetupStep {
    command: Vec<String>,
    skipped: bool,
}

trait SetupRunner {
    fn setup_cwd(&self) -> std::path::PathBuf;
    fn setup_user(&self) -> String;
    fn setup_platform(&self) -> &'static str;
    fn setup_exists(&self, path: &std::path::Path) -> bool;
    fn setup_run(&mut self, command: &[String], cwd: &std::path::Path) -> Result<String, String>;
}

struct SetupSystemRunner;

impl SetupRunner for SetupSystemRunner {
    fn setup_cwd(&self) -> std::path::PathBuf {
        std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
    }

    fn setup_user(&self) -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| "unknown".to_owned())
    }

    fn setup_platform(&self) -> &'static str {
        setup_current_platform()
    }

    fn setup_exists(&self, path: &std::path::Path) -> bool {
        path.exists()
    }

    fn setup_run(&mut self, command: &[String], cwd: &std::path::Path) -> Result<String, String> {
        setup_validate_command(command)?;
        let Some(program) = command.first() else {
            return Err("setup: empty command".to_owned());
        };
        let output = std::process::Command::new(program)
            .args(&command[1..])
            .current_dir(cwd)
            .output()
            .map_err(|error| format!("setup: failed to execute {program}: {error}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("setup: {program} failed: {}", stderr.trim()))
        }
    }
}

fn run_setup_command(argv: &[String]) -> CliOutput {
    match setup_run(argv, &mut SetupSystemRunner) {
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

fn setup_run(argv: &[String], runner: &mut impl SetupRunner) -> Result<String, String> {
    if argv.is_empty()
        || argv
            .iter()
            .any(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Ok(setup_usage_text());
    }
    let (subcommand, rest) = argv.split_first().expect("checked non-empty");
    if subcommand != "auto-wake" {
        return Err(format!(
            "unknown setup subcommand: {subcommand}\n{}",
            setup_usage_text()
        ));
    }
    let options = setup_parse_auto_wake_args(rest)?;
    let steps = setup_auto_wake(&options, runner)?;
    Ok(setup_render_auto_wake(&steps))
}

fn setup_usage_text() -> String {
    [
        SETUP_USAGE,
        "",
        "Registers the maw-boot PM2 one-shot so reboot restores the latest fleet snapshot:",
        "  loginctl enable-linger <user>",
        "  pm2 startup systemd -u <user> --hp <home>",
        "  pm2 start ecosystem.config.cjs --only maw-boot",
        "  pm2 save",
    ]
    .join("\n")
}

fn setup_parse_auto_wake_args(argv: &[String]) -> Result<SetupOptions, String> {
    let mut options = SetupOptions {
        dry_run: false,
        user: None,
        repo_root: None,
        only: SETUP_DEFAULT_ONLY.to_owned(),
    };
    let mut index = 0_usize;
    while index < argv.len() {
        setup_parse_auto_wake_arg(argv, &mut index, &mut options)?;
        index += 1;
    }
    Ok(options)
}

fn setup_parse_auto_wake_arg(
    argv: &[String],
    index: &mut usize,
    options: &mut SetupOptions,
) -> Result<(), String> {
    match argv[*index].as_str() {
        "--dry-run" => options.dry_run = true,
        "--user" => {
            let value = setup_required_value(argv, *index, "--user")?;
            setup_validate_user(value)?;
            options.user = Some(value.to_owned());
            *index += 1;
        }
        "--repo" => {
            let value = setup_required_value(argv, *index, "--repo")?;
            options.repo_root = Some(setup_validate_repo_arg(value)?);
            *index += 1;
        }
        "--only" => {
            let value = setup_required_value(argv, *index, "--only")?;
            setup_validate_pm2_name(value)?;
            value.clone_into(&mut options.only);
            *index += 1;
        }
        value if value.starts_with("--user=") => {
            let value = value.trim_start_matches("--user=");
            setup_validate_user(value)?;
            options.user = Some(value.to_owned());
        }
        value if value.starts_with("--repo=") => {
            options.repo_root = Some(setup_validate_repo_arg(
                value.trim_start_matches("--repo="),
            )?);
        }
        value if value.starts_with("--only=") => {
            let value = value.trim_start_matches("--only=");
            setup_validate_pm2_name(value)?;
            value.clone_into(&mut options.only);
        }
        value if value.starts_with('-') => return Err(format!("setup: unknown argument {value}")),
        value => return Err(format!("setup: unexpected argument {value}")),
    }
    Ok(())
}

fn setup_auto_wake(
    options: &SetupOptions,
    runner: &mut impl SetupRunner,
) -> Result<Vec<SetupStep>, String> {
    let platform = runner.setup_platform();
    if platform == "win32" && !options.dry_run {
        return Err(
            "maw setup auto-wake is only implemented for Linux/macOS service managers".to_owned(),
        );
    }
    if platform == "darwin" && !options.dry_run {
        return Err("macOS launchd auto-wake setup is not implemented yet; use maw fleet doctor --reboot for diagnostics".to_owned());
    }
    let repo = setup_repo_root(options, runner)?;
    let user = setup_user(options, runner)?;
    let commands = setup_commands(platform, &user, &options.only);
    let mut steps = Vec::<SetupStep>::new();
    for command in commands {
        steps.push(setup_run_step(command, options.dry_run, &repo, runner)?);
    }
    Ok(steps)
}

fn setup_repo_root(
    options: &SetupOptions,
    runner: &impl SetupRunner,
) -> Result<std::path::PathBuf, String> {
    let root = options
        .repo_root
        .clone()
        .unwrap_or_else(|| runner.setup_cwd());
    let root = setup_absolute_path(&root);
    let ecosystem = root.join("ecosystem.config.cjs");
    if !runner.setup_exists(&ecosystem) {
        return Err(format!(
            "ecosystem.config.cjs not found in {}; run from the maw-js repo or pass --repo <path>",
            root.display()
        ));
    }
    Ok(root)
}

fn setup_user(options: &SetupOptions, runner: &impl SetupRunner) -> Result<String, String> {
    let user = options.user.clone().unwrap_or_else(|| runner.setup_user());
    setup_validate_user(&user)?;
    Ok(user)
}

fn setup_commands(platform: &str, user: &str, only: &str) -> Vec<Vec<String>> {
    vec![
        vec![
            "loginctl".to_owned(),
            "enable-linger".to_owned(),
            user.to_owned(),
        ],
        vec![
            "pm2".to_owned(),
            "startup".to_owned(),
            "systemd".to_owned(),
            "-u".to_owned(),
            user.to_owned(),
            "--hp".to_owned(),
            setup_home_dir(platform, user),
        ],
        vec![
            "pm2".to_owned(),
            "start".to_owned(),
            "ecosystem.config.cjs".to_owned(),
            "--only".to_owned(),
            only.to_owned(),
        ],
        vec!["pm2".to_owned(), "save".to_owned()],
    ]
}

fn setup_run_step(
    command: Vec<String>,
    dry_run: bool,
    repo: &std::path::Path,
    runner: &mut impl SetupRunner,
) -> Result<SetupStep, String> {
    setup_validate_command(&command)?;
    if dry_run {
        return Ok(SetupStep {
            command,
            skipped: true,
        });
    }
    runner.setup_run(&command, repo)?;
    Ok(SetupStep {
        command,
        skipped: false,
    })
}

fn setup_render_auto_wake(steps: &[SetupStep]) -> String {
    let mut lines = vec!["maw setup auto-wake".to_owned()];
    for step in steps {
        let prefix = if step.skipped {
            "  · dry-run"
        } else {
            "  ✓"
        };
        lines.push(format!("{prefix} {}", step.command.join(" ")));
    }
    lines.push("  ✓ next reboot will restore fleet from the latest snapshot".to_owned());
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

fn setup_required_value<'a>(
    argv: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1) else {
        return Err(format!("setup: missing {flag} value"));
    };
    if value.starts_with('-') {
        return Err(format!("setup: {flag} value must not start with '-'"));
    }
    Ok(value)
}

fn setup_validate_user(value: &str) -> Result<(), String> {
    setup_validate_token(value, "user")
}

fn setup_validate_pm2_name(value: &str) -> Result<(), String> {
    setup_validate_token(value, "pm2 app")
}

fn setup_validate_token(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!("setup: invalid {label}"));
    }
    if value.contains('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("setup: invalid {label}"));
    }
    Ok(())
}

fn setup_validate_repo_arg(value: &str) -> Result<std::path::PathBuf, String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err("setup: invalid repo path".to_owned());
    }
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err("setup: invalid repo path".to_owned());
    }
    Ok(std::path::PathBuf::from(value))
}

fn setup_validate_command(command: &[String]) -> Result<(), String> {
    let Some(program) = command.first() else {
        return Err("setup: empty command".to_owned());
    };
    setup_validate_program(program)?;
    for arg in &command[1..] {
        setup_validate_command_arg(arg)?;
    }
    Ok(())
}

fn setup_validate_program(program: &str) -> Result<(), String> {
    if program.is_empty() || program.starts_with('-') || program.contains('/') {
        return Err("setup: invalid executable name".to_owned());
    }
    if program
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err("setup: invalid executable name".to_owned());
    }
    Ok(())
}

fn setup_validate_command_arg(arg: &str) -> Result<(), String> {
    if arg.bytes().any(|byte| byte == 0 || byte.is_ascii_control()) {
        return Err("setup: invalid command argument".to_owned());
    }
    Ok(())
}

fn setup_absolute_path(path: &std::path::Path) -> std::path::PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .join(path)
    }
}

fn setup_home_dir(platform: &str, user: &str) -> String {
    if platform == "darwin" {
        format!("/Users/{user}")
    } else {
        format!("/home/{user}")
    }
}

fn setup_current_platform() -> &'static str {
    if cfg!(target_os = "windows") {
        "win32"
    } else if cfg!(target_os = "macos") {
        "darwin"
    } else {
        "linux"
    }
}

#[cfg(test)]
mod setup_tests {
    use super::*;

    struct SetupFakeRunner {
        cwd: std::path::PathBuf,
        user: String,
        platform: &'static str,
        ran: Vec<Vec<String>>,
    }

    impl SetupRunner for SetupFakeRunner {
        fn setup_cwd(&self) -> std::path::PathBuf {
            self.cwd.clone()
        }
        fn setup_user(&self) -> String {
            self.user.clone()
        }
        fn setup_platform(&self) -> &'static str {
            self.platform
        }
        fn setup_exists(&self, path: &std::path::Path) -> bool {
            path.exists()
        }

        fn setup_run(
            &mut self,
            command: &[String],
            _cwd: &std::path::Path,
        ) -> Result<String, String> {
            setup_validate_command(command)?;
            self.ran.push(command.to_vec());
            Ok(String::new())
        }
    }

    fn setup_temp_repo(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("maw-setup-test-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&root).expect("repo dir");
        std::fs::write(root.join("ecosystem.config.cjs"), "module.exports = {}\n")
            .expect("ecosystem");
        root
    }

    fn setup_runner(name: &str) -> SetupFakeRunner {
        SetupFakeRunner {
            cwd: setup_temp_repo(name),
            user: "agent".to_owned(),
            platform: "linux",
            ran: Vec::new(),
        }
    }

    fn setup_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn setup_help_and_dispatch_are_native() {
        let mut runner = setup_runner("help");
        let help = setup_run(&Vec::new(), &mut runner).expect("help");
        assert!(help.contains(SETUP_USAGE));
        assert_eq!(DISPATCH_67[0].command, "setup");
    }

    #[test]
    fn setup_auto_wake_dry_run_is_hermetic() {
        let repo = setup_temp_repo("dry-run");
        let mut runner = setup_runner("dry-run-cwd");
        let out = setup_run(
            &setup_strings(&[
                "auto-wake",
                "--dry-run",
                "--user",
                "nova",
                "--repo",
                repo.to_str().expect("utf8"),
                "--only",
                "maw-boot-test",
            ]),
            &mut runner,
        )
        .expect("dry run");
        assert!(out.contains("· dry-run loginctl enable-linger nova"));
        assert!(out.contains("pm2 start ecosystem.config.cjs --only maw-boot-test"));
        assert!(runner.ran.is_empty());
    }

    #[test]
    fn setup_auto_wake_runs_injected_commands() {
        let mut runner = setup_runner("run");
        let out = setup_run(&setup_strings(&["auto-wake"]), &mut runner).expect("run");
        assert!(out.contains("✓ next reboot"));
        assert_eq!(runner.ran.len(), 4);
        assert_eq!(
            runner.ran[0],
            setup_strings(&["loginctl", "enable-linger", "agent"])
        );
        assert_eq!(runner.ran[3], setup_strings(&["pm2", "save"]));
    }

    #[test]
    fn setup_rejects_missing_repo_and_guarded_args() {
        let mut runner = setup_runner("guards");
        assert!(setup_run(
            &setup_strings(&["auto-wake", "--user", "-bad"]),
            &mut runner
        )
        .is_err());
        assert!(setup_run(
            &setup_strings(&["auto-wake", "--only", "../bad"]),
            &mut runner
        )
        .is_err());
        assert!(setup_run(
            &setup_strings(&["auto-wake", "--repo", "-bad"]),
            &mut runner
        )
        .is_err());
        let missing =
            std::env::temp_dir().join(format!("maw-setup-missing-{}", std::process::id()));
        let error = setup_run(
            &setup_strings(&[
                "auto-wake",
                "--dry-run",
                "--repo",
                missing.to_str().unwrap(),
            ]),
            &mut runner,
        )
        .expect_err("missing ecosystem");
        assert!(error.contains("ecosystem.config.cjs not found"));
    }

    #[test]
    fn setup_platform_guards_match_js_contract() {
        let mut darwin = setup_runner("darwin");
        darwin.platform = "darwin";
        assert!(setup_run(&setup_strings(&["auto-wake"]), &mut darwin)
            .expect_err("darwin")
            .contains("macOS"));
        let mut win = setup_runner("win");
        win.platform = "win32";
        assert!(setup_run(&setup_strings(&["auto-wake"]), &mut win)
            .expect_err("win")
            .contains("Linux/macOS"));
        let dry =
            setup_run(&setup_strings(&["auto-wake", "--dry-run"]), &mut win).expect("win dry");
        assert!(dry.contains("dry-run"));
    }
}
