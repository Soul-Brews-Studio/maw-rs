fn run_bind_host_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => {
                return bind_host_constants_usage_error(&format!(
                    "bind-host constants: unknown arg {arg}"
                ))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_bind_host_constants_json()
        } else {
            "bind-host constants hosts=127.0.0.1,0.0.0.0 reasons=config.peers,config.namedPeers,MAW_HOST,peers.json\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_bind_host_constants_json() -> String {
    r#"{"command":"bind-host","action":"constants","hosts":{"loopback":"127.0.0.1","remote":"0.0.0.0"},"inputFlags":["config-peers-len","config-named-peers-len","maw-host","peers-store-len","peers-store-error"],"remoteReasons":["config.peers","config.namedPeers","MAW_HOST","peers.json"],"remoteMawHostValue":"0.0.0.0","priority":["config.peers","config.namedPeers","MAW_HOST","peers.json"]}
"#
    .to_owned()
}

fn bind_host_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", bind_host_constants_usage()),
    }
}

fn bind_host_constants_usage() -> &'static str {
    "usage: maw-rs bind-host constants [--plan-json]"
}

fn bind_host_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs bind-host [--config-peers-len <n>] [--config-named-peers-len <n>] [--maw-host <host>] [--peers-store-len <n>|--peers-store-error <err>] [--plan-json]\n"
        ),
    }
}

fn run_feed_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_feed_constants_plan(&argv[1..]);
    }

    let (plan_json, action) = match parse_feed_plan_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return feed_usage_error(&message),
    };
    match action {
        FeedPlanAction::ParseLine { line } => render_feed_parse_plan(plan_json, &line),
        FeedPlanAction::Describe { event, message } => {
            let event = feed_event("oracle-a", 1_000_000, &event, &message);
            let description = describe_activity(&event);
            render_feed_description(plan_json, &event, &description)
        }
        FeedPlanAction::Active {
            now,
            window,
            events,
        } => render_feed_active(plan_json, now, window, &events),
    }
}

fn parse_feed_plan_args(argv: &[String]) -> Result<(bool, FeedPlanAction), String> {
    let mut parser = FeedArgParser {
        plan_json: false,
        action: None,
    };
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--plan-json" => parser.plan_json = true,
            "parse-line" | "--parse-line" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("feed: missing parse-line value".to_owned());
                };
                parser.action = Some(FeedPlanAction::ParseLine {
                    line: value.to_owned(),
                });
                index += 1;
            }
            "describe" | "--describe" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("feed: missing describe event value".to_owned());
                };
                parser.action = Some(FeedPlanAction::Describe {
                    event: value.to_owned(),
                    message: String::new(),
                });
                index += 1;
            }
            "active" | "--active" => {
                parser.action = Some(FeedPlanAction::Active {
                    now: 0,
                    window: 0,
                    events: Vec::new(),
                });
            }
            "--message" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("feed: missing --message value".to_owned());
                };
                parser.set_message(value)?;
                index += 1;
            }
            "--now" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("feed: missing --now value".to_owned());
                };
                parser.set_active_number(value, FeedNumberKind::Now)?;
                index += 1;
            }
            "--window" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("feed: missing --window value".to_owned());
                };
                parser.set_active_number(value, FeedNumberKind::Window)?;
                index += 1;
            }
            "--event" => {
                let Some(value) = argv.get(index + 1) else {
                    return Err("feed: missing --event value".to_owned());
                };
                parser.add_active_event(value)?;
                index += 1;
            }
            arg => return Err(format!("feed: unknown argument {arg}")),
        }
        index += 1;
    }
    parser.finish()
}

struct FeedArgParser {
    plan_json: bool,
    action: Option<FeedPlanAction>,
}

impl FeedArgParser {
    fn set_message(&mut self, value: &str) -> Result<(), String> {
        self.action = match self.action.take() {
            Some(FeedPlanAction::Describe { event, .. }) => Some(FeedPlanAction::Describe {
                event,
                message: value.to_owned(),
            }),
            _ => return Err("feed: --message requires describe".to_owned()),
        };
        Ok(())
    }

