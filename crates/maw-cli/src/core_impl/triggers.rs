const DISPATCH_144: &[DispatcherEntry] = &[
    DispatcherEntry { command: "triggers", handler: Handler::Sync(run_triggers_command_136) },
    DispatcherEntry { command: "trigger", handler: Handler::Sync(run_triggers_command_136) },
];

#[derive(Debug, Clone, serde::Deserialize)]
struct TriggerConfig136 {
    on: String,
    repo: Option<String>,
    timeout: Option<i64>,
    action: String,
    once: Option<bool>,
}

#[derive(Debug, serde::Deserialize)]
struct TriggersConfigFile136 {
    #[serde(default)]
    triggers: Vec<TriggerConfig136>,
}

fn run_triggers_command_136(argv: &[String]) -> CliOutput {
    match triggers_run_136(argv) {
        Ok(stdout) => CliOutput { code: 0, stdout, stderr: String::new() },
        Err(message) => CliOutput { code: 1, stdout: String::new(), stderr: format!("{message}\n") },
    }
}

fn triggers_run_136(argv: &[String]) -> Result<String, String> {
    triggers_parse_args_136(argv)?;
    let triggers = triggers_load_config_136()?;
    Ok(format!("{}\n", triggers_render_136(&triggers)))
}

fn triggers_parse_args_136(argv: &[String]) -> Result<(), String> {
    if argv.is_empty() { return Ok(()); }
    let arg = &argv[0];
    if matches!(arg.as_str(), "help" | "--help" | "-h") && argv.len() == 1 { return Ok(()); }
    if arg.starts_with('-') { Err(format!("triggers: unknown argument {arg}")) } else { Err(format!("triggers: unexpected argument {arg}")) }
}

fn triggers_load_config_136() -> Result<Vec<TriggerConfig136>, String> {
    let path = active_config_dir().join("maw.config.json");
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(format!("triggers: read {}: {error}", path.display())),
    };
    let value = serde_json::from_str::<serde_json::Value>(&text).map_err(|error| format!("triggers: parse {}: {error}", path.display()))?;
    let Some(raw_triggers) = value.get("triggers") else { return Ok(Vec::new()); };
    if !raw_triggers.is_array() { return Ok(Vec::new()); }
    let config = serde_json::from_value::<TriggersConfigFile136>(value).map_err(|error| format!("triggers: parse {}: {error}", path.display()))?;
    Ok(config.triggers.into_iter().filter(triggers_valid_136).collect())
}

fn triggers_valid_136(trigger: &TriggerConfig136) -> bool {
    triggers_valid_text_136(&trigger.on)
        && triggers_valid_text_136(&trigger.action)
        && trigger.repo.as_deref().is_none_or(triggers_valid_text_136)
}

fn triggers_valid_text_136(value: &str) -> bool {
    !value.is_empty() && value.trim() == value && value != "--" && !value.starts_with('-') && !value.chars().any(char::is_control)
}

fn triggers_render_136(triggers: &[TriggerConfig136]) -> String {
    if triggers.is_empty() { return triggers_empty_136(); }
    let mut out = format!("\n\x1b[36mWorkflow Triggers\x1b[0m  ({} configured)\n\n", triggers.len());
    out.push_str("  ");
    out.push_str(&triggers_pad_136("Event", 14));
    out.push_str(&triggers_pad_136("Repo/Filter", 30));
    out.push_str(&triggers_pad_136("Action", 40));
    out.push_str("Last Fired\n");
    out.push_str("  ");
    out.push_str(&"─".repeat(100));
    out.push('\n');
    for trigger in triggers {
        let event = triggers_event_label_136(trigger);
        let filter = trigger.repo.clone().unwrap_or_else(|| trigger.timeout.map_or_else(|| "—".to_owned(), |timeout| format!("timeout: {timeout}s")));
        let action = triggers_truncate_action_136(&trigger.action);
        out.push_str("  ");
        out.push_str(&triggers_pad_136(&event, 23));
        out.push_str(&triggers_pad_136(&filter, 30));
        out.push_str(&triggers_pad_136(&action, 40));
        out.push_str("\x1b[90m—\x1b[0m\n");
    }
    out
}

fn triggers_empty_136() -> String {
    concat!(
        "\x1b[90mNo triggers configured. Add a 'triggers' array to maw.config.json.\x1b[0m\n",
        "\n\x1b[90mExample:\x1b[0m\n",
        "  \"triggers\": [\n",
        "    { \"on\": \"issue-close\", \"repo\": \"Soul-Brews-Studio/maw-js\", \"action\": \"maw hey pulse-oracle 'issue closed'\" },\n",
        "    { \"on\": \"pr-merge\", \"repo\": \"Soul-Brews-Studio/maw-js\", \"action\": \"maw done neo-mawjs\" },\n",
        "    { \"on\": \"agent-idle\", \"timeout\": 30, \"action\": \"maw sleep {agent}\" }\n",
        "  ]"
    ).to_owned()
}

fn triggers_event_label_136(trigger: &TriggerConfig136) -> String {
    let mut event = match trigger.on.as_str() {
        "issue-close" => "\x1b[35missue-close\x1b[0m".to_owned(),
        "pr-merge" => "\x1b[32mpr-merge\x1b[0m".to_owned(),
        "agent-idle" => "\x1b[33magent-idle\x1b[0m".to_owned(),
        "agent-wake" => "\x1b[36magent-wake\x1b[0m".to_owned(),
        "agent-crash" => "\x1b[31magent-crash\x1b[0m".to_owned(),
        _ => trigger.on.clone(),
    };
    if trigger.once.unwrap_or(false) { event.push_str(" \x1b[33m[once]\x1b[0m"); }
    event
}

fn triggers_pad_136(value: &str, width: usize) -> String {
    if value.len() >= width { value.to_owned() } else { format!("{value}{}", " ".repeat(width - value.len())) }
}

fn triggers_truncate_action_136(value: &str) -> String {
    if value.len() > 38 { format!("{}...", &value[..35]) } else { value.to_owned() }
}
