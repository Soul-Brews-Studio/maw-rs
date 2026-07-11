impl MawWasmHost {
    #[must_use]
    pub fn handle_json(&self, name: &str, input: &str) -> String {
        if let Some(fake) = self
            .fake_responses
            .get(&(name.to_owned(), input.to_owned()))
        {
            if let Some(capability) = &fake.capability {
                if !self.caps.caps.contains(capability) {
                    return to_json(&HostResult::<Value>::err(
                        HostErrorCode::CapabilityDenied,
                        format!("capability denied: {capability}"),
                    ));
                }
                self.audit(
                    name,
                    capability,
                    fake.resource.as_deref().unwrap_or("seeded-host"),
                    fake.status.as_deref().unwrap_or("ok"),
                    Instant::now(),
                );
            }
            return fake.output.clone();
        }
        match name {
            "maw.cli.run" => to_json(&self.cli_run(input)),
            "maw.exec.run" => to_json(&self.exec_run(input)),
            "maw.exec.spawn" => to_json(&self.exec_spawn(input)),
            "maw.paths.get" => to_json(&self.paths_get(input)),
            "maw.config.get" => to_json(&self.config_get(input)),
            "maw.config.set" => to_json(&self.config_set(input)),
            "maw.consent.read" => to_json(&self.consent_read(input)),
            "maw.consent.approve" | "maw.consent.reject" | "maw.consent.trust"
            | "maw.consent.untrust" | "maw.state.set" => to_json(&HostResult::<Value>::err(
                HostErrorCode::CapabilityDenied,
                "WASM plugins cannot approve, grant trust, pair, or mutate consent state; use a human-at-terminal command",
            )),
            "maw.fs.read" => to_json(&self.fs_read(input)),
            "maw.fs.write" => to_json(&self.fs_write(input)),
            "maw.fs.mkdir" => to_json(&self.fs_mkdir(input)),
            "maw.fs.remove" => to_json(&self.fs_remove(input)),
            "maw.fs.list" => to_json(&self.fs_list(input)),
            "maw.fs.stat" => to_json(&self.fs_stat(input)),
            "maw.http.request" => to_json(&self.http_request(input)),
            "maw.net.fetch" => to_json(&self.net_fetch(input)),
            "maw.localserver.request" => to_json(&self.localserver_request(input)),
            "maw.http.peer_send" => to_json(&self.peer_send(input)),
            "maw.http.peer_wake" => to_json(&self.peer_wake(input)),
            "maw.tmux.list_sessions" => to_json(&self.tmux_list_sessions(input)),
            "maw.tmux.capture" => to_json(&self.tmux_capture(input)),
            "maw.tmux.send_keys" => to_json(&self.tmux_send_keys(input)),
            "maw.tmux.run" => to_json(&self.tmux_run(input)),
            "maw.tmux.send_enter" => to_json(&self.tmux_send_enter(input)),
            "maw.tmux.tags_read" => to_json(&self.tmux_tags_read(input)),
            "maw.tmux.tags_write" => to_json(&self.tmux_tags_write(input)),
            "maw.ssh.exec" => to_json(&self.ssh_exec(input)),
            "maw.ssh.tmux_capture" => to_json(&self.ssh_tmux_capture(input)),
            "maw.ssh.tmux_send_keys" => to_json(&self.ssh_tmux_send_keys(input)),
            _ => to_json(&HostResult::<Value>::err(HostErrorCode::Unsupported, format!("unsupported host function: {name}"))),
        }
    }

    fn has_exact_cap(&self, capability: &str) -> bool {
        self.caps.caps.contains(capability)
    }

    fn tmux_current_command(
        &self,
        target: &str,
        client: &mut TmuxClient<CommandTmuxRunner>,
    ) -> Result<String, HostResult<Value>> {
        if let Some(command) = self.tmux_pane_commands.get(target) {
            return Ok(command.clone());
        }
        client
            .display_pane_current_command(target)
            .map_err(|error| HostResult::err(HostErrorCode::IoError, error.message))
    }

    fn audit(&self, name: &str, capability: &str, resource: &str, status: &str, start: Instant) {
        if let Ok(mut events) = self.audit.lock() {
            events.push(AuditEvent {
                plugin: redact(&self.plugin_name),
                host_fn: name.to_owned(),
                capability: capability.to_owned(),
                resource: redact(resource),
                status: status.to_owned(),
                duration_ms: start.elapsed().as_millis(),
            });
        }
    }

}
