const DISPATCH_380: &[DispatcherEntry] = &[DispatcherEntry { command: "census", handler: Handler::Sync(census_run_command) }];

#[derive(Debug, Clone, PartialEq, Eq)]
struct CensusPin { display: String, space: String, oracle: String, note: Option<String> }

#[derive(Debug, Clone, PartialEq, Eq)]
struct CensusOracle { oracle: String, session: Option<String>, pane: Option<String>, model_tier: String, status: String, idle_sec: Option<u64>, annotation: String, pinned: bool }

#[derive(Debug, Clone, PartialEq, Eq)]
struct CensusSpace { display: String, space: String, oracles: Vec<CensusOracle> }

fn census_run_command(argv: &[String]) -> CliOutput {
    let json = match census_parse_args(argv) { Ok(json) => json, Err(output) => return output };
    let mut client = TmuxClient::local();
    let options = LsPlanOptions { json: false, mode: LsMode::Verbose, all: true, channels: false, active: false, active_threshold_sec: None, recent: false, recent_limit: None, filter: None, peer: None, federation: false, node: None, fleet_only: false, teams: true, verify: false, fix: false, watch_interval_sec: None, now: Some(current_epoch_seconds()), panes: client.list_panes(), session_created: BTreeMap::new() };
    let panes = project_ls_panes(&options);
    let pins = census_read_pins(&census_default_pins_path());
    let spaces = census_join(&panes, &pins, &ls_annotation_context());
    CliOutput { code: 0, stdout: if json { census_render_json(&spaces) } else { census_render_text(&spaces) }, stderr: String::new() }
}

fn census_parse_args(argv: &[String]) -> Result<bool, CliOutput> {
    let mut json = false;
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            "--help" | "-h" => return Err(CliOutput { code: 0, stdout: "usage: maw census [--json]\n".to_owned(), stderr: String::new() }),
            _ => return Err(CliOutput { code: 2, stdout: String::new(), stderr: format!("census: unknown argument {arg}\nusage: maw census [--json]\n") }),
        }
    }
    Ok(json)
}

fn census_default_pins_path() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from(".config/window-arranger-oracle/pins.json"), |home| std::path::PathBuf::from(home).join(".config/window-arranger-oracle/pins.json"))
}

fn census_read_pins(path: &std::path::Path) -> Vec<CensusPin> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes) else { return Vec::new() };
    census_parse_pins_value(&value)
}

fn census_parse_pins_value(value: &serde_json::Value) -> Vec<CensusPin> {
    let rows = value.as_array().or_else(|| value.get("pins").and_then(serde_json::Value::as_array));
    rows.into_iter().flatten().filter_map(census_parse_pin).collect()
}

fn census_parse_pin(value: &serde_json::Value) -> Option<CensusPin> {
    let oracle = census_json_str(value, &["oracle", "name", "match", "session"])?;
    Some(CensusPin {
        display: census_json_str(value, &["displayName", "display", "displayId"]).unwrap_or("unassigned").to_owned(),
        space: census_json_str(value, &["space", "spaceName", "grid"]).unwrap_or("default").to_owned(),
        oracle: oracle.to_owned(),
        note: census_json_str(value, &["note"]).map(str::to_owned),
    })
}

fn census_json_str<'a>(value: &'a serde_json::Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|key| value.get(*key).and_then(serde_json::Value::as_str).filter(|text| !text.is_empty()))
}

fn census_join(panes: &[LsPanePlan], pins: &[CensusPin], annotations: &LsAnnotationContext) -> Vec<CensusSpace> {
    let mut groups = BTreeMap::<(String, String), Vec<CensusOracle>>::new();
    let mut pinned = BTreeSet::new();
    for pane in panes.iter().filter(|pane| pane.agent || pane.session.ends_with("-oracle") || is_default_ls_oracle_session(&pane.session)) {
        let oracle = census_oracle_name(&pane.session);
        let pin = pins.iter().find(|pin| census_same_oracle(&pin.oracle, &oracle) || census_same_oracle(&pin.oracle, &pane.session));
        let key = pin.map_or_else(|| ("unassigned".to_owned(), "live".to_owned()), |pin| { pinned.insert(census_norm(&pin.oracle)); (pin.display.clone(), pin.space.clone()) });
        groups.entry(key).or_default().push(CensusOracle { oracle, session: Some(pane.session.clone()), pane: Some(pane.id.clone()), model_tier: census_model_tier(&pane.command), status: pane.status.to_owned(), idle_sec: pane.age_sec, annotation: ls_pane_annotation(pane, annotations), pinned: pin.is_some() });
    }
    for pin in pins.iter().filter(|pin| !pinned.contains(&census_norm(&pin.oracle))) {
        groups.entry((pin.display.clone(), pin.space.clone())).or_default().push(CensusOracle { oracle: pin.oracle.clone(), session: None, pane: None, model_tier: "unknown".to_owned(), status: "pinned".to_owned(), idle_sec: None, annotation: pin.note.clone().unwrap_or_default(), pinned: true });
    }
    groups.into_iter().map(|((display, space), mut oracles)| { oracles.sort_by(|a, b| a.oracle.cmp(&b.oracle)); CensusSpace { display, space, oracles } }).collect()
}

fn census_oracle_name(session: &str) -> String {
    let stem = session.split_once('-').filter(|(prefix, _)| prefix.chars().all(|ch| ch.is_ascii_digit())).map_or(session, |(_, stem)| stem);
    stem.strip_suffix("-oracle").unwrap_or(stem).to_owned()
}