    fn set_active_number(&mut self, value: &str, kind: FeedNumberKind) -> Result<(), String> {
        let parsed = value
            .parse::<i64>()
            .map_err(|_| format!("feed: {} must be an integer", kind.name()))?;
        self.action = match self.action.take() {
            Some(FeedPlanAction::Active {
                mut now,
                mut window,
                events,
            }) => {
                match kind {
                    FeedNumberKind::Now => now = parsed,
                    FeedNumberKind::Window => window = parsed,
                }
                Some(FeedPlanAction::Active {
                    now,
                    window,
                    events,
                })
            }
            _ => return Err(format!("feed: {} requires active", kind.name())),
        };
        Ok(())
    }

    fn add_active_event(&mut self, value: &str) -> Result<(), String> {
        let event = parse_feed_event_spec(value)?;
        self.action = match self.action.take() {
            Some(FeedPlanAction::Active {
                now,
                window,
                mut events,
            }) => {
                events.push(event);
                Some(FeedPlanAction::Active {
                    now,
                    window,
                    events,
                })
            }
            _ => return Err("feed: --event requires active".to_owned()),
        };
        Ok(())
    }

    fn finish(self) -> Result<(bool, FeedPlanAction), String> {
        self.action.map_or_else(
            || Err("feed: expected parse-line, describe, or active".to_owned()),
            |action| Ok((self.plan_json, action)),
        )
    }
}

#[derive(Clone, Copy)]
enum FeedNumberKind {
    Now,
    Window,
}

impl FeedNumberKind {
    const fn name(self) -> &'static str {
        match self {
            Self::Now => "--now",
            Self::Window => "--window",
        }
    }
}

enum FeedPlanAction {
    ParseLine {
        line: String,
    },
    Describe {
        event: String,
        message: String,
    },
    Active {
        now: i64,
        window: i64,
        events: Vec<FeedEvent>,
    },
}

fn parse_feed_event_spec(value: &str) -> Result<FeedEvent, String> {
    let mut parts = value.splitn(3, ':');
    let oracle = parts.next().unwrap_or_default();
    let Some(ts) = parts.next() else {
        return Err("feed: --event must be oracle:ts:message".to_owned());
    };
    let message = parts.next().unwrap_or_default();
    let ts = ts
        .parse::<i64>()
        .map_err(|_| "feed: --event ts must be an integer".to_owned())?;
    Ok(feed_event(oracle, ts, "Notification", message))
}

fn feed_event(oracle: &str, ts: i64, event: &str, message: &str) -> FeedEvent {
    FeedEvent {
        timestamp: "2026-05-18 12:00:00".to_owned(),
        oracle: oracle.to_owned(),
        host: "m5".to_owned(),
        event: event.to_owned(),
        project: "maw-js".to_owned(),
        session_id: "s1".to_owned(),
        message: message.to_owned(),
        ts,
    }
}

fn render_feed_parse_plan(plan_json: bool, line: &str) -> CliOutput {
    let parsed = parse_line(line);
    CliOutput {
        code: i32::from(parsed.is_none()),
        stdout: if plan_json {
            match parsed {
                Some(event) => format!(
                    "{{\"command\":\"feed\",\"kind\":\"parseLine\",\"parsed\":true,\"event\":{}}}\n",
                    render_feed_event_json(&event)
                ),
                None => "{\"command\":\"feed\",\"kind\":\"parseLine\",\"parsed\":false}\n".to_owned(),
            }
        } else {
            parsed.map_or_else(String::new, |event| format!("{}\n", event.message))
        },
        stderr: String::new(),
    }
}

fn render_feed_description(plan_json: bool, event: &FeedEvent, description: &str) -> CliOutput {
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"feed\",\"kind\":\"describe\",\"event\":{},\"description\":{}}}\n",
                render_feed_event_json(event),
                json_string(description)
            )
        } else {
            format!("{description}\n")
        },
        stderr: String::new(),
    }
}

