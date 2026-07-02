const DISPATCH_200: &[DispatcherEntry] = &[DispatcherEntry {
    command: "oracle-skills",
    handler: Handler::Sync(oracle_skills_run_command),
}];

const ORACLE_SKILLS_BIN: &str = "arra-oracle-skills";
const ORACLE_SKILLS_HELP: &str = "oracle-skills v0.1.0\n  Pass through to arra-oracle-skills to manage Oracle skills across AI coding agents.\n\n  usage: maw oracle-skills [args...] — pass through to arra-oracle-skills for skill management\n\n  surfaces:\n    cli: maw oracle-skills\n";

#[derive(Debug, Clone, PartialEq, Eq)]
struct OracleSkillsRunResult200 {
    code: i32,
    stdout: String,
    stderr: String,
}

trait OracleSkillsRunner200 {
    fn oracle_skills_run(&mut self, args: &[String]) -> Result<OracleSkillsRunResult200, std::io::Error>;
}

struct OracleSkillsSystemRunner200;

impl OracleSkillsRunner200 for OracleSkillsSystemRunner200 {
    fn oracle_skills_run(&mut self, args: &[String]) -> Result<OracleSkillsRunResult200, std::io::Error> {
        let status = std::process::Command::new(ORACLE_SKILLS_BIN)
            .args(args)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()?;
        Ok(OracleSkillsRunResult200 {
            code: status.code().unwrap_or(1),
            stdout: String::new(),
            stderr: String::new(),
        })
    }
}

fn oracle_skills_run_command(argv: &[String]) -> CliOutput {
    oracle_skills_run_command_in(argv, &mut OracleSkillsSystemRunner200)
}

fn oracle_skills_run_command_in<R: OracleSkillsRunner200>(argv: &[String], runner: &mut R) -> CliOutput {
    match oracle_skills_dispatch(argv, runner) {
        Ok(output) => output,
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn oracle_skills_dispatch<R: OracleSkillsRunner200>(argv: &[String], runner: &mut R) -> Result<CliOutput, String> {
    oracle_skills_validate_args(argv)?;
    if oracle_skills_has_help(argv) {
        return Ok(CliOutput { code: 0, stdout: format!("{ORACLE_SKILLS_HELP}\n"), stderr: String::new() });
    }
    let result = runner
        .oracle_skills_run(argv)
        .map_err(|_| "arra-oracle-skills not found on $PATH. Install with: bun add -g arra-oracle-skills".to_owned())?;
    if result.code == 0 {
        Ok(CliOutput { code: 0, stdout: result.stdout, stderr: result.stderr })
    } else {
        Ok(CliOutput {
            code: 1,
            stdout: result.stdout,
            stderr: oracle_skills_append_error(result.stderr, &format!("arra-oracle-skills exited with code {}", result.code)),
        })
    }
}

fn oracle_skills_validate_args(argv: &[String]) -> Result<(), String> {
    for arg in argv {
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("oracle-skills arguments must not contain NUL or control characters".to_owned());
        }
    }
    Ok(())
}

fn oracle_skills_has_help(argv: &[String]) -> bool {
    argv.iter().any(|arg| matches!(arg.as_str(), "-h" | "--help" | "-help"))
}

fn oracle_skills_append_error(mut stderr: String, message: &str) -> String {
    if !stderr.is_empty() && !stderr.ends_with('\n') { stderr.push('\n'); }
    stderr.push_str(message);
    stderr.push('\n');
    stderr
}

#[cfg(test)]
mod oracle_skills_tests200 {
    use super::*;

    #[derive(Default)]
    struct OracleSkillsFakeRunner200 {
        calls: Vec<Vec<String>>,
        result: Option<Result<OracleSkillsRunResult200, std::io::Error>>,
    }

    impl OracleSkillsRunner200 for OracleSkillsFakeRunner200 {
        fn oracle_skills_run(&mut self, args: &[String]) -> Result<OracleSkillsRunResult200, std::io::Error> {
            self.calls.push(args.to_vec());
            self.result.take().unwrap_or_else(|| Ok(OracleSkillsRunResult200 { code: 0, stdout: String::new(), stderr: String::new() }))
        }
    }

    fn oracle_skills_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn oracle_skills_dispatch_registers_native_part200() {
        assert_eq!(DISPATCH_200.len(), 1);
        assert_eq!(DISPATCH_200[0].command, "oracle-skills");
        assert_eq!(dispatcher_status("oracle-skills"), DispatchKind::Native);
    }

    #[test]
    fn oracle_skills_help_matches_plugin_metadata_without_spawn() {
        let mut runner = OracleSkillsFakeRunner200::default();
        let out = oracle_skills_run_command_in(&oracle_skills_args(&["--help"]), &mut runner);
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("oracle-skills v0.1.0"));
        assert!(out.stdout.contains("maw oracle-skills [args...]"));
        assert!(out.stderr.is_empty());
        assert!(runner.calls.is_empty());
    }

    #[test]
    fn oracle_skills_passes_cli_args_through_exactly() {
        let mut runner = OracleSkillsFakeRunner200::default();
        let out = oracle_skills_run_command_in(&oracle_skills_args(&["list", "--json"]), &mut runner);
        assert_eq!(out.code, 0);
        assert_eq!(runner.calls, vec![oracle_skills_args(&["list", "--json"])]);
    }

    #[test]
    fn oracle_skills_errors_match_maw_js_wrapper() {
        let mut missing = OracleSkillsFakeRunner200 {
            result: Some(Err(std::io::Error::new(std::io::ErrorKind::NotFound, "ENOENT"))),
            ..Default::default()
        };
        let out = oracle_skills_run_command_in(&oracle_skills_args(&["list"]), &mut missing);
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("arra-oracle-skills not found on $PATH"));
        assert!(out.stderr.contains("bun add -g arra-oracle-skills"));

        let mut nonzero = OracleSkillsFakeRunner200 {
            result: Some(Ok(OracleSkillsRunResult200 { code: 7, stdout: String::new(), stderr: String::new() })),
            ..Default::default()
        };
        let out = oracle_skills_run_command_in(&oracle_skills_args(&["install", "foo"]), &mut nonzero);
        assert_eq!(out.code, 1);
        assert_eq!(out.stderr, "arra-oracle-skills exited with code 7\n");
    }

    #[test]
    fn oracle_skills_rejects_control_chars_before_runner() {
        let mut runner = OracleSkillsFakeRunner200::default();
        let out = oracle_skills_run_command_in(&["list\nnow".to_owned()], &mut runner);
        assert_eq!(out.code, 1);
        assert!(out.stderr.contains("control"));
        assert!(runner.calls.is_empty());
    }
}
