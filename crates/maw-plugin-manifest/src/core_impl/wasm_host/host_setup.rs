impl MawWasmHost {
    #[must_use]
    pub fn new(plugin: &LoadedPlugin) -> Self {
        Self {
            plugin_name: plugin.manifest.name.clone(),
            caps: CapabilitySet::from_manifest(&plugin.manifest),
            endpoints: plugin.manifest.endpoints.clone().unwrap_or_default(),
            secrets: plugin.manifest.secrets.clone().unwrap_or_default(),
            fs_roots: BTreeMap::new(),
            secret_store: BTreeMap::new(),
            fake_responses: BTreeMap::new(),
            tmux_pane_commands: BTreeMap::new(),
            tmux_dry_run: false,
            audit: Arc::new(Mutex::new(Vec::new())),
            http_timeout_ms: 10_000,
            localserver_url: None,
            http_resolver_overrides: BTreeMap::new(),
            cwd: None,
            home: None,
        }
    }

    /// Set the cwd/home paths exposed via `maw.paths.get` and injected into
    /// exec'd children that hold the `exec:home` capability.
    #[must_use]
    pub fn with_paths(mut self, cwd: Option<String>, home: Option<String>) -> Self {
        self.cwd = cwd;
        self.home = home;
        self
    }

    /// Fill cwd/home from the invoke context without clobbering values that
    /// were already set explicitly (e.g. via `with_paths` in tests).
    fn apply_context(&mut self, ctx: &InvokeContext) {
        if self.cwd.is_none() {
            self.cwd.clone_from(&ctx.cwd);
        }
        if self.home.is_none() {
            self.home.clone_from(&ctx.home);
        }
    }

    /// Real user home for exec'd children: the context-supplied value, falling
    /// back to the host process `$HOME`.
    fn exec_home(&self) -> Option<String> {
        self.home.clone().or_else(|| {
            std::env::var_os("HOME").map(|home| home.to_string_lossy().into_owned())
        })
    }

    #[must_use]
    pub fn with_fs_root(mut self, name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        self.fs_roots.insert(name.into(), path.into());
        self
    }

    /// Grant exactly the filesystem roots this plugin's manifest capabilities
    /// declare, resolved through the fixed host registry ([`known_fs_root`]).
    ///
    /// Security model: a manifest may only name a scope (`fs:read:teams` /
    /// `fs:write:teams`); it can NEVER supply a path. A root is granted only when
    /// BOTH the cap is declared AND the scope name resolves in the hardcoded
    /// registry, so read vs write is enforced by which `fs:<verb>:*` caps exist.
    /// Resolves the base home directory from the host process `HOME`.
    #[must_use]
    pub fn with_manifest_fs_roots(self) -> Self {
        match home_dir() {
            Some(home) => self.with_manifest_fs_roots_from(&home),
            None => self,
        }
    }

    /// [`Self::with_manifest_fs_roots`] with an explicit home base — used by tests
    /// so root resolution stays deterministic without mutating process env.
    #[must_use]
    pub fn with_manifest_fs_roots_from(mut self, home: &Path) -> Self {
        for verb in ["read", "write"] {
            for scope in self.caps.scopes_for("fs", verb) {
                if self.fs_roots.contains_key(&scope) {
                    continue;
                }
                if let Some(path) = known_fs_root(&scope, home) {
                    // Fixed-registry path only. Some legacy roots are created as
                    // anchors; read-only configured roots (e.g. vault) are not.
                    if known_fs_root_should_create(&scope) {
                        let _ = std::fs::create_dir_all(&path);
                    }
                    self.fs_roots.insert(scope, path);
                }
            }
        }
        self
    }

    #[must_use]
    pub fn with_secret_ref(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.secret_store.insert(name.into(), value.into());
        self
    }

    #[must_use]
    pub fn with_localserver_url(mut self, url: impl Into<String>) -> Self {
        self.localserver_url = Some(url.into());
        self
    }

    #[must_use]
    pub fn with_http_resolver_override(
        mut self,
        host: impl Into<String>,
        addrs: impl IntoIterator<Item = IpAddr>,
    ) -> Self {
        self.http_resolver_overrides
            .insert(host.into(), addrs.into_iter().collect());
        self
    }

    #[must_use]
    pub fn with_fake_response(
        self,
        name: impl Into<String>,
        input: impl Into<String>,
        output: impl Into<String>,
    ) -> Self {
        self.with_audited_fake_response(name, input, output, None, None, None)
    }

    #[must_use]
    pub fn with_audited_fake_response(
        mut self,
        name: impl Into<String>,
        input: impl Into<String>,
        output: impl Into<String>,
        capability: Option<String>,
        resource: Option<String>,
        status: Option<String>,
    ) -> Self {
        self.fake_responses.insert(
            (name.into(), input.into()),
            FakeHostResponse {
                output: output.into(),
                capability,
                resource,
                status,
            },
        );
        self
    }

    #[must_use]
    pub fn with_tmux_pane_command(
        mut self,
        target: impl Into<String>,
        command: impl Into<String>,
    ) -> Self {
        self.tmux_pane_commands
            .insert(target.into(), command.into());
        self
    }

    #[must_use]
    pub fn with_tmux_dry_run(mut self) -> Self {
        self.tmux_dry_run = true;
        self
    }

    #[must_use]
    pub fn audit_json_lines(&self) -> String {
        self.audit.lock().map_or_else(
            |_| String::new(),
            |events| {
                events
                    .iter()
                    .map(|event| serde_json::to_string(event).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        )
    }

}
