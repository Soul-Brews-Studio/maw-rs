const DISPATCH_96: &[DispatcherEntry] = &[DispatcherEntry {
    command: "auth",
    handler: Handler::Sync(auth_run_command),
}];

const AUTH_VALUE_FLAGS: &[&str] = &[
    "--token",
    "--method",
    "--path",
    "--now",
    "--body-hash",
    "--signed-at",
    "--signature",
    "--cached-pubkey",
    "--from",
    "--timestamp",
    "--signature-v3",
    "--peer-key",
    "--address",
    "--oracle",
    "--node",
    "--body",
    "--secret",
    "--payload",
    "--header",
    "--peer-ip",
    "--workspace-key-env",
];

const AUTH_CONSENT_MUTATING_SUBCOMMANDS: &[&str] = &[
    "approve",
    "auto-approve",
    "pair-approve",
    "pair-auto",
    "trust",
];

fn auth_run_command(argv: &[String]) -> CliOutput {
    match auth_validate_argv(argv) {
        Ok(()) => run_auth_plan(argv),
        Err(message) => auth_guard_error(&message),
    }
}

fn auth_validate_argv(argv: &[String]) -> Result<(), String> {
    auth_validate_subcommand(argv)?;
    auth_validate_separator(argv)?;
    auth_validate_leading_dash_values(argv)?;
    Ok(())
}

fn auth_validate_subcommand(argv: &[String]) -> Result<(), String> {
    let Some(kind) = argv.first() else { return Ok(()); };
    if kind == "--" || kind.starts_with('-') {
        return Err("auth subcommand must not start with '-'".to_owned());
    }
    if AUTH_CONSENT_MUTATING_SUBCOMMANDS.iter().any(|blocked| blocked == kind) {
        return Err("auth: consent mutation requires explicit human flow; no auto-approve surface is exposed".to_owned());
    }
    Ok(())
}

fn auth_validate_separator(argv: &[String]) -> Result<(), String> {
    if argv.iter().any(|arg| arg == "--") {
        return Err("auth: -- separator is not allowed".to_owned());
    }
    Ok(())
}

fn auth_validate_leading_dash_values(argv: &[String]) -> Result<(), String> {
    let mut index = 1_usize;
    while index < argv.len() {
        let arg = &argv[index];
        if auth_is_value_flag(arg) {
            auth_validate_flag_value(argv, index, arg)?;
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(())
}

fn auth_is_value_flag(arg: &str) -> bool {
    AUTH_VALUE_FLAGS.iter().any(|flag| flag == &arg)
}

fn auth_validate_flag_value(argv: &[String], index: usize, flag: &str) -> Result<(), String> {
    let Some(value) = argv.get(index + 1) else { return Ok(()); };
    if value == "--" || value.starts_with('-') {
        return Err(format!("auth: {flag} value must not start with '-'"));
    }
    auth_validate_control_free_value(flag, value)
}

fn auth_validate_control_free_value(flag: &str, value: &str) -> Result<(), String> {
    if value.chars().any(char::is_control) {
        return Err(format!("auth: {flag} value must not contain control characters"));
    }
    Ok(())
}

fn auth_guard_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n"),
    }
}

#[cfg(test)]
mod auth_native_tests {
    use super::*;

    fn auth_args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn auth_dispatch_registers_single_native_command() {
        assert_eq!(DISPATCH_96.len(), 1);
        assert_eq!(DISPATCH_96[0].command, "auth");
        assert_eq!(dispatcher_status("auth"), DispatchKind::Native);
    }

    #[test]
    fn auth_sign_uses_fake_token_without_echoing_secret() {
        let output = auth_run_command(&auth_args(&[
            "sign-v1",
            "--token",
            "fake-test-token",
            "--now",
            "123",
            "--plan-json",
        ]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"kind\":\"sign-v1\""));
        assert!(!output.stdout.contains("fake-test-token"));
        assert!(!output.stderr.contains("fake-test-token"));
    }

