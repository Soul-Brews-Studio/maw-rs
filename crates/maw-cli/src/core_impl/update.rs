const DISPATCH_150: &[DispatcherEntry] = &[
    DispatcherEntry { command: "update", handler: Handler::Sync(update_run_command) },
    DispatcherEntry { command: "upgrade", handler: Handler::Sync(upgrade_run_command) },
];

const UPDATE_USAGE: &str = "usage: maw update [ref]\n\n  Update maw-js to a specific ref, channel, or branch.\n\n  Examples:\n    maw update          update to main (default)\n    maw update alpha    update to latest alpha tag\n    maw update beta     update to latest beta tag\n    maw update main     update to main branch\n\n  Flags:\n    --yes, -y     skip confirmation prompt (for scripts/fleet)\n    --help, -h    show this message and exit (no side effects)\n\n  ⚠ Manual `bun add -g` may loop — use `maw update <ref>` instead.";
const UPDATE_DEFAULT_REF: &str = "main";
const UPDATE_ALLOWED_FLAGS: &[&str] = &["--yes", "-y", "--help", "-h"];

#[derive(Debug, Clone, PartialEq, Eq)]
struct UpdateRequest150 { command: &'static str, ref_name: String, help: bool, yes: bool }

fn update_run_command(argv: &[String]) -> CliOutput { update_run_command_in("update", argv) }

fn upgrade_run_command(argv: &[String]) -> CliOutput { upgrade_run_command_in(argv) }

fn upgrade_run_command_in(argv: &[String]) -> CliOutput { update_run_command_in("upgrade", argv) }

fn update_run_command_in(command: &'static str, argv: &[String]) -> CliOutput {
    match update_parse_request(command, argv) {
        Ok(request) if request.help => update_output(0, format!("{UPDATE_USAGE}\n"), String::new()),
        Ok(request) => update_clean_error(&request),
        Err(message) => update_output(1, String::new(), format!("\u{1b}[31merror\u{1b}[0m: {message}\n")),
    }
}

fn update_parse_request(command: &'static str, argv: &[String]) -> Result<UpdateRequest150, String> {
    update_validate_args(argv)?;
    let help = argv.iter().any(|arg| matches!(arg.as_str(), "--help" | "-h"));
    let yes = argv.iter().any(|arg| matches!(arg.as_str(), "--yes" | "-y"));
    let ref_name = update_first_positional_ref(argv).unwrap_or_else(|| UPDATE_DEFAULT_REF.to_owned());
    if !help { update_validate_ref(&ref_name)?; }
    Ok(UpdateRequest150 { command, ref_name, help, yes })
}

fn update_validate_args(argv: &[String]) -> Result<(), String> {
    for arg in argv {
        if arg == "--" { return Err("-- separator is not allowed for maw update".to_owned()); }
        if arg.chars().any(|ch| ch == '\0' || ch.is_control()) {
            return Err("arguments must not contain NUL or control characters".to_owned());
        }
        if arg.starts_with('-') && !UPDATE_ALLOWED_FLAGS.contains(&arg.as_str()) {
            return Err(format!("invalid ref \"{arg}\" — looks like a flag. Run `maw update --help` for usage."));
        }
    }
    Ok(())
}

fn update_first_positional_ref(argv: &[String]) -> Option<String> {
    argv.iter().find(|arg| !arg.starts_with('-')).cloned()
}

fn update_validate_ref(ref_name: &str) -> Result<(), String> {
    if ref_name.is_empty() || !ref_name.chars().all(update_ref_char_allowed) {
        return Err(format!(
            "invalid ref \"{ref_name}\" — only [a-zA-Z0-9._-/] characters permitted"
        ));
    }
    Ok(())
}

fn update_ref_char_allowed(ch: char) -> bool { ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | '/') }

fn update_clean_error(request: &UpdateRequest150) -> CliOutput {
    let confirmation_note = if request.yes {
        String::new()
    } else {
        "  note: maw-js would ask for confirmation here; maw-rs stops before any install step.\n"
            .to_owned()
    };
    let stderr = format!(
        "\u{1b}[31merror\u{1b}[0m: `maw {}` is native-only in maw-rs\n  maw-rs is a single Rust binary; the maw-js bun self-update path is disabled.\n  requested ref: {}\n{}  install a newer maw-rs binary from the release artifact or your package manager, then re-run `maw --version`.\n  no bun fallback or delegated `maw` process was invoked.\n",
        request.command, request.ref_name, confirmation_note
    );
    update_output(1, String::new(), stderr)
}

fn update_output(code: i32, stdout: String, stderr: String) -> CliOutput { CliOutput { code, stdout, stderr } }

#[cfg(test)]
mod update_upgrade_tests150 {
    use super::*;

    fn update_args(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn update_dispatch_registers_update_and_upgrade_native() {
        assert_eq!(dispatcher_status("update"), DispatchKind::Native);
        assert_eq!(dispatcher_status("upgrade"), DispatchKind::Native);
        assert_eq!(DISPATCH_150.len(), 2);
        assert_eq!(DISPATCH_150[0].command, "update");
        assert_eq!(DISPATCH_150[1].command, "upgrade");
    }

    #[test]
    fn update_parse_matches_maw_js_flags_and_default_ref() {
        let parsed = update_parse_request("update", &update_args(&["--yes"])).expect("parse");
        assert_eq!(parsed.ref_name, "main");
        assert!(parsed.yes);
        assert!(!parsed.help);

        let parsed = update_parse_request("upgrade", &update_args(&["alpha", "--yes"])).expect("parse");
        assert_eq!(parsed.command, "upgrade");
        assert_eq!(parsed.ref_name, "alpha");
    }

    #[test]
    fn update_help_short_circuits_like_maw_js() {
        let out = update_run_command(&update_args(&["--help"]));
        assert_eq!(out.code, 0);
        assert!(out.stdout.contains("usage: maw update [ref]"));
        assert!(out.stderr.is_empty());
    }

    #[test]
    fn update_rejects_flags_control_chars_and_ref_metacharacters() {
        let flag = update_run_command(&update_args(&["--yess"]));
        assert_eq!(flag.code, 1);
        assert!(flag.stderr.contains("looks like a flag"));

        let control = update_run_command(&["main\nnext".to_owned(), "--yes".to_owned()]);
        assert_eq!(control.code, 1);
        assert!(control.stderr.contains("control"));

        let injected = update_run_command(&update_args(&["$(whoami)", "--yes"]));
        assert_eq!(injected.code, 1);
        assert!(injected.stderr.contains("invalid ref"));
    }

    #[test]
    fn update_clean_error_is_nonzero_and_native_guidance() {
        let out = upgrade_run_command(&update_args(&["beta", "--yes"]));
        assert_eq!(out.code, 1);
        assert!(out.stdout.is_empty());
        assert!(out.stderr.contains("`maw upgrade` is native-only in maw-rs"));
        assert!(out.stderr.contains("requested ref: beta"));
        assert!(out.stderr.contains("no bun fallback"));
    }
}