fn census_norm(value: &str) -> String { census_oracle_name(value).to_lowercase().replace(['_', ' '], "-") }
fn census_same_oracle(left: &str, right: &str) -> bool { census_norm(left) == census_norm(right) }

fn census_model_tier(command: &str) -> String {
    let command = command.to_lowercase();
    if command.contains("opus") { "opus" } else if command.contains("sonnet") { "sonnet" } else if command.contains("haiku") { "haiku" } else if command.contains("gpt-5") { "gpt-5" } else if command.contains("codex") { "codex" } else if command.contains("claude") { "claude" } else { "unknown" }.to_owned()
}

fn census_render_text(spaces: &[CensusSpace]) -> String {
    if spaces.is_empty() { return "maw census: no oracle panes or pins found\n".to_owned(); }
    let mut out = "maw census\n".to_owned();
    let mut current = "";
    for space in spaces {
        if current != space.display { current = &space.display; let _ = writeln!(out, "\nDisplay: {current}"); }
        let _ = writeln!(out, "  Space: {}", space.space);
        for oracle in &space.oracles {
            let pane = oracle.pane.as_deref().unwrap_or("-");
            let idle = oracle.idle_sec.map_or("-".to_owned(), |age| format_ls_age(Some(age)));
            let pin = if oracle.pinned { "📌" } else { " " };
            let _ = writeln!(out, "    {pin} {:<22} {:<8} {:<6} {:<8} {}", oracle.oracle, oracle.status, idle, pane, oracle.annotation);
        }
    }
    out
}

fn census_render_json(spaces: &[CensusSpace]) -> String {
    let mut by_display = BTreeMap::<&str, Vec<&CensusSpace>>::new();
    for space in spaces { by_display.entry(&space.display).or_default().push(space); }
    let displays = by_display.into_iter().map(|(display, spaces)| {
        let spaces = spaces.iter().map(|space| format!("{{\"name\":{},\"oracles\":{}}}", json_string(&space.space), census_oracles_json(&space.oracles))).collect::<Vec<_>>();
        format!("{{\"name\":{},\"spaces\":[{}]}}", json_string(display), spaces.join(","))
    }).collect::<Vec<_>>();
    format!("{{\"schema\":\"maw.census.v1\",\"displays\":[{}]}}\n", displays.join(","))
}

fn census_oracles_json(oracles: &[CensusOracle]) -> String {
    let rows = oracles.iter().map(|oracle| format!("{{\"oracle\":{},\"session\":{},\"pane\":{},\"modelTier\":{},\"status\":{},\"idleSec\":{},\"annotation\":{},\"pinned\":{}}}", json_string(&oracle.oracle), json_opt_str(oracle.session.as_deref()), json_opt_str(oracle.pane.as_deref()), json_string(&oracle.model_tier), json_string(&oracle.status), json_opt_u64(oracle.idle_sec), json_string(&oracle.annotation), oracle.pinned)).collect::<Vec<_>>();
    format!("[{}]", rows.join(","))
}

fn json_opt_str(value: Option<&str>) -> String { value.map_or_else(|| "null".to_owned(), json_string) }

#[cfg(test)]
mod census_tests {
    use super::*;

    fn fixture_panes() -> Vec<LsPanePlan> {
        let value: serde_json::Value = serde_json::from_str(include_str!("../../tests/fixtures/census/ls-panes.json")).expect("ls fixture");
        value.as_array().expect("rows").iter().map(|row| {
            let target = row["target"].as_str().expect("target");
            let session = target.split_once(':').map_or(target, |(session, _)| session);
            let age_sec = row["ageSec"].as_u64();
            LsPanePlan { id: row["id"].as_str().unwrap_or_default().to_owned(), target: target.to_owned(), session: session.to_owned(), command: row["command"].as_str().unwrap_or_default().to_owned(), title: String::new(), source: None, last_activity: None, session_created: None, status: ls_pane_status(age_sec), age_sec, agent: true }
        }).collect()
    }

    #[test]
    fn census_join_places_live_and_pinned_oracles() {
        let pins = census_parse_pins_value(&serde_json::from_str(include_str!("../../tests/fixtures/census/pins.json")).expect("pins"));
        let panes = fixture_panes();
        let spaces = census_join(&panes, &pins, &LsAnnotationContext::default());
        assert_eq!(spaces.len(), 2);
        assert_eq!(spaces[0].display, "Built-in");
        assert_eq!(spaces[0].oracles[0].oracle, "athena");
        assert_eq!(spaces[0].oracles[0].model_tier, "opus");
        assert!(spaces[1].oracles.iter().any(|oracle| oracle.oracle == "zai" && oracle.session.is_some()));
        assert!(spaces[1].oracles.iter().any(|oracle| oracle.oracle == "ghost" && oracle.status == "pinned"));
    }

    #[test]
    fn census_json_renders_contract_schema() {
        let spaces = vec![CensusSpace { display: "D1".to_owned(), space: "S1".to_owned(), oracles: vec![CensusOracle { oracle: "athena".to_owned(), session: Some("01-athena".to_owned()), pane: Some("%1".to_owned()), model_tier: "opus".to_owned(), status: "active".to_owned(), idle_sec: Some(1), annotation: "fleet: athena".to_owned(), pinned: true }] }];
        let value: serde_json::Value = serde_json::from_str(&census_render_json(&spaces)).expect("json");
        assert_eq!(value["schema"], "maw.census.v1");
        assert_eq!(value["displays"][0]["spaces"][0]["oracles"][0]["oracle"], "athena");
    }
}
