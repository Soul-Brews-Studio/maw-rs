const DISPATCH_115: &[DispatcherEntry] = &[DispatcherEntry { command: "attach-ssh", handler: Handler::Sync(attachssh_run_command) }];

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttachsshTarget { node: String, session_name: String, ssh_alias: String }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AttachsshOptions { dry_run: bool, plan_json: bool }

#[derive(Debug, Clone, PartialEq, Eq)]
struct AttachsshStatus { success: bool, code: Option<i32> }

trait AttachsshCommandRunner {
    fn attachssh_status(&mut self, program: &str, args: &[String], interactive: bool) -> Result<AttachsshStatus, String>;
}

struct AttachsshSystemRunner;

impl AttachsshCommandRunner for AttachsshSystemRunner {
    fn attachssh_status(&mut self, program: &str, args: &[String], interactive: bool) -> Result<AttachsshStatus, String> {
        let mut command = std::process::Command::new(program);
        command.args(args);
        if interactive {
            command.stdin(std::process::Stdio::inherit()).stdout(std::process::Stdio::inherit()).stderr(std::process::Stdio::inherit());
        } else {
            command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        }
        let status = command.status().map_err(|error| error.to_string())?;
        Ok(AttachsshStatus { success: status.success(), code: status.code() })
    }
}

fn attachssh_run_command(argv: &[String]) -> CliOutput { attachssh_run_with_runner(argv, &mut AttachsshSystemRunner) }

fn attachssh_run_with_runner<R: AttachsshCommandRunner>(argv: &[String], runner: &mut R) -> CliOutput {
    let (target, opts) = match attachssh_parse_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return attachssh_usage_error(&message),
    };
    if let Err(message) = attachssh_validate_target(&target) { return command_target_error("attach-ssh", &message); }
    let probe_args = attachssh_probe_args(&target.ssh_alias);
    let attach_args = attachssh_exec_args(&target.ssh_alias, &target.session_name);
    if opts.plan_json { return attachssh_plan_json(&target, &probe_args, &attach_args); }
    if opts.dry_run { return attachssh_dry_run(&probe_args, &attach_args); }
    match attachssh_preflight(runner, &target.ssh_alias) {
        Ok(()) => {}
        Err(reason) => {
            let msg = format!("✗ ssh {} unreachable in 3s ({reason})\n  • check ~/.ssh/config for 'Host {}'\n  • check 'maw peers list' for the routing alias\n  • try the WG hostname directly: ssh {}.wg\n", target.ssh_alias, target.ssh_alias, target.ssh_alias);
            return CliOutput { code: 1, stdout: String::new(), stderr: msg };
        }
    }
    match runner.attachssh_status("ssh", &attach_args, true) {
        Ok(status) if status.success => CliOutput { code: 0, stdout: String::new(), stderr: String::new() },
        Ok(status) => command_target_error("attach-ssh", &format!("ssh attach to {} ({}) failed with exit {}", target.node, target.ssh_alias, status.code.unwrap_or(1))),
        Err(error) => command_target_error("attach-ssh", &format!("failed to execute ssh: {error}")),
    }
}

fn attachssh_parse_args(argv: &[String]) -> Result<(AttachsshTarget, AttachsshOptions), String> {
    let mut node = None;
    let mut session_name = None;
    let mut ssh_alias = None;
    let mut opts = AttachsshOptions { dry_run: false, plan_json: false };
    let mut positional = Vec::new();
    let mut index = 0usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(attachssh_usage_text()),
            "--dry-run" => opts.dry_run = true,
            "--plan-json" => opts.plan_json = true,
            "--node" => { let Some(value) = argv.get(index + 1) else { return Err("attach-ssh: missing --node value".to_owned()); }; node = Some(value.clone()); index += 1; }
            "--session" | "--session-name" => { let Some(value) = argv.get(index + 1) else { return Err("attach-ssh: missing --session value".to_owned()); }; session_name = Some(value.clone()); index += 1; }
            "--ssh-alias" => { let Some(value) = argv.get(index + 1) else { return Err("attach-ssh: missing --ssh-alias value".to_owned()); }; ssh_alias = Some(value.clone()); index += 1; }
            arg if arg.starts_with("--node=") => node = Some(arg["--node=".len()..].to_owned()),
            arg if arg.starts_with("--session=") => session_name = Some(arg["--session=".len()..].to_owned()),
            arg if arg.starts_with("--session-name=") => session_name = Some(arg["--session-name=".len()..].to_owned()),
            arg if arg.starts_with("--ssh-alias=") => ssh_alias = Some(arg["--ssh-alias=".len()..].to_owned()),
            arg if arg.starts_with('-') => return Err(format!("attach-ssh: unknown argument {arg}")),
            value => positional.push(value.to_owned()),
        }
        index += 1;
    }
    if node.is_none() && positional.len() >= 2 { node = Some(positional[0].clone()); session_name = Some(positional[1].clone()); }
    if ssh_alias.is_none() { ssh_alias.clone_from(&node); }
    Ok((AttachsshTarget { node: node.ok_or_else(|| "attach-ssh: --node required".to_owned())?, session_name: session_name.ok_or_else(|| "attach-ssh: --session required".to_owned())?, ssh_alias: ssh_alias.ok_or_else(|| "attach-ssh: --ssh-alias required".to_owned())? }, opts))
}