    #[test]
    fn auth_guards_separator_and_leading_dash_values_without_secret_echo() {
        let sep = auth_run_command(&auth_args(&["sign-v1", "--"]));
        assert_eq!(sep.code, 2);
        assert!(sep.stderr.contains("separator"));
        let guarded = auth_run_command(&auth_args(&["sign-v1", "--token", "-secret-token"]));
        assert_eq!(guarded.code, 2);
        assert!(guarded.stderr.contains("--token value must not start"));
        assert!(!guarded.stderr.contains("secret-token"));
    }

    #[test]
    fn auth_blocks_auto_approve_consent_surface() {
        let output = auth_run_command(&auth_args(&["auto-approve", "--token", "fake-test-token"]));
        assert_eq!(output.code, 2);
        assert!(output.stderr.contains("no auto-approve"));
        assert!(!output.stderr.contains("fake-test-token"));
    }


    const AUTH_D2_NOW: &str = "1700000000";
    const AUTH_D2_FROM: &str = "mawjs:m5";
    const AUTH_D2_SECRET: &str = "d2-test-workspace-secret";
    const AUTH_D2_ENV: &str = "MAW_TEST_D2_WORKSPACE_KEY";
    const AUTH_D2_ED25519_PUBKEY: &str =
        "79b5562e8fe654f94078b112e8a98ba7901f853ae695bed7e0e3910bad049664";
    const AUTH_D2_ED25519_SIG: &str = concat!(
        "d232e00767facc77aca0eaaf2ebc18dc3c608639430f93167679805c7e3ccf69",
        "f15a856c7d8f4eddf64730cc61d4ccc0c28ca91b9a9df1a5016c628d737b3a0f"
    );

    fn auth_d2_hmac_headers(method: &str, path: &str, body: &str) -> Vec<String> {
        let body_hash = hash_body(Some(body.as_bytes()));
        let payload = format!("{method}:{path}:{AUTH_D2_NOW}:{body_hash}");
        let signature = sign_hmac_sig(AUTH_D2_SECRET, &payload);
        vec![
            "--header".to_owned(),
            format!("x-maw-from={AUTH_D2_FROM}"),
            "--header".to_owned(),
            format!("x-maw-timestamp={AUTH_D2_NOW}"),
            "--header".to_owned(),
            "x-maw-auth-version=v2".to_owned(),
            "--header".to_owned(),
            format!("x-maw-signature={signature}"),
        ]
    }

    fn auth_run_d2(args: &[String]) -> CliOutput {
        auth_run_command(args)
    }

    fn auth_d2_ed25519_args(cached_pubkey: Option<&str>) -> Vec<String> {
        let mut args = vec![
            "verify-request".to_owned(),
            "--method".to_owned(),
            "POST".to_owned(),
            "--path".to_owned(),
            "/triggers/fire".to_owned(),
            "--now".to_owned(),
            AUTH_D2_NOW.to_owned(),
            "--body".to_owned(),
            "{\"event\":\"agent-idle\"}".to_owned(),
            "--peer-ip".to_owned(),
            "198.51.100.10".to_owned(),
            "--header".to_owned(),
            format!("x-maw-from={AUTH_D2_FROM}"),
            "--header".to_owned(),
            format!("x-maw-timestamp={AUTH_D2_NOW}"),
            "--header".to_owned(),
            format!("x-maw-ed25519-signature={AUTH_D2_ED25519_SIG}"),
            "--header".to_owned(),
            format!("x-maw-ed25519-pubkey={AUTH_D2_ED25519_PUBKEY}"),
            "--plan-json".to_owned(),
        ];
        if let Some(pubkey) = cached_pubkey {
            args.extend(["--cached-pubkey".to_owned(), pubkey.to_owned()]);
        }
        args
    }

