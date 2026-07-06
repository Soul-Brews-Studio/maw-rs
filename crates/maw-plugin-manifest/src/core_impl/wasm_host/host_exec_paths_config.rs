impl MawWasmHost {
    fn exec_run(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ExecRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let base = executable_basename(&args.cmd);
        if is_hard_denied_exec(&args.cmd, &args.args) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "hard-denied executable or interactive/privileged option",
            );
        }
        let cap = match self
            .caps
            .require("proc", "exec", Some(&base))
            .or_else(|_| self.caps.require("shell", "exec", Some(&base)))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        if let Some(cwd) = &args.cwd {
            if let Err(err) = self.check_cwd(cwd) {
                return err;
            }
        }
        let mut env = match sanitize_env(args.env.as_ref()) {
            Ok(env) => env,
            Err(err) => return err,
        };
        // sanitize_env strips HOME by default. Re-inject the real user HOME only
        // when the manifest opts in via the `exec:home` capability, so exec'd
        // tools that need it (e.g. locating ~/.claude) can find it.
        if self.caps.contains("exec", "home", None) {
            if let Some(home) = self.exec_home() {
                env.insert("HOME".to_owned(), home);
            }
        }
        let mut cmd = Command::new(&args.cmd);
        cmd.args(&args.args)
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(cwd) = &args.cwd {
            cmd.current_dir(cwd);
        }
        let output = run_child(
            cmd,
            args.stdin.as_deref(),
            args.timeout_ms.unwrap_or(10_000).min(MAX_EXEC_TIMEOUT_MS),
        );
        let result = match output {
            Ok(output) => {
                let status = output.status.code().unwrap_or(-1);
                if status != 0 && !args.allow_non_zero {
                    HostResult::err(
                        HostErrorCode::ProcessFailed,
                        format!("process exited with status {status}"),
                    )
                } else {
                    HostResult::ok(
                        json!({"status": status, "stdout": String::from_utf8_lossy(&output.stdout), "stderr": String::from_utf8_lossy(&output.stderr), "durationMs": start.elapsed().as_millis()}),
                    )
                }
            }
            Err(code) => HostResult::err(code, "process execution failed"),
        };
        self.audit("maw.exec.run", &cap, &args.cmd, status_of(&result), start);
        result
    }

    fn exec_spawn(&self, input: &str) -> HostResult<Value> {
        let mut args = match parse_args::<ExecRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        args.allow_non_zero = true;
        self.exec_run(&serde_json::to_string(&args).unwrap_or_default())
    }

    fn paths_get(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<PathsGetArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let name = args.name.as_deref().unwrap_or_default();
        // Fixed allowlist — deliberately NOT a general env/path getter.
        let value = match name {
            "home" => self.home.clone(),
            "cwd" => self.cwd.clone(),
            "teams" => self.fs_roots.get("teams").map(|root| root.to_string_lossy().into_owned()).or_else(|| {
                self.home.as_ref().map(|home| {
                    Path::new(home).join(".claude").join("teams").to_string_lossy().into_owned()
                })
            }),
            "vault" => {
                if !self.has_exact_cap("fs:read:vault") {
                    return HostResult::err(
                        HostErrorCode::CapabilityDenied,
                        "capability denied: fs:read:vault",
                    );
                }
                if let Some(root) = self.fs_roots.get("vault") {
                    Some(root.to_string_lossy().into_owned())
                } else {
                    match self.home.as_ref() {
                        Some(home) => match configured_vault_root(
                            Path::new(home),
                            self.config_root.as_deref(),
                            self.vault_root.as_deref(),
                        ) {
                            Ok(path) => Some(path.to_string_lossy().into_owned()),
                            Err(err) => return err,
                        },
                        None => None,
                    }
                }
            }
            _ => {
                return HostResult::err(
                    HostErrorCode::InvalidArgs,
                    format!("unknown path name '{name}'; allowed: home, cwd, teams, vault"),
                );
            }
        };
        let result = value.map_or_else(
            || {
                HostResult::err(
                    HostErrorCode::NotFound,
                    format!("path '{name}' is not available in this context"),
                )
            },
            |path| HostResult::ok(json!({ "name": name, "path": path })),
        );
        self.audit(
            "maw.paths.get",
            "paths:get",
            name,
            status_of(&result),
            start,
        );
        result
    }

    fn config_get(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ConfigGetArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self
            .caps
            .require("sdk", "config", Some("read"))
            .or_else(|_| self.caps.require("sdk", "config:read", None))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let path = match self.config_file_path() {
            Ok(path) => path,
            Err(err) => return err,
        };
        let config = match read_config_json(&path) {
            Ok(config) => config,
            Err(err) => return err,
        };
        let resource = args
            .key
            .as_deref()
            .map_or_else(|| "config".to_owned(), |key| format!("config:{key}"));
        let value = args
            .key
            .as_deref()
            .and_then(|key| get_json_path(&config, key))
            .cloned()
            .unwrap_or(Value::Null);
        let result = HostResult::ok(json!({"key": args.key, "value": value, "config": config}));
        self.audit("maw.config.get", &cap, &resource, status_of(&result), start);
        result
    }
}
