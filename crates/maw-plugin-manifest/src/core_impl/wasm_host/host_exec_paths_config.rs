impl MawWasmHost {
    fn cli_run(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<CliRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if args.command.is_empty()
            || args.command.starts_with('-')
            || args.command.chars().any(char::is_control)
        {
            return HostResult::err(HostErrorCode::InvalidArgs, "invalid CLI command");
        }
        let cap = match self.caps.require("cli", "run", Some(&args.command)) {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let Ok(exe) = std::env::current_exe() else {
            return HostResult::err(
                HostErrorCode::ProcessFailed,
                "failed to resolve current maw executable",
            );
        };
        let mut command = Command::new(exe);
        command.arg(&args.command).args(&args.args);
        if let Some(cwd) = &self.cwd {
            command.current_dir(cwd);
        }
        let result = match command.output() {
            Ok(output) => HostResult::ok(json!({
                "status": output.status.code().unwrap_or(-1),
                "stdout": String::from_utf8_lossy(&output.stdout),
                "stderr": String::from_utf8_lossy(&output.stderr),
            })),
            Err(_) => HostResult::err(HostErrorCode::ProcessFailed, "CLI command failed to run"),
        };
        self.audit("maw.cli.run", &cap, &args.command, status_of(&result), start);
        result
    }

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
            "repos" | "fleet-state" | "fleet-legacy" | "fleet-config" => self
                .fs_roots
                .get(name)
                .map(|root| root.to_string_lossy().into_owned()),
            "teams" => self.fs_roots.get("teams").map(|root| root.to_string_lossy().into_owned()).or_else(|| {
                self.home.as_ref().map(|home| {
                    Path::new(home).join(".claude").join("teams").to_string_lossy().into_owned()
                })
            }),
            "claude-projects" => {
                if !self.has_exact_cap("fs:read:claude-projects") {
                    return HostResult::err(
                        HostErrorCode::CapabilityDenied,
                        "capability denied: fs:read:claude-projects",
                    );
                }
                self.fs_roots.get("claude-projects").map(|root| root.to_string_lossy().into_owned())
            }
            "psi" => {
                if !self.has_exact_cap("fs:read:psi") {
                    return HostResult::err(
                        HostErrorCode::CapabilityDenied,
                        "capability denied: fs:read:psi",
                    );
                }
                self.fs_roots
                    .get("psi")
                    .map(|root| root.to_string_lossy().into_owned())
            }
            "maw-cache" => return self.paths_get_maw_cache(name, start),
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
                    format!("unknown path name '{name}'; allowed: home, cwd, repos, fleet-state, fleet-legacy, fleet-config, teams, claude-projects, maw-cache, psi, vault"),
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

    fn paths_get_maw_cache(&self, name: &str, start: Instant) -> HostResult<Value> {
        if !self.has_exact_cap("fs:read:maw-cache") {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "capability denied: fs:read:maw-cache",
            );
        }
        let Some(root) = self.fs_roots.get("maw-cache") else {
            return HostResult::err(
                HostErrorCode::NotFound,
                "path 'maw-cache' is not available in this context",
            );
        };
        let mut value = json!({"name": name, "path": root});
        if std::env::var_os("MAW_HOME").is_none()
            && std::env::var_os("MAW_CACHE_DIR").is_none()
        {
            if let Some(legacy) = self.fs_roots.get("maw-legacy") {
                if legacy != root {
                    value["legacyPath"] = json!(legacy.join("artifacts"));
                }
            }
        }
        self.audit("maw.paths.get", "paths:get", name, "ok", start);
        HostResult::ok(value)
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
