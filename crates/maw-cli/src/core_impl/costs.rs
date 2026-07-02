const DISPATCH_37: &[DispatcherEntry] = &[
    DispatcherEntry { command: "costs", handler: Handler::Sync(run_costs_command) },
];

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct CostsAgentSummary {
    name: String,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_create_tokens: u64,
    total_tokens: u64,
    estimated_cost: f64,
    sessions: u64,
    turns: u64,
    models: BTreeMap<String, u64>,
    last_active: String,
}

#[derive(Debug, Clone, Default)]
struct CostsSessionUsage {
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    cache_create_tokens: u64,
    turns: u64,
    model: String,
    last_timestamp: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CostsArgs {
    daily: bool,
    days: usize,
    json: bool,
}

fn run_costs_command(raw_args: &[String]) -> CliOutput {
    let parsed = match costs_parse_costs_args(raw_args) {
        Ok(parsed) => parsed,
        Err(message) => return costs_usage_error(&message),
    };
    if parsed.daily {
        costs_render_costs_daily_command(parsed.days, parsed.json)
    } else {
        costs_render_costs_summary_command()
    }
}

fn costs_parse_costs_args(argv: &[String]) -> Result<CostsArgs, String> {
    let mut daily = false;
    let mut days = 7usize;
    let mut json = false;
    let mut index = 0;
    while index < argv.len() {
        match argv[index].as_str() {
            "--help" | "-h" => return Err(costs_usage_text().to_owned()),
            "--json" | "-j" => json = true,
            "--daily" => {
                daily = true;
                if let Some(next) = argv.get(index + 1) {
                    if !next.starts_with('-') {
                        days = costs_parse_costs_days(next)?;
                        index += 1;
                    }
                }
            }
            "--days" => {
                daily = true;
                let value = argv
                    .get(index + 1)
                    .filter(|value| !value.starts_with('-'))
                    .ok_or_else(|| "costs: missing --days value".to_owned())?;
                days = costs_parse_costs_days(value)?;
                index += 1;
            }
            other => return Err(format!("costs: unknown argument {other}")),
        }
        index += 1;
    }
    Ok(CostsArgs { daily, days, json })
}

fn costs_parse_costs_days(raw: &str) -> Result<usize, String> {
    let days = raw
        .parse::<usize>()
        .map_err(|_| "costs: days must be 1–365".to_owned())?;
    if !(1..=365).contains(&days) {
        return Err("costs: days must be 1–365".to_owned());
    }
    Ok(days)
}

fn costs_render_costs_summary_command() -> CliOutput {
    let projects_dir = costs_resolve_costs_projects_dir();
    let Ok(agents) = costs_collect_costs_agents(&projects_dir) else {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: "Cannot read ~/.claude/projects/\n".to_owned(),
        };
    };
    let agents = agents
        .into_iter()
        .filter(|agent| agent.sessions > 0)
        .collect::<Vec<_>>();
    if agents.is_empty() {
        return CliOutput {
            code: 0,
            stdout: "\x1b[90mno session data found\x1b[0m\n".to_owned(),
            stderr: String::new(),
        };
    }

    let total_agents = agents.len();
    let total_sessions = agents.iter().map(|agent| agent.sessions).sum::<u64>();
    let total_tokens = agents.iter().map(|agent| agent.total_tokens).sum::<u64>();
    let total_cost = agents.iter().map(|agent| agent.estimated_cost).sum::<f64>();
    let mut stdout = String::new();
    let _ = writeln!(stdout, "\n\x1b[36mCOST TRACKING\x1b[0m  ({total_agents} agents, {total_sessions} sessions)\n");
    let hdr = format!(
        "{}  {}  {}  {}  {}  {}",
        costs_pad_end("Agent", 30),
        costs_pad_start("Tokens", 14),
        costs_pad_start("Est. Cost", 12),
        costs_pad_start("Sessions", 10),
        costs_pad_start("Turns", 8),
        costs_pad_start("Last Active", 13),
    );
    let _ = writeln!(stdout, "  \x1b[90m{hdr}\x1b[0m");
    let _ = writeln!(stdout, "  \x1b[90m{}\x1b[0m", "─".repeat(hdr.chars().count()));

