fn wants_help(argv: &[String], value_flags: &[&str]) -> bool {
    wants_help_scan(argv, value_flags, false)
}

fn wants_help_before_positionals(argv: &[String], value_flags: &[&str]) -> bool {
    wants_help_scan(argv, value_flags, true)
}

fn wants_help_scan(argv: &[String], value_flags: &[&str], stop_at_first_positional: bool) -> bool {
    let mut index = 0_usize;
    while let Some(arg) = argv.get(index).map(String::as_str) {
        if arg == "--" {
            return false;
        }
        if matches!(arg, "--help" | "-h") {
            return true;
        }
        if help_arg_has_inline_value(arg, value_flags) {
            index += 1;
            continue;
        }
        if value_flags.contains(&arg) {
            index += 2;
            continue;
        }
        if stop_at_first_positional && !arg.starts_with('-') {
            return false;
        }
        index += 1;
    }
    false
}

fn help_arg_has_inline_value(arg: &str, value_flags: &[&str]) -> bool {
    value_flags
        .iter()
        .any(|flag| arg.strip_prefix(flag).is_some_and(|rest| rest.starts_with('=')))
}

fn help_output(usage: impl AsRef<str>) -> CliOutput {
    let mut stdout = usage.as_ref().to_owned();
    if !stdout.ends_with('\n') {
        stdout.push('\n');
    }
    CliOutput { code: 0, stdout, stderr: String::new() }
}
