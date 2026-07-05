impl MawWasmHost {
    fn roots_for(&self, verb: &str) -> BTreeMap<String, PathBuf> {
        self.caps
            .scopes_for("fs", verb)
            .into_iter()
            .filter_map(|scope| {
                self.fs_roots
                    .get(&scope)
                    .and_then(|root| canonicalize_checked_path(root).ok())
                    .map(|root| (scope, root))
            })
            .collect()
    }

    /// Safely create `dir` and any missing ancestors, bounded to the plugin's
    /// declared write roots. Authorization is inherent: `roots_for("write")`
    /// only contains scopes for which `fs:write:<scope>` is granted, so a
    /// directory can only be created under a root the plugin may write.
    fn secure_mkdirp(&self, dir: &Path) -> Result<PathBuf, HostResult<Value>> {
        ensure_dir_within_roots(dir, &self.roots_for("write"))
    }

    fn check_cwd(&self, cwd: &str) -> Result<(), HostResult<Value>> {
        let cwd = canonicalize_checked(cwd)?;
        let roots = self
            .roots_for("read")
            .into_values()
            .chain(self.roots_for("write").into_values())
            .collect::<Vec<_>>();
        if roots.iter().any(|root| cwd.starts_with(root)) {
            Ok(())
        } else {
            Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "cwd outside declared filesystem roots",
            ))
        }
    }

    fn secret_ref(&self, key: Option<&str>) -> Result<String, HostResult<Value>> {
        let Some(key) = key else {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "peerKeyRef is required",
            ));
        };
        self.secret_store.get(key).cloned().ok_or_else(|| {
            HostResult::err(
                HostErrorCode::CapabilityDenied,
                "secret ref not available to plugin",
            )
        })
    }

    fn config_file_path(&self) -> Result<PathBuf, HostResult<Value>> {
        let root = self
            .fs_roots
            .get("config")
            .cloned()
            .unwrap_or_else(default_config_root);
        if deny_special_path(&root) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special config root denied",
            ));
        }
        if let Err(error) = std::fs::create_dir_all(&root) {
            return Err(HostResult::err(
                HostErrorCode::IoError,
                format!("create config root failed: {error}"),
            ));
        }
        let root = canonicalize_checked_path(&root)?;
        Ok(root.join("maw.config.json"))
    }

    fn resolve_localserver_url(&self) -> Result<Url, HostResult<Value>> {
        if let Some(url) = &self.localserver_url {
            return parse_localserver_base_url(url);
        }
        if let Ok(url) = std::env::var("MAW_LOCALSERVER_URL") {
            return parse_localserver_base_url(&url);
        }
        if let Ok(url) = std::env::var("MAW_ENGINE_URL") {
            return parse_localserver_base_url(&url);
        }
        let config_path = self.config_file_path()?;
        let config = read_config_json(&config_path).unwrap_or(Value::Null);
        let port = config
            .get("port")
            .and_then(json_u16)
            .or_else(|| std::env::var("MAW_PORT").ok().and_then(|value| value.parse::<u16>().ok()))
            .unwrap_or(31_745);
        parse_localserver_base_url(&format!("http://127.0.0.1:{port}"))
    }

    fn consent_state_root(&self) -> Result<PathBuf, HostResult<Value>> {
        let root = self
            .fs_roots
            .get("state")
            .cloned()
            .unwrap_or_else(default_state_root);
        if deny_special_path(&root) {
            return Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "special consent state root denied",
            ));
        }
        if root.exists() {
            canonicalize_checked_path(&root)
        } else {
            Ok(root)
        }
    }

    fn protected_security_paths(&self) -> Result<Vec<ProtectedPath>, HostResult<Value>> {
        let state_root = self.consent_state_root()?;
        [
            protected_dir(state_root.join("consent-pending")),
            protected_dir(state_root.join("consent")),
            protected_dir(state_root.join("trust")),
            protected_dir(state_root.join("pairing")),
            protected_file(state_root.join("trust.json")),
            protected_file(state_root.join("peer-key")),
            protected_file(state_root.join("peers.json")),
            protected_file(state_root.join("pair-code-store.json")),
            protected_file(state_root.join("recent-hellos.json")),
            protected_file(state_root.join("audit.jsonl")),
            protected_file(state_root.join("audit.log")),
            protected_file(state_root.join("audit.ndjson")),
        ]
        .into_iter()
        .map(resolve_protected_path)
        .collect()
    }

    fn deny_protected_security_path(&self, path: &Path) -> Result<(), HostResult<Value>> {
        if path_is_protected_security_state(path, &self.protected_security_paths()?) {
            Err(HostResult::err(
                HostErrorCode::CapabilityDenied,
                "protected security-state path denied",
            ))
        } else {
            Ok(())
        }
    }
}
