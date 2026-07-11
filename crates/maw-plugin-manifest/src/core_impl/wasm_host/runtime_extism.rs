#[derive(Debug, Clone, Default)]
pub struct ExtismWasmInvokeRuntime {
    host_overrides: BTreeMap<String, MawWasmHost>,
    grant_manifest_fs_roots: bool,
}

impl ExtismWasmInvokeRuntime {
    #[must_use]
    pub fn with_host(mut self, plugin_name: impl Into<String>, host: MawWasmHost) -> Self {
        self.host_overrides.insert(plugin_name.into(), host);
        self
    }

    /// Config surface for the invoking path (e.g. real `maw <plugin>` CLI
    /// dispatch): when enabled, each plugin whose host is built by default gets
    /// exactly the filesystem roots its manifest caps declare, resolved through
    /// the fixed host registry ([`MawWasmHost::with_manifest_fs_roots`]).
    /// Explicit `with_host` overrides are left untouched.
    #[must_use]
    pub const fn with_manifest_fs_roots(mut self) -> Self {
        self.grant_manifest_fs_roots = true;
        self
    }
}

impl PluginInvokeRuntime for ExtismWasmInvokeRuntime {
    fn invoke_ts(&mut self, _plugin: &LoadedPlugin, _ctx: &InvokeContext) -> InvokeResult {
        InvokeResult::error("TS plugin runtime is not available in Extism runtime")
    }

    fn invoke_wasm(
        &mut self,
        plugin: &LoadedPlugin,
        ctx: &InvokeContext,
        wasm_bytes: &[u8],
    ) -> InvokeResult {
        let mut host = self
            .host_overrides
            .remove(&plugin.manifest.name)
            .unwrap_or_else(|| {
                let host = MawWasmHost::new(plugin);
                if self.grant_manifest_fs_roots {
                    host.with_manifest_fs_roots()
                } else {
                    host
                }
            });
        host.apply_context(ctx);
        let manifest = ExtismManifest::new([Wasm::data(wasm_bytes.to_vec())])
            .with_allowed_hosts(host.caps.scopes_for("net", "https").into_iter());
        let mut builder = PluginBuilder::new(manifest).with_wasi(false);
        for name in HOST_FN_NAMES {
            let fn_name = (*name).to_owned();
            let data = UserData::new(host.clone());
            builder = builder.with_function(
                *name,
                [ValType::I64],
                [ValType::I64],
                data,
                move |plugin, inputs, outputs, user_data| {
                    extism_host_call_named(plugin, inputs, outputs, &user_data, &fn_name)
                },
            );
        }
        let mut runtime = match builder.build() {
            Ok(plugin) => plugin,
            Err(error) => {
                return InvokeResult::error(format!("wasm instantiation failed: {error}"))
            }
        };
        let input = invoke_context_json(ctx);
        match runtime.call::<&str, String>(&plugin.wasm_export, &input) {
            Ok(output) => {
                parse_invoke_result_stdout(output.as_bytes()).unwrap_or_else(InvokeResult::error)
            }
            Err(error) => InvokeResult::error(format!("wasm call failed: {error}")),
        }
    }
}

pub const HOST_FN_NAMES: &[&str] = &[
    "maw.cli.run",
    "maw.exec.run",
    "maw.exec.spawn",
    "maw.paths.get",
    "maw.time.now",
    "maw.config.get",
    "maw.config.set",
    "maw.consent.read",
    "maw.fs.read",
    "maw.fs.write",
    "maw.fs.mkdir",
    "maw.fs.remove",
    "maw.fs.list",
    "maw.fs.stat",
    "maw.http.request",
    "maw.net.fetch",
    "maw.localserver.request",
    "maw.http.peer_send",
    "maw.http.peer_wake",
    "maw.tmux.list_sessions",
    "maw.tmux.capture",
    "maw.tmux.send_keys",
    "maw.tmux.run",
    "maw.tmux.command",
    "maw.tmux.send_enter",
    "maw.tmux.tags_read",
    "maw.tmux.tags_write",
    "maw.ssh.exec",
    "maw.ssh.tmux_capture",
    "maw.ssh.tmux_send_keys",
];

fn extism_host_call_named(
    plugin: &mut CurrentPlugin,
    inputs: &[Val],
    outputs: &mut [Val],
    host: &UserData<MawWasmHost>,
    name: &str,
) -> Result<(), extism::Error> {
    let input: String = plugin.memory_get_val(&inputs[0])?;
    let host = host.get()?;
    let host = host
        .lock()
        .map_err(|_| extism::Error::msg("host lock failed"))?;
    let output = host.handle_json(name, &input);
    plugin.memory_set_val(&mut outputs[0], output)?;
    Ok(())
}

fn parse_args<T: for<'de> Deserialize<'de>>(input: &str) -> Result<T, HostResult<Value>> {
    serde_json::from_str(input).map_err(|error| {
        HostResult::err(
            HostErrorCode::InvalidArgs,
            format!("invalid JSON args: {error}"),
        )
    })
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| {
        r#"{"ok":false,"error":"serialize failed","code":"io_error"}"#.to_owned()
    })
}

fn status_of<T>(result: &HostResult<T>) -> &'static str {
    match result {
        HostResult::Ok { .. } => "ok",
        HostResult::Err { .. } => "error",
    }
}
