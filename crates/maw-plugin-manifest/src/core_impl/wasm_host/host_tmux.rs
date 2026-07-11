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

    fn tmux_command(&self, input: &str) -> HostResult<Value> {
        let start = Instant::now();
        let args = match parse_args::<TmuxCommandArgs>(input) {
            Ok(args) => args,
            Err(err) => return err,
        };
        let capability = if matches!(
            args.command.as_str(),
            "display-message" | "show-options" | "list-windows"
        ) {
            "tmux:read".to_owned()
        } else {
            format!("tmux:raw:{}", args.command)
        };
        if !self.has_exact_cap(&capability) {
            return HostResult::err(
                HostErrorCode::CapabilityDenied,
                format!("capability denied: {capability}"),
            );
        }
        if !valid_tmux_command_argv(&args.command, &args.args) {
            return HostResult::err(
                HostErrorCode::InvalidArgs,
                "tmux command or arguments are outside the managed ABI",
            );
        }
        let output = if self.tmux_dry_run {
            String::new()
        } else {
            let mut runner = CommandTmuxRunner::new();
            match maw_tmux::TmuxRunner::run(&mut runner, &args.command, &args.args) {
                Ok(output) => output,
                Err(error) => return HostResult::err(HostErrorCode::IoError, error.message),
            }
        };
        let resource = format!("tmux://{}", args.command);
        let result = HostResult::ok(json!({
            "command": args.command,
            "args": args.args,
            "stdout": output,
        }));
        self.audit(
            "maw.tmux.command",
            &capability,
            &resource,
            status_of(&result),
            start,
        );
        result
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

fn valid_tmux_command_argv(command: &str, args: &[String]) -> bool {
    let safe = |value: &str| {
        !value.is_empty()
            && value == value.trim()
            && !value.starts_with('-')
            && !value.chars().any(char::is_control)
    };
    let token = |value: &str| safe(value) && value.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.'));
    let layout = |value: &str| matches!(value, "even-horizontal" | "even-vertical" | "main-horizontal" | "main-vertical" | "tiled");
    match (command, args) {
        ("display-message", [print, format]) => {
            print == "-p" && matches!(format.as_str(), "#{session_name}" | "#{window_name}" | "#{pane_id}" | "#{window_id}")
        }
        ("display-message", [target_flag, target, print, format]) => {
            target_flag == "-t" && safe(target) && print == "-p" && matches!(format.as_str(), "#{pane_current_command}" | "#{session_name}:#{window_index}.#{pane_index}" | "#{session_name}:#{window_index}")
        }
        ("show-options", [target, session, global, option]) => {
            target == "-t" && safe(session) && global == "-gv" && option == "base-index"
        }
        ("list-windows", [target, session, format_flag, format]) => {
            target == "-t"
                && safe(session)
                && format_flag == "-F"
                && format == "#{window_index}\t#{window_name}\t#{window_active}\t#{window_panes}"
        }
        ("new-session", [detached, session_flag, session, name_flag, name]) => {
            detached == "-d"
                && session_flag == "-s"
                && safe(session)
                && name_flag == "-n"
                && name == "maw-stream-placeholder"
        }
        ("link-window", [detached, source_flag, source, target_flag, target]) => {
            detached == "-d"
                && source_flag == "-s"
                && safe(source)
                && target_flag == "-t"
                && safe(target)
        }
        ("rename-window", [target_flag, target, alias]) => {
            target_flag == "-t" && safe(target) && safe(alias) && !alias.contains(':')
        }
        ("set-window-option", [target_flag, target, key, source]) => {
            target_flag == "-t"
                && safe(target)
                && key == "@maw-linked-from"
                && safe(source)
        }
        ("kill-window", [target_flag, target]) => {
            target_flag == "-t"
                && safe(target)
                && target.ends_with(":maw-stream-placeholder")
        }
        ("kill-session" | "unlink-window" | "kill-pane", [target_flag, target]) => {
            target_flag == "-t" && safe(target)
        }
        ("list-panes", [target_flag, target, format_flag, format]) => {
            target_flag == "-t" && safe(target) && format_flag == "-F" && matches!(format.as_str(), "#{pane_id}|||#{pane_title}|||#{@maw_tile}" | "#{pane_index}|||#{pane_id}|||#{pane_title}|||#{pane_top}" | "#{pane_id}" | "#{pane_height}")
        }
        ("split-window", [target_flag, target, horizontal, print, format_flag, format, shell]) => {
            target_flag == "-t" && safe(target) && horizontal == "-h" && print == "-P" && format_flag == "-F" && format == "#{pane_id}" && !shell.is_empty() && !shell.chars().any(char::is_control)
        }
        ("select-pane", [target_flag, target, title_flag, title]) => {
            target_flag == "-t" && safe(target) && title_flag == "-T" && token(title)
        }
        ("set-option", [pane, target_flag, target, key, value]) if pane == "-p" => {
            target_flag == "-t" && safe(target) && match key.as_str() {
                "pane-border-format" => value.starts_with("#[fg=") && value.ends_with(",bold] #{pane_title}"),
                "pane-active-border-style" => value.starts_with("fg=") && token(&value[3..]),
                "@maw_tile" => value == "1",
                "@maw_tile_parent" | "@maw_tile_role" => safe(value),
                _ => false,
            }
        }
        ("set-option", [window, target_flag, target, key, value]) if window == "-w" => {
            target_flag == "-t" && safe(target) && key == "pane-border-status" && value == "top"
        }
        ("send-keys", [target_flag, target, key]) => {
            target_flag == "-t" && safe(target) && matches!(key.as_str(), "C-u" | "Enter")
        }
        ("send-keys", [target_flag, target, literal, value]) => {
            target_flag == "-t" && safe(target) && literal == "-l" && token(value)
        }
        ("select-layout", [target_flag, target, preset]) => {
            target_flag == "-t" && safe(target) && layout(preset)
        }
        ("swap-pane", [source_flag, source, target_flag, target]) => {
            source_flag == "-s" && safe(source) && target_flag == "-t" && safe(target)
        }
        _ => false,
    }
}
