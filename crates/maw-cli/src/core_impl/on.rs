const DISPATCH_42: &[DispatcherEntry] = &[
    DispatcherEntry { command: "on", handler: Handler::Sync(run_on_command) },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct OnCommandOptions {
    oracle: String,
    event: String,
    once: bool,
    timeout: i64,
    action: String,
}

fn run_on_command(argv: &[String]) -> CliOutput {
    match on_run_command_impl(argv) {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(error) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{error}\n"),
        },
    }
}

fn on_run_command_impl(argv: &[String]) -> Result<String, String> {
    let Some(options) = on_parse_command(argv)? else {
        return Ok(on_usage_text());
    };

    let path = active_config_dir().join("maw.config.json");
    let mut config = on_read_config(&path)?;
    on_append_trigger(&mut config, &options)?;
    write_json_atomic(&path, &config)?;

    let badge = if options.once {
        " \x1b[33m[once]\x1b[0m"
    } else {
        ""
    };
    Ok(format!(
        "\x1b[32m✓\x1b[0m trigger added: on {} {}{} → {}",
        options.oracle, options.event, badge, options.action
    ))
}

fn on_parse_command(argv: &[String]) -> Result<Option<OnCommandOptions>, String> {
    let oracle = argv.first().cloned().unwrap_or_default();
    let event = argv.get(1).cloned().unwrap_or_default();
    let once_index = argv.iter().position(|arg| arg == "--once");
    let once = once_index.is_some();
    let action_index = once_index.map_or(2, |index| index + 1);
    let timeout_index = argv.iter().position(|arg| arg == "--timeout");
    let timeout = timeout_index
        .and_then(|index| argv.get(index + 1))
        .and_then(|value| on_parse_js_i64_prefix(value))
        .unwrap_or(30);
    let action_args = argv.get(action_index..).unwrap_or(&[]);
    let action = action_args
        .iter()
        .enumerate()
        .filter_map(|(index, arg)| on_keep_action_part(action_args, index).then_some(arg.as_str()))
        .collect::<Vec<_>>()
        .join(" ");

    if oracle.is_empty() || event.is_empty() || action.is_empty() {
        return Ok(None);
    }

    on_validate_oracle(&oracle)?;
    on_validate_event(&event)?;

    Ok(Some(OnCommandOptions {
        oracle,
        event,
        once,
        timeout,
        action,
    }))
}

fn on_keep_action_part(action_args: &[String], index: usize) -> bool {
    let arg = &action_args[index];
    if arg == "--once" || arg == "--timeout" {
        return false;
    }
    index == 0 || action_args.get(index - 1).is_none_or(|previous| previous != "--timeout")
}

fn on_validate_oracle(oracle: &str) -> Result<(), String> {
    if oracle.trim() != oracle || oracle.starts_with('-') {
        Err("on: oracle must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn on_validate_event(event: &str) -> Result<(), String> {
    if event.trim() != event || event.starts_with('-') {
        Err("on: event must be non-empty, unpadded, and not start with '-'".to_owned())
    } else {
        Ok(())
    }
}

fn on_read_config(path: &std::path::Path) -> Result<serde_json::Value, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map_err(|error| format!("on: parse {}: {error}", path.display())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(serde_json::json!({})),
        Err(error) => Err(format!("on: read {}: {error}", path.display())),
    }
}

fn on_append_trigger(config: &mut serde_json::Value, options: &OnCommandOptions) -> Result<(), String> {
    if !config.is_object() {
        *config = serde_json::json!({});
    }
    let trigger = on_render_trigger(options);
    let triggers = config
        .as_object_mut()
        .expect("object after normalization")
        .entry("triggers")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let Some(triggers) = triggers.as_array_mut() else {
        return Err("on: config.triggers must be an array".to_owned());
    };
    triggers.push(trigger);
    Ok(())
}

fn on_render_trigger(options: &OnCommandOptions) -> serde_json::Value {
    let mut trigger = serde_json::json!({
        "on": format!("agent-{}", options.event),
        "repo": options.oracle,
        "timeout": options.timeout,
        "action": options.action,
        "name": format!("on-{}-{}", options.oracle, options.event),
    });
    if options.once {
        trigger["once"] = serde_json::Value::Bool(true);
    }
    trigger
}

fn on_parse_js_i64_prefix(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    let (sign, digits) = trimmed
        .strip_prefix('-')
        .map_or((1_i64, trimmed), |tail| (-1_i64, tail));
    let digits = digits
        .chars()
        .take_while(char::is_ascii_digit)
        .collect::<String>();
    (!digits.is_empty())
        .then(|| digits.parse::<i64>().ok().and_then(|number| number.checked_mul(sign)))
        .flatten()
}

fn on_usage_text() -> String {
    concat!(
        "\x1b[36mUsage:\x1b[0m maw on <oracle> <event> [--once] [--timeout N] \"<action>\"\n",
        "\n\x1b[33mEvents:\x1b[0m agent-idle, agent-wake, agent-crash\n",
        "\n\x1b[33mExamples:\x1b[0m\n",
        "  maw on neo idle --once \"maw hey homekeeper 'neo done'\"\n",
        "  maw on neo crash \"maw wake neo\"",
    )
    .to_owned()
}

#[cfg(test)]
mod on_tests {
    use super::*;

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn on_parser_matches_maw_js_once_timeout_action_filtering() {
        let parsed = on_parse_command(&strings(&[
            "neo",
            "idle",
            "--once",
            "maw",
            "hey",
            "homekeeper",
            "done",
            "--timeout",
            "12ms",
        ]))
        .expect("parse")
        .expect("options");

        assert_eq!(parsed.oracle, "neo");
        assert_eq!(parsed.event, "idle");
        assert!(parsed.once);
        assert_eq!(parsed.timeout, 12);
        assert_eq!(parsed.action, "maw hey homekeeper done");
    }

    #[test]
    fn on_parser_default_timeout_and_usage() {
        let parsed = on_parse_command(&strings(&["neo", "crash", "maw wake neo"]))
            .expect("parse")
            .expect("options");
        assert_eq!(parsed.timeout, 30);
        assert!(!parsed.once);
        assert_eq!(parsed.action, "maw wake neo");
        assert!(on_parse_command(&[]).expect("usage").is_none());
    }

    #[test]
    fn on_oracle_guard_blocks_option_injection_target() {
        let error = on_parse_command(&strings(&["-t", "idle", "maw wake neo"]))
            .expect_err("guard");
        assert!(error.contains("not start with '-'"));
    }

    #[test]
    fn on_trigger_preserves_existing_config_and_appends_once_flag_only_when_true() {
        let mut config = serde_json::json!({"node":"local","triggers":[{"name":"old"}]});
        on_append_trigger(
            &mut config,
            &OnCommandOptions {
                oracle: "neo".to_owned(),
                event: "wake".to_owned(),
                once: true,
                timeout: 7,
                action: "maw hey neo up".to_owned(),
            },
        )
        .expect("append");

        assert_eq!(config["node"], "local");
        assert_eq!(config["triggers"].as_array().expect("array").len(), 2);
        assert_eq!(config["triggers"][1]["on"], "agent-wake");
        assert_eq!(config["triggers"][1]["once"], true);
    }
}