    for agent in &agents {
        let name = costs_truncate_ellipsis(&agent.name, 28);
        let tokens = costs_pad_start(&costs_fmt_costs_num(agent.total_tokens), 14);
        let cost = costs_pad_start(&format!("${:.2}", agent.estimated_cost), 12);
        let cost_color = costs_color(agent.estimated_cost, 10.0, 1.0);
        let sessions = costs_pad_start(&agent.sessions.to_string(), 10);
        let turns = costs_pad_start(&agent.turns.to_string(), 8);
        let last_active = if agent.last_active.is_empty() { "—".to_owned() } else { agent.last_active.chars().take(10).collect() };
        let _ = writeln!(
            stdout,
            "  {}  {}  {}{}\x1b[0m  {}  {}  {}",
            costs_pad_end(&name, 30),
            tokens,
            cost_color,
            cost,
            sessions,
            turns,
            costs_pad_start(&last_active, 13),
        );
    }
    let _ = writeln!(stdout, "  \x1b[90m{}\x1b[0m", "─".repeat(hdr.chars().count()));
    let total_cost_color = costs_color(total_cost, 50.0, 10.0);
    let _ = writeln!(
        stdout,
        "  {}  {}  {}{}\x1b[0m  {}",
        costs_pad_end("TOTAL", 30),
        costs_pad_start(&costs_fmt_costs_num(total_tokens), 14),
        total_cost_color,
        costs_pad_start(&format!("${total_cost:.2}"), 12),
        costs_pad_start(&total_sessions.to_string(), 10),
    );
    stdout.push('\n');
    CliOutput { code: 0, stdout, stderr: String::new() }
}

fn costs_render_costs_daily_command(days: usize, json_output: bool) -> CliOutput {
    let projects_dir = costs_resolve_costs_projects_dir();
    let buckets = costs_make_costs_buckets(days);
    let Ok(daily) = costs_collect_costs_daily(&projects_dir, &buckets) else {
        return CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: "Cannot read ~/.claude/projects/\n".to_owned(),
        };
    };
    let total_cost = daily.iter().map(|agent| agent.total_cost).sum::<f64>();
    let total_agents = daily.len();
    if json_output {
        let response = CostsDailyResponse {
            window: days,
            buckets,
            agents: daily,
            total: CostsDailyTotal { cost: total_cost, agents: total_agents },
        };
        return CliOutput {
            code: 0,
            stdout: serde_json::to_string_pretty(&response).unwrap_or_else(|_| "{}".to_owned()) + "\n",
            stderr: String::new(),
        };
    }
    if daily.is_empty() {
        return CliOutput {
            code: 0,
            stdout: format!("\x1b[90mno activity in the last {days} days\x1b[0m\n"),
            stderr: String::new(),
        };
    }
    let last_bucket = buckets.last().cloned().unwrap_or_default();
    let mut stdout = String::new();
    let _ = writeln!(stdout, "\n\x1b[36mDAILY COSTS\x1b[0m  ({days}d ending {last_bucket})\n");
    let name_width = 28usize;
    for agent in &daily {
        let name = if agent.name.chars().count() > name_width {
            costs_truncate_ellipsis(&agent.name, name_width - 1)
        } else {
            costs_pad_end(&agent.name, name_width)
        };
        let _ = writeln!(
            stdout,
            "  {}  {}  ${:.2}",
            name,
            costs_sparkline(&agent.daily_costs, Some(&agent.had_activity)),
            agent.total_cost,
        );
    }
    let total_costs = (0..days)
        .map(|index| daily.iter().map(|agent| agent.daily_costs[index]).sum::<f64>())
        .collect::<Vec<_>>();
    let total_had = total_costs.iter().map(|value| *value > 0.0).collect::<Vec<_>>();
    let sep_len = name_width + 2 + days + 4;
    let _ = writeln!(stdout, "  {}", "─".repeat(sep_len));
    let _ = writeln!(
        stdout,
        "  {}  {}  ${:.2}",
        costs_pad_end("TOTAL", name_width),
        costs_sparkline(&total_costs, Some(&total_had)),
        total_cost,
    );
    stdout.push('\n');
    CliOutput { code: 0, stdout, stderr: String::new() }
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CostsDailyResponse {
    window: usize,
    buckets: Vec<String>,
    agents: Vec<CostsDailyAgent>,
    total: CostsDailyTotal,
}