fn render_feed_active(plan_json: bool, now: i64, window: i64, events: &[FeedEvent]) -> CliOutput {
    let active = active_oracles_at(events, now, window);
    let values: Vec<String> = active.values().map(render_feed_event_json).collect();
    CliOutput {
        code: 0,
        stdout: if plan_json {
            format!(
                "{{\"command\":\"feed\",\"kind\":\"active\",\"now\":{now},\"window\":{window},\"active\":[{}]}}\n",
                values.join(",")
            )
        } else {
            format!(
                "{}\n",
                active.keys().cloned().collect::<Vec<_>>().join("\n")
            )
        },
        stderr: String::new(),
    }
}

fn render_feed_event_json(event: &FeedEvent) -> String {
    format!(
        "{{\"timestamp\":{},\"oracle\":{},\"host\":{},\"event\":{},\"project\":{},\"sessionId\":{},\"message\":{},\"ts\":{}}}",
        json_string(&event.timestamp),
        json_string(&event.oracle),
        json_string(&event.host),
        json_string(&event.event),
        json_string(&event.project),
        json_string(&event.session_id),
        json_string(&event.message),
        event.ts
    )
}

fn run_feed_constants_plan(argv: &[String]) -> CliOutput {
    let mut plan_json = false;
    for arg in argv {
        match arg.as_str() {
            "--plan-json" => plan_json = true,
            arg => {
                return feed_constants_usage_error(&format!("feed constants: unknown arg {arg}"))
            }
        }
    }

    CliOutput {
        code: 0,
        stdout: if plan_json {
            render_feed_constants_json()
        } else {
            "feed constants actions=parse-line,describe,active fields=timestamp,oracle,host,event,project,sessionId,message,ts\n".to_owned()
        },
        stderr: String::new(),
    }
}

fn render_feed_constants_json() -> String {
    r#"{"command":"feed","action":"constants","actions":["parse-line","describe","active"],"eventFields":["timestamp","oracle","host","event","project","sessionId","message","ts"],"rowSeparator":" | ","messageDelimiter":" » ","timestampFormat":"YYYY-MM-DD HH:mm:ss","activeCutoff":"ts>=now-window","activeOrdering":"oracle asc, latest ts per oracle","descriptionTruncate":{"maxChars":60,"prefixChars":57,"suffix":"..."},"activityEvents":["PreToolUse","PostToolUse","PostToolUseFailure","UserPromptSubmit","SubagentStart","SubagentStop","SessionStart","SessionEnd","Stop","Notification"],"toolIcons":{"Bash":"⚡","Read":"📖","Edit":"✏️","Write":"📝","Grep":"🔍","Glob":"📂","Agent":"🤖","WebFetch":"🌐","WebSearch":"🔎","default":"🔧"}}
"#
    .to_owned()
}

fn feed_constants_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!("{message}\n{}\n", feed_constants_usage()),
    }
}

fn feed_constants_usage() -> &'static str {
    "usage: maw-rs feed constants [--plan-json]"
}

fn feed_usage_error(message: &str) -> CliOutput {
    CliOutput {
        code: 2,
        stdout: String::new(),
        stderr: format!(
            "{message}\nusage: maw-rs feed parse-line <line> [--plan-json]\n       maw-rs feed describe <event> [--message <message>] [--plan-json]\n       maw-rs feed active --now <ms> --window <ms> [--event <oracle:ts:message>]... [--plan-json]\n       maw-rs feed constants [--plan-json]\n"
        ),
    }
}

fn run_fuzzy_plan(argv: &[String]) -> CliOutput {
    if matches!(argv.first().map(String::as_str), Some("constants")) {
        return run_fuzzy_constants_plan(&argv[1..]);
    }

    let (plan_json, action) = match parse_fuzzy_plan_args(argv) {
        Ok(parsed) => parsed,
        Err(message) => return fuzzy_usage_error(&message),
    };

    match action {
        FuzzyPlanAction::Distance { left, right } => {
            render_fuzzy_distance(plan_json, &left, &right)
        }
        FuzzyPlanAction::Match {
            input,
            candidates,
            max_results,
            max_distance,
        } => render_fuzzy_match(plan_json, &input, &candidates, max_results, max_distance),
    }
}

