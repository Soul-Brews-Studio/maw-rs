// Reading back what was said, by correlating two half-records.
//
// The audit jsonl knows a send was attempted; the event log knows what became of
// it. Neither alone answers "did my message arrive", so `maw hey --log` joins
// them and renders the pairs, leaving unmatched audits visible rather than
// dropping them.

#[derive(Debug, Clone, PartialEq, Eq)] struct HeyLogOptions { since: Option<String>, from: Option<String>, suspicious: bool, limit: usize }
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)] struct HeyLogAudit { ts: String, cmd: String, args: Vec<String>, user: String }
#[derive(Debug, Clone, serde::Deserialize, PartialEq, Eq)] struct HeyLogEvent { ts: String, from: String, host: String, msg: String, route: String, to: String }
#[derive(Debug, Clone, PartialEq, Eq)] struct HeyLogRow { event: HeyLogEvent, user: Option<String>, reasons: Vec<&'static str> }

const HEY_LOG_USAGE: &str = "usage: maw-rs hey log [--since T] [--from X] [--suspicious] [-n N]";

fn hey_log_command(raw_args: &[String]) -> CliOutput {
    if wants_help(raw_args, &["--since", "--from", "-n"]) { return help_output(HEY_LOG_USAGE); }
    let options = match hey_log_parse_args(raw_args) {
        Ok(options) => options,
        Err(message) => return CliOutput { code: 2, stdout: String::new(), stderr: format!("{message}\n{HEY_LOG_USAGE}\n") },
    };
    CliOutput { code: 0, stdout: hey_log_render(&options), stderr: String::new() }
}

fn hey_log_parse_args(raw_args: &[String]) -> Result<HeyLogOptions, String> {
    let mut options = HeyLogOptions { since: None, from: None, suspicious: false, limit: 20 };
    let mut index = 0;
    while index < raw_args.len() {
        match raw_args[index].as_str() {
            "--since" => { options.since = Some(raw_args.get(index + 1).ok_or_else(|| "hey log: missing --since value".to_owned())?.clone()); index += 1; }
            value if value.starts_with("--since=") => options.since = Some(value["--since=".len()..].to_owned()),
            "--from" => { options.from = Some(raw_args.get(index + 1).ok_or_else(|| "hey log: missing --from value".to_owned())?.clone()); index += 1; }
            value if value.starts_with("--from=") => options.from = Some(value["--from=".len()..].to_owned()),
            "--suspicious" => options.suspicious = true,
            "-n" => { options.limit = hey_log_parse_limit(raw_args.get(index + 1).ok_or_else(|| "hey log: missing -n value".to_owned())?)?; index += 1; }
            value if value.starts_with("-n=") => options.limit = hey_log_parse_limit(&value["-n=".len()..])?,
            value if value.starts_with('-') => return Err(format!("hey log: unknown argument {value}")),
            value => return Err(format!("hey log: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(options)
}

fn hey_log_parse_limit(value: &str) -> Result<usize, String> {
    let limit = value.parse::<usize>().map_err(|_| "hey log: -n must be a positive integer".to_owned())?;
    (limit > 0).then_some(limit).ok_or_else(|| "hey log: -n must be a positive integer".to_owned())
}

fn hey_log_render(options: &HeyLogOptions) -> String {
    let env = real_xdg_env();
    let audits = hey_log_read_audits(&audit_jsonl_path(&env));
    let events = hey_log_read_events(&maw_data_path(&env, &["maw-log.jsonl"]));
    let mut rows = hey_log_correlate(&events, &audits);
    rows.retain(|row| {
        options.since.as_ref().is_none_or(|since| row.event.ts >= *since)
            && options.from.as_ref().is_none_or(|from| row.event.from == *from)
            && (!options.suspicious || !row.reasons.is_empty())
    });
    if rows.len() > options.limit { rows.drain(0..rows.len() - options.limit); }
    hey_log_format_rows(&rows)
}

fn hey_log_read_audits(path: &std::path::Path) -> Vec<HeyLogAudit> {
    std::fs::read_to_string(path).map_or_else(|_| Vec::new(), |text| {
        text.lines()
            .filter_map(|line| serde_json::from_str::<HeyLogAudit>(line).ok())
            .filter(|audit| audit.cmd == "hey" || audit.cmd == "send")
            .collect()
    })
}

fn hey_log_read_events(path: &std::path::Path) -> Vec<HeyLogEvent> {
    std::fs::read_to_string(path).map_or_else(|_| Vec::new(), |text| text.lines().filter_map(|line| serde_json::from_str(line).ok()).collect())
}

fn hey_log_correlate(events: &[HeyLogEvent], audits: &[HeyLogAudit]) -> Vec<HeyLogRow> {
    let mut by_ts = std::collections::BTreeMap::<String, Vec<usize>>::new();
    for (index, audit) in audits.iter().enumerate() { by_ts.entry(audit.ts.clone()).or_default().push(index); }
    let mut used = vec![false; audits.len()];
    events.iter().map(|event| {
        let audit_index = by_ts.get(&event.ts).and_then(|indexes| indexes.iter().copied().find(|index| !used[*index]));
        if let Some(index) = audit_index { used[index] = true; }
        let audit = audit_index.map(|index| &audits[index]);
        let mut reasons = Vec::new();
        if let Some(audit) = audit {
            if !hey_log_from_matches_user(&event.from, &audit.user) { reasons.push("from!=user"); }
            if hey_log_audit_from_unresolved(&audit.args) { reasons.push("bad --from"); }
        } else {
            reasons.push("missing audit");
        }
        if event.msg.starts_with('[') { reasons.push("prefix-bypass"); }
        HeyLogRow { event: event.clone(), user: audit.map(|audit| audit.user.clone()), reasons }
    }).collect()
}

fn hey_log_from_matches_user(from: &str, user: &str) -> bool { from == user || from.split(':').any(|part| part == user) }

fn hey_log_audit_from_unresolved(args: &[String]) -> bool {
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--from" => return args.get(index + 1).and_then(|value| explicit_wire_sender_oracle(value)).is_none(),
            value if value.starts_with("--from=") => return explicit_wire_sender_oracle(&value["--from=".len()..]).is_none(),
            _ => {}
        }
        index += 1;
    }
    false
}

fn hey_log_format_rows(rows: &[HeyLogRow]) -> String {
    use std::fmt::Write as _;
    if rows.is_empty() { return "No hey log entries.\n".to_owned(); }
    let mut output = "status\tts\tfrom\tuser\tto\troute\thost\tmsg\treasons\n".to_owned();
    for row in rows {
        let status = if row.reasons.is_empty() { "✓ verified" } else { "⚠ suspicious" };
        let reasons = if row.reasons.is_empty() { "-".to_owned() } else { row.reasons.join(",") };
        let msg = row.event.msg.replace(['\n', '\t'], " ");
        let _ = writeln!(output, "{status}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}", row.event.ts, row.event.from, row.user.as_deref().unwrap_or("-"), row.event.to, row.event.route, row.event.host, msg, reasons);
    }
    output
}