#[derive(Debug, Clone, serde::Serialize)]
struct CostsDailyTotal {
    cost: f64,
    agents: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct CostsDailyAgent {
    name: String,
    daily_costs: Vec<f64>,
    total_cost: f64,
    had_activity: Vec<bool>,
}

fn costs_collect_costs_daily(
    projects_dir: &std::path::Path,
    buckets: &[String],
) -> Result<Vec<CostsDailyAgent>, std::io::Error> {
    let dirs = costs_project_dirs(projects_dir)?;
    let bucket_index = buckets
        .iter()
        .enumerate()
        .map(|(index, bucket)| (bucket.as_str(), index))
        .collect::<BTreeMap<_, _>>();
    let mut daily: BTreeMap<String, CostsDailyAgent> = BTreeMap::new();
    for dir in dirs {
        let dir_path = projects_dir.join(&dir);
        let Ok(files) = costs_jsonl_files(&dir_path) else { continue; };
        if files.is_empty() {
            continue;
        }
        let name = costs_agent_name_from_dir(&dir);
        daily.entry(name.clone()).or_insert_with(|| CostsDailyAgent {
            name: name.clone(),
            daily_costs: vec![0.0; buckets.len()],
            total_cost: 0.0,
            had_activity: vec![false; buckets.len()],
        });
        for file in files {
            let Some(usage) = costs_scan_costs_session_file(&dir_path.join(file)) else { continue; };
            if usage.last_timestamp.is_empty() {
                continue;
            }
            let date = costs_local_date_str(&usage.last_timestamp);
            let Some(index) = bucket_index.get(date.as_str()).copied() else { continue; };
            let cost = costs_estimate_costs_session(&usage);
            if let Some(agent) = daily.get_mut(&name) {
                agent.daily_costs[index] += cost;
                agent.total_cost += cost;
                agent.had_activity[index] = true;
            }
        }
    }
    let mut agents = daily
        .into_values()
        .filter(|agent| agent.total_cost > 0.0)
        .collect::<Vec<_>>();
    agents.sort_by(|a, b| b.total_cost.total_cmp(&a.total_cost));
    Ok(agents)
}

fn costs_collect_costs_agents(
    projects_dir: &std::path::Path,
) -> Result<Vec<CostsAgentSummary>, std::io::Error> {
    let dirs = costs_project_dirs(projects_dir)?;
    let mut agents: BTreeMap<String, CostsAgentSummary> = BTreeMap::new();
    for dir in dirs {
        let dir_path = projects_dir.join(&dir);
        let Ok(files) = costs_jsonl_files(&dir_path) else { continue; };
        if files.is_empty() {
            continue;
        }
        let name = costs_agent_name_from_dir(&dir);
        agents.entry(name.clone()).or_insert_with(|| CostsAgentSummary {
            name: name.clone(),
            ..CostsAgentSummary::default()
        });
        for file in files {
            let Some(usage) = costs_scan_costs_session_file(&dir_path.join(file)) else { continue; };
            if let Some(agent) = agents.get_mut(&name) {
                agent.input_tokens += usage.input_tokens;
                agent.output_tokens += usage.output_tokens;
                agent.cache_read_tokens += usage.cache_read_tokens;
                agent.cache_create_tokens += usage.cache_create_tokens;
                agent.total_tokens += usage.input_tokens
                    + usage.output_tokens
                    + usage.cache_read_tokens
                    + usage.cache_create_tokens;
                agent.estimated_cost += costs_estimate_costs_session(&usage);
                agent.sessions += 1;
                agent.turns += usage.turns;
                let tier = costs_model_tier(&usage.model).to_owned();
                *agent.models.entry(tier).or_insert(0) += usage.turns;
                if usage.last_timestamp > agent.last_active {
                    agent.last_active = usage.last_timestamp;
                }
            }
        }
    }
    let mut agents = agents.into_values().filter(|agent| agent.sessions > 0).collect::<Vec<_>>();
    agents.sort_by(|a, b| b.estimated_cost.total_cmp(&a.estimated_cost));
    Ok(agents)
}

fn costs_project_dirs(projects_dir: &std::path::Path) -> Result<Vec<String>, std::io::Error> {
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(projects_dir)? {
        let Ok(entry) = entry else { continue; };
        let Ok(file_type) = entry.file_type() else { continue; };
        if file_type.is_dir() {
            dirs.push(entry.file_name().to_string_lossy().into_owned());
        }
    }
    Ok(dirs)
}

fn costs_jsonl_files(dir_path: &std::path::Path) -> Result<Vec<std::ffi::OsString>, std::io::Error> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir_path)? {
        let Ok(entry) = entry else { continue; };
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) == Some("jsonl") {
            files.push(entry.file_name());
        }
    }
    Ok(files)
}

