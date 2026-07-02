const DISPATCH_143: &[DispatcherEntry] = &[DispatcherEntry {
    command: "cross-team-queue",
    handler: Handler::Sync(ctq_run_command),
}];

const CTQ_USAGE: &str = "usage: maw cross-team-queue [--json|--help]";
const CTQ_EMPTY_JSON: &str = "{\"items\":[],\"stats\":{\"totalItems\":0,\"byRecipient\":{},\"byType\":{},\"oldestAgeHours\":null,\"newestAgeHours\":null},\"errors\":[],\"schemaVersion\":1}\n";

fn ctq_run_command(argv: &[String]) -> CliOutput {
    match ctq_dispatch(argv) {
        Ok(output) => output,
        Err((code, message)) => CliOutput {
            code,
            stdout: String::new(),
            stderr: format!("cross-team-queue: {message}\n"),
        },
    }
}

fn ctq_dispatch(argv: &[String]) -> Result<CliOutput, (i32, String)> {
    ctq_validate_argv(argv).map_err(|message| (2, message))?;
    if argv.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h" | "help")) {
        return Ok(ctq_ok(&format!("{CTQ_USAGE}\n")));
    }
    if let Some(arg) = argv.iter().find(|arg| !matches!(arg.as_str(), "--json")) {
        return Err((2, format!("unexpected argument {arg:?}. {CTQ_USAGE}")));
    }
    Ok(ctq_ok(CTQ_EMPTY_JSON))
}

fn ctq_validate_argv(argv: &[String]) -> Result<(), String> {
    for arg in argv {
        if arg == "--" {
            return Err("-- separator is not allowed".to_owned());
        }
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("arguments must not contain control characters".to_owned());
        }
        if arg.starts_with('-') && !matches!(arg.as_str(), "--json" | "--help" | "-h") {
            return Err(format!("unknown flag {arg}"));
        }
    }
    Ok(())
}

fn ctq_ok(stdout: &str) -> CliOutput {
    CliOutput { code: 0, stdout: stdout.to_owned(), stderr: String::new() }
}

#[cfg(test)]
mod ctq_tests {
    use super::*;

    fn ctq_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn ctq_dispatch_registers_native_and_empty_contract() {
        assert_eq!(dispatcher_status("cross-team-queue"), DispatchKind::Native);
        let out = ctq_run_command(&ctq_args(&[]));
        assert_eq!(out.code, 0, "{}", out.stderr);
        assert_eq!(out.stdout, CTQ_EMPTY_JSON);
    }

    #[test]
    fn ctq_guards_unknown_flags_and_separator() {
        let out = ctq_run_command(&ctq_args(&["--recipient", "nova"]));
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("unknown flag --recipient"));
        let out = ctq_run_command(&ctq_args(&["--"]));
        assert_eq!(out.code, 2);
        assert!(out.stderr.contains("separator"));
    }
}