fn attachssh_usage_error(message: &str) -> CliOutput {
    let usage = attachssh_usage_text();
    let stderr = if message == usage { format!("{usage}\n") } else { format!("{message}\n{usage}\n") };
    CliOutput { code: 2, stdout: String::new(), stderr }
}

fn attachssh_usage_text() -> String { "usage: maw-rs attach-ssh --node <node> --session <session> --ssh-alias <alias> [--dry-run|--plan-json]".to_owned() }

fn attachssh_validate_target(target: &AttachsshTarget) -> Result<(), String> {
    attachssh_validate_node(&target.node)?;
    attachssh_validate_alias(&target.node, &target.ssh_alias)?;
    if !attachssh_safe_token(&target.session_name) { return Err(format!("cannot attach to {}: unsafe tmux session '{}'", target.node, target.session_name)); }
    Ok(())
}

fn attachssh_validate_node(node: &str) -> Result<(), String> {
    if !attachssh_safe_token(node) { return Err(format!("cannot attach: unsafe node '{node}'")); }
    Ok(())
}

fn attachssh_validate_alias(node: &str, alias: &str) -> Result<(), String> {
    if alias.trim().is_empty() { return Err(format!("cannot attach to {node}: missing SSH target")); }
    if !attachssh_safe_token(alias) { return Err(format!("cannot attach to {node}: unsafe ssh alias '{alias}'")); }
    Ok(())
}

fn attachssh_safe_token(value: &str) -> bool {
    let trimmed = value.trim();
    !trimmed.is_empty() && trimmed == value && !trimmed.starts_with('-') && trimmed.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':' | '-'))
}

fn attachssh_probe_args(alias: &str) -> Vec<String> { vec!["-o".to_owned(), "ConnectTimeout=3".to_owned(), "-o".to_owned(), "BatchMode=yes".to_owned(), alias.to_owned(), "true".to_owned()] }

fn attachssh_exec_args(alias: &str, session_name: &str) -> Vec<String> { vec!["-tt".to_owned(), alias.to_owned(), format!("tmux attach-session -t {session_name}")] }

fn attachssh_preflight<R: AttachsshCommandRunner>(runner: &mut R, alias: &str) -> Result<(), String> {
    let status = runner.attachssh_status("ssh", &attachssh_probe_args(alias), false)?;
    if status.success { Ok(()) } else { Err(format!("ssh exited {}", status.code.map_or_else(|| "?".to_owned(), |code| code.to_string()))) }
}

fn attachssh_plan_json(target: &AttachsshTarget, probe_args: &[String], attach_args: &[String]) -> CliOutput {
    CliOutput { code: 0, stdout: format!("{{\"command\":\"attach-ssh\",\"node\":{},\"sessionName\":{},\"sshAlias\":{},\"probeArgs\":{},\"sshArgs\":{}}}\n", json_string(&target.node), json_string(&target.session_name), json_string(&target.ssh_alias), json_string_array(probe_args), json_string_array(attach_args)), stderr: String::new() }
}

fn attachssh_dry_run(probe_args: &[String], attach_args: &[String]) -> CliOutput {
    CliOutput { code: 0, stdout: format!("ssh {}\nssh {}\n", probe_args.join(" "), attach_args.join(" ")), stderr: String::new() }
}

#[cfg(test)]
mod attachssh_tests {
    use super::*;

    #[derive(Default)]
    struct AttachsshFakeRunner { calls: Vec<(String, Vec<String>, bool)> }

    impl AttachsshCommandRunner for AttachsshFakeRunner {
        fn attachssh_status(&mut self, program: &str, args: &[String], interactive: bool) -> Result<AttachsshStatus, String> {
            self.calls.push((program.to_owned(), args.to_vec(), interactive));
            Ok(AttachsshStatus { success: true, code: Some(0) })
        }
    }

    fn attachssh_strings(values: &[&str]) -> Vec<String> { values.iter().map(|value| (*value).to_owned()).collect() }

    #[test]
    fn attachssh_dispatch_fragment_owns_attach_ssh() { assert_eq!(DISPATCH_115[0].command, "attach-ssh"); }

    #[test]
    fn attachssh_live_path_uses_ssh_argv_vec_without_shell() {
        let mut runner = AttachsshFakeRunner::default();
        let out = attachssh_run_with_runner(&attachssh_strings(&["--node", "peer-one", "--session", "50-mawjs", "--ssh-alias", "peer-one"]), &mut runner);
        assert_eq!(out.code, 0);
        assert_eq!(runner.calls.len(), 2);
        assert_eq!(runner.calls[0], ("ssh".to_owned(), attachssh_probe_args("peer-one"), false));
        assert_eq!(runner.calls[1], ("ssh".to_owned(), attachssh_exec_args("peer-one", "50-mawjs"), true));
    }

    #[test]
    fn attachssh_rejects_option_injection_before_spawn() {
        let mut runner = AttachsshFakeRunner::default();
        let out = attachssh_run_with_runner(&attachssh_strings(&["--node", "peer-one", "--session", "ok", "--ssh-alias", "-oProxyCommand=touch+pwned"]), &mut runner);
        assert_ne!(out.code, 0);
        assert!(out.stderr.contains("unsafe ssh alias"));
        assert!(runner.calls.is_empty());
    }
}