fn costs_scan_costs_session_file(path: &std::path::Path) -> Option<CostsSessionUsage> {
    let content = std::fs::read_to_string(path).ok()?;
    costs_scan_costs_session_content(&content)
}

fn costs_scan_costs_session_content(content: &str) -> Option<CostsSessionUsage> {
    let mut usage = CostsSessionUsage::default();
    for line in content.lines().filter(|line| !line.is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else { continue; };
        if value.get("type").and_then(serde_json::Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(message_usage) = value.get("message").and_then(|message| message.get("usage")) else { continue; };
        usage.input_tokens += costs_json_u64(message_usage, "input_tokens");
        usage.output_tokens += costs_json_u64(message_usage, "output_tokens");
        usage.cache_read_tokens += costs_json_u64(message_usage, "cache_read_input_tokens");
        usage.cache_create_tokens += costs_json_u64(message_usage, "cache_creation_input_tokens");
        usage.turns += 1;
        if usage.model.is_empty() {
            if let Some(model) = value
                .get("message")
                .and_then(|message| message.get("model"))
                .and_then(serde_json::Value::as_str)
            {
                model.clone_into(&mut usage.model);
            }
        }
        if let Some(timestamp) = value.get("timestamp").and_then(serde_json::Value::as_str) {
            timestamp.clone_into(&mut usage.last_timestamp);
        }
    }
    (usage.turns > 0).then_some(usage)
}

fn costs_json_u64(value: &serde_json::Value, key: &str) -> u64 {
    value.get(key).and_then(serde_json::Value::as_u64).unwrap_or(0)
}

#[allow(clippy::cast_precision_loss)]
fn costs_estimate_costs_session(usage: &CostsSessionUsage) -> f64 {
    let (input_rate, output_rate) = match costs_model_tier(&usage.model) {
        "opus" => (15.0, 75.0),
        "haiku" => (0.25, 1.25),
        _ => (3.0, 15.0),
    };
    let total_input = usage.input_tokens + usage.cache_read_tokens + usage.cache_create_tokens;
    (total_input as f64 / 1_000_000.0) * input_rate
        + (usage.output_tokens as f64 / 1_000_000.0) * output_rate
}

fn costs_model_tier(model: &str) -> &'static str {
    if model.contains("opus") {
        "opus"
    } else if model.contains("haiku") {
        "haiku"
    } else {
        "sonnet"
    }
}

fn costs_agent_name_from_dir(dir: &str) -> String {
    let trimmed = dir.strip_prefix('-').unwrap_or(dir);
    let parts = trimmed.split('-').collect::<Vec<_>>();
    if let Some(gh_idx) = parts.iter().position(|part| *part == "github") {
        if parts.get(gh_idx + 1) == Some(&"com") && parts.len() > gh_idx + 3 {
            return parts[gh_idx + 2..].join("-");
        }
    }
    if parts.len() >= 2 {
        parts[parts.len() - 2..].join("-")
    } else {
        trimmed.to_owned()
    }
}

fn costs_resolve_costs_projects_dir() -> std::path::PathBuf {
    if let Some(value) = std::env::var_os("MAW_CLAUDE_PROJECTS_DIR") {
        return std::path::PathBuf::from(value);
    }
    if let Some(value) = costs_projects_dir_from_config() {
        return std::path::PathBuf::from(costs_expand_costs_home(&value));
    }
    if let Some(value) = std::env::var_os("CLAUDE_HOME") {
        return std::path::PathBuf::from(value).join("projects");
    }
    std::env::var_os("HOME").map_or_else(|| std::path::PathBuf::from("."), std::path::PathBuf::from)
        .join(".claude")
        .join("projects")
}

