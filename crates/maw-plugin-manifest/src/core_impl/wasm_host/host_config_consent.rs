impl MawWasmHost {
    fn config_set(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ConfigSetArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self
            .caps
            .require("sdk", "config", Some("write"))
            .or_else(|_| self.caps.require("sdk", "config:write", None))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        if args.key.trim().is_empty() {
            return HostResult::err(HostErrorCode::InvalidArgs, "config key is required");
        }
        if !is_plugin_writable_config_key_path(&args.key)
            || value_contains_secret_config_key_path(&args.key, &args.value)
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "config key is not in the host allowlist for WASM writes",
            );
        }
        let path = match self.config_file_path() {
            Ok(path) => path,
            Err(err) => return err,
        };
        let resource = format!("config:{}", args.key);
        self.audit("maw.config.set", &cap, &resource, "attempt", start);
        let mut config = match read_config_json(&path) {
            Ok(config) => config,
            Err(err) => return err,
        };
        if let Err(err) = set_json_path(&mut config, &args.key, args.value.clone()) {
            return err;
        }
        let final_value = get_json_path(&config, &args.key)
            .cloned()
            .unwrap_or(args.value);
        if let Err(err) = write_config_json(&path, &config) {
            return err;
        }
        HostResult::ok(
            json!({"key": args.key, "written": true, "audit": "config-write", "finalValue": final_value}),
        )
    }

    fn consent_read(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<ConsentReadArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let cap = match self
            .caps
            .require("sdk", "consent", Some("read"))
            .or_else(|_| self.caps.require("sdk", "consent:read", None))
        {
            Ok(cap) => cap,
            Err(err) => return err,
        };
        let view = args.view.as_deref().unwrap_or("pending");
        let state_root = match self.consent_state_root() {
            Ok(path) => path,
            Err(err) => return err,
        };
        let result = match view {
            "pending" | "list" => {
                let rows = match read_consent_pending(&state_root) {
                    Ok(rows) => rows,
                    Err(err) => return err,
                };
                HostResult::ok(json!({"text": format_consent_pending(&rows), "pending": rows}))
            }
            "trust" | "list-trust" => {
                let rows = match read_consent_trust(&state_root) {
                    Ok(rows) => rows,
                    Err(err) => return err,
                };
                HostResult::ok(json!({"text": format_consent_trust(&rows), "trust": rows}))
            }
            _ => HostResult::err(HostErrorCode::InvalidArgs, "view must be pending or trust"),
        };
        let resource = if matches!(view, "trust" | "list-trust") {
            "consent:trust"
        } else {
            "consent:pending"
        };
        self.audit(
            "maw.consent.read",
            &cap,
            resource,
            status_of(&result),
            start,
        );
        result
    }

}