    #[test]
    fn auth_verify_request_d2_loopback_accepts_no_credentials() {
        let output = auth_run_command(&auth_args(&[
            "verify-request",
            "--peer-ip",
            "127.0.0.1",
            "--now",
            AUTH_D2_NOW,
            "--plan-json",
        ]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"mode\":\"d2\""));
        assert!(output.stdout.contains("\"kind\":\"accept\""));
        assert!(output.stdout.contains("\"who\":\"loopback\""));
    }

    #[test]
    fn auth_verify_request_d2_rejects_xff_spoof_on_non_loopback_peer() {
        let output = auth_run_command(&auth_args(&[
            "verify-request",
            "--peer-ip",
            "198.51.100.10",
            "--now",
            AUTH_D2_NOW,
            "--header",
            "x-forwarded-for=127.0.0.1",
            "--plan-json",
        ]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"kind\":\"reject\""));
        assert!(output.stdout.contains("\"reason\":\"missing-credentials\""));
        assert!(!output.stdout.contains("loopback"));
    }

    #[test]
    fn auth_verify_request_d2_hmac_uses_workspace_key_env_and_redacts_secret() {
        let _guard = env_test_lock()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _restore = EnvVarRestore::capture(AUTH_D2_ENV);
        std::env::set_var(AUTH_D2_ENV, AUTH_D2_SECRET);
        let mut args = vec![
            "verify-request".to_owned(),
            "--method".to_owned(),
            "POST".to_owned(),
            "--path".to_owned(),
            "/api/send".to_owned(),
            "--now".to_owned(),
            AUTH_D2_NOW.to_owned(),
            "--body".to_owned(),
            "body".to_owned(),
            "--peer-ip".to_owned(),
            "198.51.100.10".to_owned(),
            "--workspace-key-env".to_owned(),
            AUTH_D2_ENV.to_owned(),
            "--plan-json".to_owned(),
        ];
        args.extend(auth_d2_hmac_headers("POST", "/api/send", "body"));
        let output = auth_run_d2(&args);
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"kind\":\"accept\""));
        assert!(output.stdout.contains("\"who\":\"hmac-v2:mawjs:m5\""));
        assert!(!output.stdout.contains(AUTH_D2_SECRET));
        assert!(!output.stderr.contains(AUTH_D2_SECRET));
    }

    #[test]
    fn auth_verify_request_d2_ed25519_accepts_in_memory_tofu_first_contact() {
        let output = auth_run_d2(&auth_d2_ed25519_args(None));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"kind\":\"accept\""));
        assert!(output.stdout.contains("\"who\":\"ed25519:mawjs:m5\""));
    }

    #[test]
    fn auth_verify_request_d2_ed25519_pin_mismatch_rejects_without_repin() {
        let other_key = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let output = auth_run_d2(&auth_d2_ed25519_args(Some(other_key)));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"kind\":\"reject\""));
        assert!(output.stdout.contains("\"reason\":\"ed25519-pin-mismatch\""));
        assert!(!output.stdout.contains(AUTH_D2_ED25519_PUBKEY));
    }

    #[test]
    fn auth_verify_request_d2_rejects_secret_literal_env_name_shape() {
        let output = auth_run_command(&auth_args(&[
            "verify-request",
            "--workspace-key-env",
            "ghp_not_an_env_name",
            "--peer-ip",
            "198.51.100.10",
        ]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert_eq!(output.stdout, "reject\n");
        assert!(!output.stdout.contains("ghp_not_an_env_name"));
    }

    #[test]
    fn auth_verify_request_header_remains_plan_only_and_guarded() {
        let output = auth_run_command(&auth_args(&[
            "verify-request",
            "--now",
            "123",
            "--header",
            "x-maw-from=mawjs:m5",
            "--plan-json",
        ]));
        assert_eq!(output.code, 0, "{}", output.stderr);
        assert!(output.stdout.contains("\"command\":\"auth\""));
        let bad = auth_run_command(&auth_args(&["verify-request", "--header", "-bad"]));
        assert_eq!(bad.code, 2);
        assert!(bad.stderr.contains("--header value must not start"));
    }
}