fn costs_projects_dir_from_config() -> Option<String> {
    let env = real_xdg_env();
    let path = maw_config_path(&env, &["maw.config.json"]);
    let raw = std::fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&raw).ok()?;
    let configured = [
        value.pointer("/env/MAW_CLAUDE_PROJECTS_DIR"),
        value.pointer("/costs/projectsDir"),
        value.pointer("/claude/projectsDir"),
        value.get("claudeProjectsDir"),
    ]
    .into_iter()
    .flatten()
    .find_map(|value| value.as_str().map(ToOwned::to_owned));
    configured
}

fn costs_expand_costs_home(value: &str) -> String {
    if let Some(rest) = value.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return std::path::PathBuf::from(home).join(rest).to_string_lossy().into_owned();
        }
    }
    value.to_owned()
}

fn costs_make_costs_buckets(days: usize) -> Vec<String> {
    let today = std::env::var("MAW_COSTS_TODAY")
        .ok()
        .filter(|value| value.len() >= 10)
        .map_or_else(costs_today_utc_date, |value| value[..10].to_owned());
    let (year, month, day) = costs_parse_ymd(&today).unwrap_or((1970, 1, 1));
    let today_days = costs_days_from_civil(year, month, day);
    (0..days)
        .map(|index| {
            let offset = i64::try_from(days - 1 - index).unwrap_or(i64::MAX);
            costs_civil_from_days(today_days - offset)
        })
        .collect()
}

fn costs_today_utc_date() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| i64::try_from(duration.as_secs()).unwrap_or(i64::MAX));
    costs_civil_from_days(secs.div_euclid(86_400))
}

fn costs_local_date_str(iso_ts: &str) -> String {
    iso_ts.chars().take(10).collect()
}

fn costs_parse_ymd(raw: &str) -> Option<(i64, u32, u32)> {
    let mut parts = raw.split('-');
    let year = parts.next()?.parse().ok()?;
    let month = parts.next()?.parse().ok()?;
    let day = parts.next()?.parse().ok()?;
    Some((year, month, day))
}

fn costs_days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = i64::from(month);
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn costs_civil_from_days(days: i64) -> String {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn costs_sparkline(values: &[f64], had_activity: Option<&[bool]>) -> String {
    const BLOCKS: &[char] = &[' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let active = had_activity.map_or(*value > 0.0, |activity| activity.get(index).copied().unwrap_or(false));
            if !active {
                return '░';
            }
            if max == 0.0 {
                return '▁';
            }
            let norm = ((*value / max) * 7.0).round() as usize;
            BLOCKS[norm + 1]
        })
        .collect()
}

#[allow(clippy::cast_precision_loss)]
fn costs_fmt_costs_num(n: u64) -> String {
    if n >= 1_000_000_000 {
        format!("{:.1}B", n as f64 / 1_000_000_000.0)
    } else if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn costs_pad_start(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        value.to_owned()
    } else {
        format!("{}{}", " ".repeat(width - len), value)
    }
}

fn costs_pad_end(value: &str, width: usize) -> String {
    let len = value.chars().count();
    if len >= width {
        value.to_owned()
    } else {
        format!("{}{}", value, " ".repeat(width - len))
    }
}

fn costs_truncate_ellipsis(value: &str, max_prefix_chars: usize) -> String {
    if value.chars().count() <= max_prefix_chars {
        value.to_owned()
    } else {
        format!("{}…", value.chars().take(max_prefix_chars).collect::<String>())
    }
}

fn costs_color(value: f64, high: f64, medium: f64) -> &'static str {
    if value > high {
        "\x1b[31m"
    } else if value > medium {
        "\x1b[33m"
    } else {
        "\x1b[32m"
    }
}

fn costs_usage_error(message: &str) -> CliOutput {
    let stderr = if message == costs_usage_text() {
        format!("{message}\n")
    } else {
        format!("{message}\n{}\n", costs_usage_text())
    };
    CliOutput { code: 2, stdout: String::new(), stderr }
}

fn costs_usage_text() -> &'static str {
    "usage: maw-rs costs [--daily [N]|--days N] [--json]"
}
