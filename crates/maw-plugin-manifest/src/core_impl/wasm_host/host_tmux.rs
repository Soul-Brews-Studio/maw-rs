impl MawWasmHost {
    fn tmux_list_sessions(&self, _input: &str) -> HostResult<Value> {
        if let Err(err) = self.caps.require("tmux", "read", None) {
            return err;
        }
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        HostResult::ok(json!({"sessions": tmux_sessions_json(client.list_all())}))
    }

    fn tmux_capture(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxCaptureArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "capture", None) && !self.caps.contains("tmux", "read", None)
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux capture capability denied",
            );
        }
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        match client.capture(&args.target, args.lines) {
            Ok(mut content) => {
                if args.strip_ansi.unwrap_or(false) {
                    content = maw_tmux::strip_tmux_ansi(&content);
                }
                HostResult::ok(
                    json!({"target": args.target, "content": content, "lines": args.lines.unwrap_or(80)}),
                )
            }
            Err(error) => HostResult::err(HostErrorCode::IoError, error.message),
        }
    }

    fn tmux_send_keys(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<TmuxSendArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let text = args.keys.join(" ");
        let destructive = maw_tmux::check_destructive(&text);
        let needs_force = destructive.destructive
            || args.force.unwrap_or(false)
            || args.allow_destructive.unwrap_or(false);
        let has_force_cap =
            self.has_exact_cap("tmux:send:force") || self.has_exact_cap("tmux:send:*");
        let cap = if needs_force {
            if !has_force_cap {
                return HostResult::err(
                    HostErrorCode::CapabilityDenied,
                    "tmux send force capability denied",
                );
            }
            "tmux:send:force"
        } else if self.caps.contains("tmux", "send", None) {
            "tmux:send"
        } else {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux send capability denied",
            );
        };
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        let pane_command = match self.tmux_current_command(&args.target, &mut client) {
            Ok(command) => command,
            Err(err) => return err,
        };
        if maw_tmux::is_claude_like_pane(Some(&pane_command))
            && !has_force_cap
            && !args.allow_ai_pane.unwrap_or(false)
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux send into AI-agent pane denied",
            );
        }
        if !self.tmux_dry_run {
            let send = if args.literal.unwrap_or(false) {
                client.send_keys_literal(&args.target, &text)
            } else {
                client.send_keys(&args.target, &args.keys)
            };
            if let Err(error) = send {
                return HostResult::err(HostErrorCode::IoError, error.message);
            }
            if args.enter.unwrap_or(false) {
                if let Err(error) = client.send_enter(&args.target) {
                    return HostResult::err(HostErrorCode::IoError, error.message);
                }
            }
        }
        let result = HostResult::ok(
            json!({"target": args.target, "sent": true, "destructive": destructive.destructive}),
        );
        self.audit(
            "maw.tmux.send_keys",
            cap,
            &args.target,
            status_of(&result),
            start,
        );
        result
    }

    fn tmux_run(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxRunArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        self.tmux_send_keys(
            &serde_json::to_string(&TmuxSendArgs {
                target: args.target.clone(),
                keys: vec![args.text],
                literal: Some(true),
                enter: Some(true),
                allow_destructive: Some(false),
                force: Some(false),
                allow_ai_pane: Some(false),
            })
            .unwrap_or_default(),
        )
    }

    fn tmux_send_enter(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxEnterArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "send", None) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux send capability denied",
            );
        }
        let count = args.count.unwrap_or(1).min(5);
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        for _ in 0..count {
            if let Err(error) = client.send_enter(&args.target) {
                return HostResult::err(HostErrorCode::IoError, error.message);
            }
        }
        HostResult::ok(json!({"target": args.target, "count": count}))
    }

    fn tmux_tags_read(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<FsPathArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "read", None) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux read capability denied",
            );
        }
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        match client.read_pane_tags(&args.path) {
            Ok(tags) => HostResult::ok(json!({"title": tags.title, "meta": tags.meta})),
            Err(error) => HostResult::err(HostErrorCode::IoError, error.message),
        }
    }

    fn tmux_tags_write(&self, input: &str) -> HostResult<Value> {
        let args = match parse_args::<TmuxTagsWriteArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        if !self.caps.contains("tmux", "write-tags", None) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tmux write-tags capability denied",
            );
        }
        let meta = args.meta.unwrap_or_default();
        if meta
            .keys()
            .any(|key| !key.starts_with("@maw-") && !key.starts_with("maw-"))
        {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                "tag keys must use @maw-* namespace",
            );
        }
        let pairs = meta.into_iter().collect::<Vec<_>>();
        let mut client = TmuxClient::new(CommandTmuxRunner::new());
        match client.tag_pane(&args.target, args.title.as_deref(), &pairs) {
            Ok(()) => HostResult::ok(json!({"target": args.target})),
            Err(error) => HostResult::err(HostErrorCode::IoError, error.message),
        }
    }

}
