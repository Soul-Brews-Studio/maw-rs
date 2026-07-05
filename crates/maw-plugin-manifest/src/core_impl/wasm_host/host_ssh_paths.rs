impl MawWasmHost {
    fn ssh_exec(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<SshExecArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !is_safe_ssh_host_token(&args.host) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "ssh host must be non-empty, unpadded, not start with '-', and contain only ASCII letters, digits, '_', '.', ':', or '-'",
            );
        }
        if !self.caps.contains("shell", "ssh", Some(&args.host))
            || !self.caps.contains("proc", "exec", Some("ssh"))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "ssh exec capability denied",
            );
        }
        if args
            .args
            .iter()
            .any(|arg| matches!(arg.as_str(), "-A" | "-L" | "-R" | "-D" | "-tt" | "-t"))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "interactive/forwarding ssh options denied",
            );
        }
        let mut cmd = Command::new("ssh");
        cmd.arg("-T")
            .arg("--")
            .arg(&args.host)
            .arg(&args.cmd)
            .args(&args.args)
            .env_clear()
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::piped());
        let output = run_child(
            cmd,
            args.stdin.as_deref(),
            args.timeout_ms.unwrap_or(10_000).min(MAX_EXEC_TIMEOUT_MS),
        );
        match output {
            Ok(output) => HostResult::ok(
                json!({"transport": "ssh", "host": args.host, "status": output.status.code().unwrap_or(-1), "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr)}),
            ),
            Err(code) => HostResult::err(code, "ssh execution failed"),
        }
    }

    fn ssh_tmux_capture(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<SshTmuxCaptureArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !is_safe_tmux_target_token(&args.target) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "tmux target must be non-empty, unpadded, not start with '-', and contain only ASCII letters, digits, '_', '.', ':', '%', or '-'",
            );
        }
        self.ssh_exec(
            &serde_json::to_string(&SshExecArgs {
                host: args.host,
                cmd: "tmux".to_owned(),
                args: vec![
                    "capture-pane".to_owned(),
                    "-p".to_owned(),
                    "-t".to_owned(),
                    args.target,
                    "-S".to_owned(),
                    format!("-{}", args.lines.unwrap_or(80)),
                ],
                stdin: None,
                timeout_ms: None,
            })
            .unwrap_or_default(),
        )
    }

    fn ssh_tmux_send_keys(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<SshTmuxSendArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !is_safe_tmux_target_token(&args.target) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "tmux target must be non-empty, unpadded, not start with '-', and contain only ASCII letters, digits, '_', '.', ':', '%', or '-'",
            );
        }
        self.ssh_exec(
            &serde_json::to_string(&SshExecArgs {
                host: args.host,
                cmd: "tmux".to_owned(),
                args: [
                    vec!["send-keys".to_owned(), "-t".to_owned(), args.target],
                    args.keys,
                ]
                .concat(),
                stdin: None,
                timeout_ms: None,
            })
            .unwrap_or_default(),
        )
    }

    fn secure_path(
        &self,
        requested: &str,
        verb: &str,
    ) -> Result<(String, PathBuf), HostResult<Value>> {
        let path = canonicalize_checked(requested)?;
        if deny_special_path(&path) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special filesystem path denied",
            ));
        }
        let roots = self.roots_for(verb);
        let (scope, _) = roots
            .iter()
            .find(|(_, root)| path.starts_with(root))
            .ok_or_else(|| {
                HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "filesystem path outside declared roots",
                )
            })?;
        let cap = self.caps.require("fs", verb, Some(scope))?;
        Ok((cap, path))
    }

    fn secure_write_path(&self, requested: &str) -> Result<(String, PathBuf), HostResult<Value>> {
        let raw = Path::new(requested);
        let path = resolve_write_path(raw)?;
        if deny_special_path(&path) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special filesystem path denied",
            ));
        }
        self.deny_protected_security_path(&path)?;
        let roots = self.roots_for("write");
        let (scope, _) = roots
            .iter()
            .find(|(_, root)| path.starts_with(root))
            .ok_or_else(|| {
                HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "filesystem path outside declared write roots",
                )
            })?;
        let cap = self.caps.require("fs", "write", Some(scope))?;
        Ok((cap, path))
    }

    fn secure_remove_path(&self, requested: &str) -> Result<(String, PathBuf), HostResult<Value>> {
        if contains_glob_pattern(requested) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "glob/wildcard filesystem paths are denied",
            ));
        }
        let raw = Path::new(requested);
        let meta = std::fs::symlink_metadata(raw).map_err(|error| {
            HostResult::err(
                if error.kind() == std::io::ErrorKind::NotFound {
                    HostErrorCode::NotFound
                } else {
                    HostErrorCode::IoError
                },
                format!("stat failed: {error}"),
            )
        })?;
        if meta.file_type().is_symlink() {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "symlink deletion is denied",
            ));
        }
        let path = canonicalize_checked_path(raw)?;
        if deny_special_path(&path) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special filesystem path denied",
            ));
        }
        self.deny_protected_security_path(&path)?;
        let roots = self.roots_for("write");
        let (scope, _) = roots
            .iter()
            .find(|(_, root)| path.starts_with(root))
            .ok_or_else(|| {
                HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "filesystem path outside declared write roots",
                )
            })?;
        let cap = self.caps.require("fs", "write", Some(scope))?;
        Ok((cap, path))
    }

}
