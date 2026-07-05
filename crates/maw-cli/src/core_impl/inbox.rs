const DISPATCH_62: &[DispatcherEntry] = &[DispatcherEntry {
    command: "inbox",
    handler: Handler::Async(run_inbox_command),
}];

const INBOX_USAGE: &str = "maw inbox [--unread] [--from <peer>] [--last N] | status [oracle-name] [--json] [--all] | drain [oracle-name] --safe [--max N] [--older-than-hours H] [--json] [--dry-run] | read <id> | show [N] | write <msg> | pending | approve <id> | reject <id> | show-pending <id>";
const INBOX_SAFE_DRAIN_DEFAULT_MAX: usize = 25;
const INBOX_SAFE_DRAIN_DEFAULT_MIN_AGE_SECONDS: u64 = 4 * 60 * 60;
const INBOX_UNREAD_RED_THRESHOLD: usize = 50;
const INBOX_OLDEST_RED_SECONDS: u64 = 4 * 60 * 60;
const INBOX_ARCHIVE_RED_SECONDS: u64 = 8 * 60 * 60;
const INBOX_PENDING_TTL_SECONDS: u64 = 30 * 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboxEnv {
    inbox_dir: std::path::PathBuf,
    pending_dir: std::path::PathBuf,
    state_dir: std::path::PathBuf,
    oracle: String,
    node: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboxMessage {
    id: String,
    filename: String,
    path: std::path::PathBuf,
    from: String,
    to: String,
    timestamp_ms: u64,
    read: bool,
    body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct InboxPendingMessage {
    id: String,
    sender: String,
    target: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    query: Option<String>,
    #[serde(rename = "sentAt")]
    sent_at: String,
    status: String,
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct InboxStatus {
    oracle: String,
    unread: usize,
    oldest_age_seconds: Option<u64>,
    last_archive_age_seconds: Option<u64>,
    delta_since_last_check: i64,
    level: String,
    reasons: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct InboxDrainResult {
    oracle: String,
    scanned: usize,
    matched: usize,
    archived: usize,
    remaining_matches: usize,
    max: usize,
    dry_run: bool,
    safe: bool,
    older_than_seconds: u64,
    processed_dir: String,
    items: Vec<InboxDrainItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
struct InboxDrainItem {
    id: String,
    filename: String,
    reason: String,
    age_seconds: u64,
    destination: Option<String>,
    action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct InboxCursorEntry {
    unread: usize,
    #[serde(rename = "latestArchiveMtimeMs")]
    latest_archive_mtime_ms: Option<u64>,
    #[serde(rename = "checkedAt")]
    checked_at: String,
}

type InboxCursorStore = BTreeMap<String, InboxCursorEntry>;

trait InboxSender {
    fn inbox_send<'a>(
        &'a mut self,
        query: &'a str,
        message: &'a str,
        acl_bypass: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>>;
}

struct InboxSystemSender;

impl InboxSender for InboxSystemSender {
    fn inbox_send<'a>(
        &'a mut self,
        query: &'a str,
        message: &'a str,
        acl_bypass: bool,
    ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
        Box::pin(async move {
            inbox_validate_target_arg(query, "query")?;
            let output = run_hey_in_process(query, message, acl_bypass).await;
            if output.code == 0 {
                Ok(())
            } else {
                let detail = if output.stderr.trim().is_empty() {
                    output.stdout.trim().to_owned()
                } else {
                    output.stderr.trim().to_owned()
                };
                Err(format!("inbox: maw hey failed: {detail}"))
            }
        })
    }
}

fn run_inbox_command(args: Vec<String>) -> Pin<Box<dyn Future<Output = CliOutput> + Send>> {
    Box::pin(async move {
        match inbox_run(&args, &inbox_real_env(), &mut InboxSystemSender).await {
        Ok(stdout) => CliOutput {
            code: 0,
            stdout,
            stderr: String::new(),
        },
        Err(message) => CliOutput {
            code: 1,
            stdout: String::new(),
            stderr: format!("{message}\n"),
        },
        }
    })
}

async fn inbox_run(
    argv: &[String],
    env: &InboxEnv,
    sender: &mut impl InboxSender,
) -> Result<String, String> {
    if wants_help_before_positionals(argv, &["--from", "--last"]) {
        return Ok(format!("usage: {INBOX_USAGE}\n"));
    }
    match argv.first().map(String::as_str) {
        Some("pending" | "queue") => inbox_run_pending(env, inbox_now_ms()),
        Some("show-pending" | "pending-show") => inbox_run_show_pending(&argv[1..], env, inbox_now_ms()),
        Some("approve") => inbox_run_approve(&argv[1..], env, sender, inbox_now_ms()).await,
        Some("reject") => inbox_run_reject(&argv[1..], env, inbox_now_ms()),
        Some("list" | "ls") => inbox_run_list(&argv[1..], env, inbox_now_ms()),
        Some("read") => inbox_run_mark_read(&argv[1..], env),
        Some("show") => inbox_run_show(&argv[1..], env),
        Some("write") => inbox_run_write(&argv[1..], env, inbox_now_ms()),
        Some("status") => inbox_run_status(&argv[1..], env, inbox_now_ms()),
        Some("drain") => inbox_run_drain(&argv[1..], env, inbox_now_ms()),
        Some(value) if value.starts_with('-') => inbox_run_list(argv, env, inbox_now_ms()),
        Some(value) => Err(format!("inbox: unknown subcommand {value}")),
        None => inbox_run_list(argv, env, inbox_now_ms()),
    }
}

fn inbox_real_env() -> InboxEnv {
    let xdg = current_xdg_env();
    let config_dir = maw_config_dir(&xdg);
    let state_dir = maw_state_dir(&xdg);
    let config = merged_config_value_for_env(&xdg);
    let inbox_dir = inbox_resolve_dir(&config);
    InboxEnv {
        inbox_dir,
        pending_dir: config_dir.join("pending"),
        state_dir,
        oracle: inbox_config_string(&config, "oracle", "local"),
        node: inbox_config_string(&config, "node", "cli"),
    }
}

fn inbox_state_pending_dir(env: &InboxEnv) -> std::path::PathBuf {
    env.state_dir.join("pending")
}

fn inbox_config_string(config: &serde_json::Value, key: &str, fallback: &str) -> String {
    config
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

fn inbox_resolve_dir(config: &serde_json::Value) -> std::path::PathBuf {
    if let Some(psi) = config.get("psiPath").and_then(serde_json::Value::as_str) {
        return std::path::Path::new(psi).join("inbox");
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
    let unicode = cwd.join("ψ").join("inbox");
    if unicode.exists() {
        unicode
    } else {
        cwd.join("psi").join("inbox")
    }
}

fn inbox_run_list(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let options = inbox_parse_list_args(argv)?;
    let messages = inbox_load_messages(&env.inbox_dir)?;
    let rows = inbox_list_rows(&messages, &options);
    Ok(inbox_render_list(
        &rows,
        options.last.unwrap_or(20),
        now_ms,
    ))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct InboxListOptions {
    unread: bool,
    from: Option<String>,
    last: Option<usize>,
}

fn inbox_parse_list_args(argv: &[String]) -> Result<InboxListOptions, String> {
    let mut options = InboxListOptions::default();
    let mut index = 0_usize;
    while index < argv.len() {
        match argv[index].as_str() {
            "--unread" => options.unread = true,
            "--from" => {
                let value = inbox_required_value(argv, index, "--from")?;
                inbox_validate_target_arg(value, "from")?;
                options.from = Some(value.to_owned());
                index += 1;
            }
            "--last" => {
                let value = inbox_required_value(argv, index, "--last")?;
                options.last = Some(inbox_parse_usize(value, "--last")?);
                index += 1;
            }
            value if value.starts_with("--from=") => {
                let value = value.trim_start_matches("--from=");
                inbox_validate_target_arg(value, "from")?;
                options.from = Some(value.to_owned());
            }
            value if value.starts_with("--last=") => {
                options.last = Some(inbox_parse_usize(
                    value.trim_start_matches("--last="),
                    "--last",
                )?);
            }
            value if value.starts_with('-') => {
                return Err(format!("inbox: unknown argument {value}"))
            }
            value => return Err(format!("inbox: unexpected argument {value}")),
        }
        index += 1;
    }
    Ok(options)
}

#[derive(Debug, Clone, Copy)]
struct InboxListRow<'a> {
    id: usize,
    message: &'a InboxMessage,
}

fn inbox_list_rows<'a>(
    messages: &'a [InboxMessage],
    options: &InboxListOptions,
) -> Vec<InboxListRow<'a>> {
    messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            if options.unread && message.read {
                return None;
            }
            if options
                .from
                .as_ref()
                .is_some_and(|from| &message.from != from)
            {
                return None;
            }
            Some(InboxListRow {
                id: index + 1,
                message,
            })
        })
        .collect()
}

fn inbox_render_list(rows: &[InboxListRow<'_>], limit: usize, now_ms: u64) -> String {
    if rows.is_empty() {
        return "\u{001b}[90mno inbox messages\u{001b}[0m\n".to_owned();
    }
    let mut out = format!(
        "\n\u{001b}[36mINBOX\u{001b}[0m ({} total)\n\n",
        rows.len()
    );
    out.push_str("  ID R FROM           WHEN       SUBJECT\n");
    out.push_str("  -- - -------------- ---------- --------------------------------------------\n");
    for row in rows.iter().take(limit) {
        inbox_render_list_row(&mut out, row, now_ms);
    }
    out.push('\n');
    out
}

fn inbox_render_list_row(out: &mut String, row: &InboxListRow<'_>, now_ms: u64) {
    let message = row.message;
    let dot = if message.read {
        "\u{001b}[90m○\u{001b}[0m"
    } else {
        "\u{001b}[32m●\u{001b}[0m"
    };
    let from = inbox_pad(&inbox_truncate(&message.from, 14), 14);
    let when = inbox_pad(&inbox_relative_time(message.timestamp_ms, now_ms), 10);
    let subject = inbox_truncate(&message.body.replace('\n', " "), 50);
    let _ = writeln!(out, "  {:>2} {dot} {from} {when} {subject}", row.id);
}

fn inbox_run_mark_read(argv: &[String], env: &InboxEnv) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox read <id>")?;
    let Some(message) = inbox_find_message(&env.inbox_dir, id)? else {
        return Err(format!("message not found: {id}"));
    };
    let mut out = inbox_render_show(&message);
    if message.read {
        let _ = writeln!(
            out,
            "\n\u{001b}[90malready read:\u{001b}[0m {}",
            message.filename
        );
        return Ok(out);
    }
    let content = std::fs::read_to_string(&message.path)
        .map_err(|error| format!("inbox: read {}: {error}", message.path.display()))?;
    let updated = inbox_mark_frontmatter_read(&content, inbox_now_ms());
    if updated == content {
        return Err(format!("could not mark read: {}", message.filename));
    }
    std::fs::write(&message.path, updated)
        .map_err(|error| format!("inbox: write {}: {error}", message.path.display()))?;
    let _ = writeln!(
        out,
        "\n\u{001b}[32m✓\u{001b}[0m marked read: {}",
        message.filename
    );
    Ok(out)
}

fn inbox_run_show(argv: &[String], env: &InboxEnv) -> Result<String, String> {
    if argv.len() > 1 {
        return Err("usage: maw inbox show [N|name]".to_owned());
    }
    if let Some(value) = argv.first() {
        inbox_validate_lookup_arg(value, "message")?;
    }
    let messages = inbox_load_messages(&env.inbox_dir)?;
    if messages.is_empty() {
        return Ok("\u{001b}[90mno inbox messages\u{001b}[0m\n".to_owned());
    }
    let target = argv.first().map(String::as_str);
    let Some(message) = inbox_pick_message(&messages, target) else {
        return Ok(format!(
            "\u{001b}[31merror\u{001b}[0m: not found: {}\n",
            target.unwrap_or_default()
        ));
    };
    Ok(inbox_render_show(message))
}

fn inbox_run_write(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let note = inbox_parse_write_note(argv)?;
    if !env.inbox_dir.exists() {
        return Ok(format!(
            "\u{001b}[31merror\u{001b}[0m: inbox not found: {}\n",
            env.inbox_dir.display()
        ));
    }
    let filename = inbox_write_file(&env.inbox_dir, &env.node, &env.node, &note, now_ms)?;
    Ok(format!(
        "\u{001b}[32m✓\u{001b}[0m wrote \u{001b}[33m{filename}\u{001b}[0m\n"
    ))
}

fn inbox_parse_write_note(argv: &[String]) -> Result<String, String> {
    let mut note_args = argv;
    if note_args.first().is_some_and(|arg| arg == "--") {
        note_args = &note_args[1..];
    } else if note_args.first().is_some_and(|arg| arg.starts_with('-')) {
        return Err("inbox: write message starting with '-' requires -- separator".to_owned());
    }
    if note_args.is_empty() {
        return Err("usage: maw inbox write <msg>".to_owned());
    }
    Ok(note_args.join(" "))
}

fn inbox_run_status(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let (oracle, json, all) = inbox_parse_status_args(argv)?;
    if all {
        let status = inbox_build_status(&env.oracle, &env.inbox_dir, env, now_ms)?;
        let statuses = vec![status];
        return inbox_render_status_list(&statuses, json);
    }
    let oracle = oracle.unwrap_or_else(|| env.oracle.clone());
    let status = inbox_build_status(&oracle, &env.inbox_dir, env, now_ms)?;
    inbox_render_status(&status, json)
}

fn inbox_parse_status_args(argv: &[String]) -> Result<(Option<String>, bool, bool), String> {
    let mut oracle = None::<String>;
    let mut json = false;
    let mut all = false;
    for arg in argv {
        match arg.as_str() {
            "--json" => json = true,
            "--all" => all = true,
            value if value.starts_with('-') => {
                return Err(format!("inbox: unknown argument {value}"))
            }
            value => {
                inbox_validate_target_arg(value, "oracle")?;
                if oracle.replace(value.to_owned()).is_some() {
                    return Err("usage: maw inbox status [oracle-name] [--json] [--all]".to_owned());
                }
            }
        }
    }
    if all && oracle.is_some() {
        return Err("usage: maw inbox status [oracle-name] [--json] [--all]".to_owned());
    }
    Ok((oracle, json, all))
}

fn inbox_build_status(
    oracle: &str,
    inbox_dir: &std::path::Path,
    env: &InboxEnv,
    now_ms: u64,
) -> Result<InboxStatus, String> {
    let messages = inbox_load_messages(inbox_dir)?;
    let unread_messages = messages
        .iter()
        .filter(|message| !message.read)
        .collect::<Vec<_>>();
    let oldest_age = unread_messages
        .iter()
        .map(|message| inbox_age_seconds(message.timestamp_ms, now_ms))
        .max();
    let archive_age =
        inbox_latest_archive_mtime_ms(inbox_dir)?.map(|mtime| inbox_age_seconds(mtime, now_ms));
    let mut cursor = inbox_read_cursor(&env.state_dir);
    let previous = cursor.get(oracle);
    let delta = previous.map_or(0, |entry| {
        inbox_usize_delta(unread_messages.len(), entry.unread)
    });
    let mut reasons = Vec::<String>::new();
    inbox_push_status_reasons(
        &mut reasons,
        unread_messages.len(),
        oldest_age,
        archive_age,
        delta,
    );
    let status = InboxStatus {
        oracle: oracle.to_owned(),
        unread: unread_messages.len(),
        oldest_age_seconds: oldest_age,
        last_archive_age_seconds: archive_age,
        delta_since_last_check: delta,
        level: if reasons.is_empty() { "green" } else { "red" }.to_owned(),
        reasons,
    };
    cursor.insert(oracle.to_owned(), inbox_cursor_entry(&status, now_ms));
    inbox_write_cursor(&env.state_dir, &cursor)?;
    Ok(status)
}

fn inbox_usize_delta(current: usize, previous: usize) -> i64 {
    let current = i64::try_from(current).unwrap_or(i64::MAX);
    let previous = i64::try_from(previous).unwrap_or(i64::MAX);
    current.saturating_sub(previous)
}

fn inbox_push_status_reasons(
    reasons: &mut Vec<String>,
    unread: usize,
    oldest_age: Option<u64>,
    archive_age: Option<u64>,
    delta: i64,
) {
    if unread > INBOX_UNREAD_RED_THRESHOLD {
        reasons.push("unread>50".to_owned());
    }
    if oldest_age.is_some_and(|age| age > INBOX_OLDEST_RED_SECONDS) {
        reasons.push("oldest>4h".to_owned());
    }
    if archive_age.is_some_and(|age| age > INBOX_ARCHIVE_RED_SECONDS) {
        reasons.push("since_archive>8h".to_owned());
    } else if archive_age.is_none() && unread > 0 {
        reasons.push("no_archive".to_owned());
    }
    if delta > 0 {
        reasons.push("delta>0_no_archive_activity".to_owned());
    }
}

fn inbox_run_drain(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let options = inbox_parse_drain_args(argv)?;
    if options
        .oracle
        .as_ref()
        .is_some_and(|oracle| oracle != &env.oracle)
    {
        return Err("inbox: native drain currently supports local inbox only".to_owned());
    }
    let result = inbox_drain_local(&options, env, now_ms)?;
    if options.json {
        inbox_json_pretty(&result)
    } else {
        Ok(inbox_format_drain_result(&result))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct InboxDrainOptions {
    oracle: Option<String>,
    json: bool,
    dry_run: bool,
    max: usize,
    older_than_seconds: u64,
}

fn inbox_parse_drain_args(argv: &[String]) -> Result<InboxDrainOptions, String> {
    let mut options = InboxDrainOptions {
        oracle: None,
        json: false,
        dry_run: false,
        max: INBOX_SAFE_DRAIN_DEFAULT_MAX,
        older_than_seconds: INBOX_SAFE_DRAIN_DEFAULT_MIN_AGE_SECONDS,
    };
    let mut safe = false;
    let mut index = 0_usize;
    while index < argv.len() {
        inbox_parse_drain_arg(argv, &mut index, &mut options, &mut safe)?;
        index += 1;
    }
    if !safe {
        return Err("usage: maw inbox drain [oracle-name] --safe [--max N] [--older-than-hours H] [--json] [--dry-run]".to_owned());
    }
    Ok(options)
}

fn inbox_parse_drain_arg(
    argv: &[String],
    index: &mut usize,
    options: &mut InboxDrainOptions,
    safe: &mut bool,
) -> Result<(), String> {
    match argv[*index].as_str() {
        "--safe" => *safe = true,
        "--json" => options.json = true,
        "--dry-run" => options.dry_run = true,
        "--max" => {
            options.max = inbox_parse_usize(inbox_required_value(argv, *index, "--max")?, "--max")?;
            *index += 1;
        }
        "--older-than-hours" => {
            options.older_than_seconds = inbox_parse_hours_seconds(inbox_required_value(
                argv,
                *index,
                "--older-than-hours",
            )?)?;
            *index += 1;
        }
        value if value.starts_with("--max=") => {
            options.max = inbox_parse_usize(value.trim_start_matches("--max="), "--max")?;
        }
        value if value.starts_with("--older-than-hours=") => {
            options.older_than_seconds =
                inbox_parse_hours_seconds(value.trim_start_matches("--older-than-hours="))?;
        }
        value if value.starts_with('-') => return Err(format!("inbox: unknown argument {value}")),
        value => inbox_set_drain_oracle(options, value)?,
    }
    Ok(())
}

fn inbox_set_drain_oracle(options: &mut InboxDrainOptions, value: &str) -> Result<(), String> {
    inbox_validate_target_arg(value, "oracle")?;
    if options.oracle.replace(value.to_owned()).is_some() {
        return Err("usage: maw inbox drain [oracle-name] --safe [--max N] [--older-than-hours H] [--json] [--dry-run]".to_owned());
    }
    Ok(())
}

fn inbox_drain_local(
    options: &InboxDrainOptions,
    env: &InboxEnv,
    now_ms: u64,
) -> Result<InboxDrainResult, String> {
    let messages = inbox_load_messages(&env.inbox_dir)?;
    let mut candidates = inbox_drain_candidates(&messages, now_ms, options.older_than_seconds);
    candidates.sort_by_key(|(_, _, age)| *age);
    let selected = candidates.into_iter().take(options.max).collect::<Vec<_>>();
    let processed_dir = env
        .inbox_dir
        .join("processed")
        .join(inbox_archive_day(now_ms));
    let mut items = Vec::<InboxDrainItem>::new();
    for (message, reason, age) in selected {
        let destination = inbox_unique_archive_path(&processed_dir, &message.filename);
        if !options.dry_run {
            inbox_archive_message(&message.path, &destination, now_ms)?;
        }
        items.push(inbox_drain_item(
            &message,
            &reason,
            age,
            &destination,
            options.dry_run,
        ));
    }
    let matched = inbox_drain_candidates(&messages, now_ms, options.older_than_seconds).len();
    Ok(inbox_drain_result(
        env,
        options,
        matched,
        messages.len(),
        &processed_dir,
        items,
    ))
}

fn inbox_drain_candidates(
    messages: &[InboxMessage],
    now_ms: u64,
    min_age: u64,
) -> Vec<(InboxMessage, String, u64)> {
    messages
        .iter()
        .filter_map(|message| {
            let reason = inbox_safe_drain_reason(message)?;
            let age = inbox_age_seconds(message.timestamp_ms, now_ms);
            (age >= min_age).then(|| (message.clone(), reason, age))
        })
        .collect()
}

fn inbox_run_pending(env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let rows = inbox_load_pending_for_env(env, now_ms)?
        .into_iter()
        .filter(|message| message.status == "pending")
        .collect::<Vec<_>>();
    Ok(inbox_format_pending_list(&rows))
}

fn inbox_run_show_pending(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox show-pending <id>")?;
    let Some(message) = inbox_resolve_pending_for_env(env, id, now_ms)? else {
        return Err(format!("pending message not found: {id}"));
    };
    Ok(inbox_format_pending_detail(&message))
}

async fn inbox_run_approve(
    argv: &[String],
    env: &InboxEnv,
    sender: &mut impl InboxSender,
    now_ms: u64,
) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox approve <id>")?;
    let Some(mut message) = inbox_resolve_pending_for_env(env, id, now_ms)? else {
        return Err(format!("pending message not found: {id}"));
    };
    if message.status != "pending" {
        return Err(format!(
            "message {} is already {}",
            message.id, message.status
        ));
    }
    let original_status = message.status.clone();
    "approved".clone_into(&mut message.status);
    let state_pending_dir = inbox_state_pending_dir(env);
    inbox_write_pending(&state_pending_dir, &message)?;
    let query = message.query.as_deref().unwrap_or(&message.target);
    if let Err(error) = sender.inbox_send(query, &message.message, true).await {
        original_status.clone_into(&mut message.status);
        inbox_write_pending(&state_pending_dir, &message)?;
        return Err(error);
    }
    inbox_delete_pending(&state_pending_dir, &message.id)?;
    Ok(format!(
        "approved: {} ({} → {})\n",
        message.id, message.sender, message.target
    ))
}

fn inbox_run_reject(argv: &[String], env: &InboxEnv, now_ms: u64) -> Result<String, String> {
    let id = inbox_single_id_arg(argv, "usage: maw inbox reject <id>")?;
    let Some(mut message) = inbox_resolve_pending_for_env(env, id, now_ms)? else {
        return Err(format!("pending message not found: {id}"));
    };
    let state_pending_dir = inbox_state_pending_dir(env);
    if message.status != "rejected" {
        "rejected".clone_into(&mut message.status);
        inbox_write_pending(&state_pending_dir, &message)?;
    }
    inbox_delete_pending(&state_pending_dir, &message.id)?;
    Ok(format!(
        "rejected: {} ({} → {})\n",
        message.id, message.sender, message.target
    ))
}

fn inbox_load_messages(inbox_dir: &std::path::Path) -> Result<Vec<InboxMessage>, String> {
    let Ok(entries) = std::fs::read_dir(inbox_dir) else {
        return Ok(Vec::new());
    };
    let mut messages = Vec::<InboxMessage>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("md") || !path.is_file() {
            continue;
        }
        if let Some(message) = inbox_load_message(&path)? {
            messages.push(message);
        }
    }
    messages.sort_by_key(|message| std::cmp::Reverse(message.timestamp_ms));
    Ok(messages)
}

fn inbox_load_message(path: &std::path::Path) -> Result<Option<InboxMessage>, String> {
    let Ok(content) = std::fs::read_to_string(path) else {
        return Ok(None);
    };
    let filename = path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or_default()
        .to_owned();
    let id = filename.strip_suffix(".md").unwrap_or(&filename).to_owned();
    let (fields, body) = inbox_parse_frontmatter(&content);
    let timestamp_ms = inbox_message_timestamp_ms(&filename, path, fields.get("timestamp"))?;
    Ok(Some(InboxMessage {
        id,
        filename,
        path: path.to_path_buf(),
        from: fields
            .get("from")
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        to: fields
            .get("to")
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        timestamp_ms,
        read: fields.get("read").is_some_and(|value| value == "true"),
        body,
    }))
}

fn inbox_parse_frontmatter(content: &str) -> (BTreeMap<String, String>, String) {
    if !content.starts_with("---\n") {
        return (BTreeMap::new(), content.trim().to_owned());
    }
    let Some(end) = content[4..].find("\n---") else {
        return (BTreeMap::new(), content.trim().to_owned());
    };
    let end = end + 4;
    let mut fields = BTreeMap::<String, String>::new();
    for line in content[4..end].lines() {
        if let Some((key, value)) = line.split_once(':') {
            fields.insert(key.trim().to_owned(), value.trim().to_owned());
        }
    }
    let body = content[end + "\n---".len()..].trim().to_owned();
    (fields, body)
}

fn inbox_message_timestamp_ms(
    filename: &str,
    path: &std::path::Path,
    frontmatter: Option<&String>,
) -> Result<u64, String> {
    if let Some(ms) = frontmatter.and_then(|value| inbox_parse_iso_ms(value)) {
        return Ok(ms);
    }
    if let Some(ms) = inbox_parse_filename_ms(filename) {
        return Ok(ms);
    }
    let metadata = std::fs::metadata(path)
        .map_err(|error| format!("inbox: stat {}: {error}", path.display()))?;
    Ok(inbox_system_time_ms(
        metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
    ))
}

fn inbox_parse_iso_ms(value: &str) -> Option<u64> {
    let prefix = value.get(0..16)?;
    let year = prefix.get(0..4)?.parse::<i32>().ok()?;
    let month = prefix.get(5..7)?.parse::<u32>().ok()?;
    let day = prefix.get(8..10)?.parse::<u32>().ok()?;
    let hour = prefix.get(11..13)?.parse::<u32>().ok()?;
    let minute = prefix.get(14..16)?.parse::<u32>().ok()?;
    inbox_ymdhm_to_ms(year, month, day, hour, minute)
}

fn inbox_parse_filename_ms(filename: &str) -> Option<u64> {
    let head = filename.get(0..16)?;
    let normalized = head.replace('_', "T").replace('-', "");
    let year = normalized.get(0..4)?.parse::<i32>().ok()?;
    let month = normalized.get(4..6)?.parse::<u32>().ok()?;
    let day = normalized.get(6..8)?.parse::<u32>().ok()?;
    let hour = normalized.get(9..11)?.parse::<u32>().ok()?;
    let minute = normalized.get(11..13)?.parse::<u32>().ok()?;
    inbox_ymdhm_to_ms(year, month, day, hour, minute)
}

fn inbox_ymdhm_to_ms(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Option<u64> {
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = inbox_days_from_civil(year, month, day)?;
    let seconds = days * 86_400 + i64::from(hour) * 3600 + i64::from(minute) * 60;
    u64::try_from(seconds).ok().map(|value| value * 1000)
}

fn inbox_days_from_civil(year: i32, month: u32, day: u32) -> Option<i64> {
    let year = year - i32::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month_i = i32::try_from(month).ok()?;
    let day_i = i32::try_from(day).ok()?;
    let doy = (153 * (month_i + if month_i > 2 { -3 } else { 9 }) + 2) / 5 + day_i - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(i64::from(era) * 146_097 + i64::from(doe) - 719_468)
}

fn inbox_find_message(
    inbox_dir: &std::path::Path,
    id: &str,
) -> Result<Option<InboxMessage>, String> {
    let messages = inbox_load_messages(inbox_dir)?;
    Ok(inbox_pick_message(&messages, Some(id)).cloned())
}

fn inbox_pick_message<'a>(
    messages: &'a [InboxMessage],
    target: Option<&str>,
) -> Option<&'a InboxMessage> {
    let Some(target) = target else {
        return messages.first();
    };
    target
        .parse::<usize>()
        .ok()
        .and_then(|index| index.checked_sub(1).and_then(|idx| messages.get(idx)))
        .or_else(|| {
            messages
                .iter()
                .find(|message| message.id.to_lowercase().contains(&target.to_lowercase()))
        })
}

fn inbox_render_show(message: &InboxMessage) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "\n\u{001b}[36m{}\u{001b}[0m", message.filename);
    let _ = writeln!(out, "\u{001b}[90mfrom: {}\u{001b}[0m", message.from);
    let _ = writeln!(
        out,
        "\u{001b}[90mwhen: {}\u{001b}[0m\n",
        inbox_iso_label(message.timestamp_ms)
    );
    out.push_str(&message.body);
    out.push('\n');
    out
}

fn inbox_mark_frontmatter_read(content: &str, now_ms: u64) -> String {
    if !content.starts_with("---\n") {
        return content.to_owned();
    }
    let Some(end) = content[4..].find("\n---") else {
        return content.to_owned();
    };
    let end = end + 4;
    let mut frontmatter = content[..end + "\n---".len()].to_owned();
    if frontmatter.lines().any(|line| line.trim() == "read: false") {
        frontmatter = frontmatter.replace("read: false", "read: true");
    } else if !frontmatter.lines().any(|line| line.starts_with("read:")) {
        frontmatter = frontmatter.replace("\n---", "\nread: true\n---");
    }
    if !frontmatter.lines().any(|line| line.starts_with("readAt:")) {
        let replacement = format!("\nreadAt: {}\n---", inbox_iso_label(now_ms));
        frontmatter = frontmatter.replace("\n---", &replacement);
    }
    frontmatter + &content[end + "\n---".len()..]
}

fn inbox_write_file(
    inbox_dir: &std::path::Path,
    from: &str,
    to: &str,
    body: &str,
    now_ms: u64,
) -> Result<String, String> {
    inbox_validate_target_arg(from, "from")?;
    inbox_validate_target_arg(to, "to")?;
    std::fs::create_dir_all(inbox_dir)
        .map_err(|error| format!("inbox: create {}: {error}", inbox_dir.display()))?;
    let filename = inbox_filename(from, body, now_ms);
    let frontmatter = format!(
        "---\nfrom: {from}\nto: {to}\ntimestamp: {}\nread: false\n---\n\n{body}\n",
        inbox_iso_label(now_ms)
    );
    std::fs::write(inbox_dir.join(&filename), frontmatter)
        .map_err(|error| format!("inbox: write {filename}: {error}"))?;
    Ok(filename)
}

fn inbox_filename(from: &str, body: &str, now_ms: u64) -> String {
    let label = inbox_file_time_label(now_ms);
    let slug = inbox_slugify(body);
    format!("{label}_{from}_{slug}.md")
}

fn inbox_slugify(body: &str) -> String {
    let mut slug = String::new();
    for word in body.split_whitespace().take(5) {
        if !slug.is_empty() {
            slug.push('-');
        }
        for ch in word.to_lowercase().chars() {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                slug.push(ch);
            }
            if slug.len() >= 40 {
                break;
            }
        }
        if slug.len() >= 40 {
            break;
        }
    }
    if slug.is_empty() {
        "note".to_owned()
    } else {
        slug
    }
}

fn inbox_read_cursor(state_dir: &std::path::Path) -> InboxCursorStore {
    let path = state_dir.join("inbox-cursor.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn inbox_write_cursor(state_dir: &std::path::Path, store: &InboxCursorStore) -> Result<(), String> {
    std::fs::create_dir_all(state_dir)
        .map_err(|error| format!("inbox: create {}: {error}", state_dir.display()))?;
    let json = serde_json::to_string_pretty(store).map_err(|error| error.to_string())?;
    std::fs::write(state_dir.join("inbox-cursor.json"), format!("{json}\n"))
        .map_err(|error| format!("inbox: write cursor: {error}"))
}

fn inbox_cursor_entry(status: &InboxStatus, now_ms: u64) -> InboxCursorEntry {
    InboxCursorEntry {
        unread: status.unread,
        latest_archive_mtime_ms: None,
        checked_at: inbox_iso_label(now_ms),
    }
}

fn inbox_latest_archive_mtime_ms(inbox_dir: &std::path::Path) -> Result<Option<u64>, String> {
    let processed = inbox_dir.join("processed");
    let Ok(days) = std::fs::read_dir(processed) else {
        return Ok(None);
    };
    let mut latest = None::<u64>;
    for day in days.flatten().filter(|entry| entry.path().is_dir()) {
        inbox_scan_archive_day(&day.path(), &mut latest)?;
    }
    Ok(latest)
}

fn inbox_scan_archive_day(path: &std::path::Path, latest: &mut Option<u64>) -> Result<(), String> {
    let Ok(files) = std::fs::read_dir(path) else {
        return Ok(());
    };
    for file in files.flatten().filter(|entry| entry.path().is_file()) {
        let metadata = std::fs::metadata(file.path()).map_err(|error| error.to_string())?;
        let ms = inbox_system_time_ms(metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH));
        *latest = Some(latest.map_or(ms, |old| old.max(ms)));
    }
    Ok(())
}

fn inbox_safe_drain_reason(message: &InboxMessage) -> Option<String> {
    let line = message
        .body
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    if !line.starts_with('[') || !line.contains(']') || line.contains('?') {
        return None;
    }
    let lower = format!("{}\n{}", message.filename, line).to_lowercase();
    inbox_safe_reason_patterns()
        .into_iter()
        .find(|(_, needle)| lower.contains(needle))
        .map(|(reason, _)| reason.to_owned())
}

fn inbox_safe_reason_patterns() -> Vec<(&'static str, &'static str)> {
    vec![
        ("ci-green", "ci green confirmed"),
        ("local-ship", "local ship commit"),
        ("alpha-pushed", "alpha pushed"),
        ("coverage-pushed", "coverage batch pushed"),
        ("green-batch", "green batch"),
        ("verified", "verified"),
        ("next-slice-shipped", "shipped next slice"),
        ("delivery-confirm", "delivery confirm"),
        ("council", "no response needed"),
    ]
}

fn inbox_archive_message(
    source: &std::path::Path,
    destination: &std::path::Path,
    now_ms: u64,
) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|error| format!("inbox: create {}: {error}", parent.display()))?;
    }
    std::fs::rename(source, destination).map_err(|error| {
        format!(
            "inbox: archive {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    let _ = now_ms;
    Ok(())
}

fn inbox_unique_archive_path(
    processed_dir: &std::path::Path,
    filename: &str,
) -> std::path::PathBuf {
    let stem = filename.strip_suffix(".md").unwrap_or(filename);
    let ext = if std::path::Path::new(filename)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
    {
        ".md"
    } else {
        ""
    };
    let mut candidate = processed_dir.join(filename);
    let mut suffix = 2_usize;
    while candidate.exists() {
        candidate = processed_dir.join(format!("{stem}-{suffix}{ext}"));
        suffix += 1;
    }
    candidate
}

fn inbox_drain_item(
    message: &InboxMessage,
    reason: &str,
    age: u64,
    destination: &std::path::Path,
    dry_run: bool,
) -> InboxDrainItem {
    InboxDrainItem {
        id: message.id.clone(),
        filename: message.filename.clone(),
        reason: reason.to_owned(),
        age_seconds: age,
        destination: Some(destination.display().to_string()),
        action: if dry_run { "would_archive" } else { "archived" }.to_owned(),
    }
}

fn inbox_drain_result(
    env: &InboxEnv,
    options: &InboxDrainOptions,
    matched: usize,
    scanned: usize,
    processed_dir: &std::path::Path,
    items: Vec<InboxDrainItem>,
) -> InboxDrainResult {
    InboxDrainResult {
        oracle: options.oracle.clone().unwrap_or_else(|| env.oracle.clone()),
        scanned,
        matched,
        archived: items.len(),
        remaining_matches: matched.saturating_sub(items.len()),
        max: options.max,
        dry_run: options.dry_run,
        safe: true,
        older_than_seconds: options.older_than_seconds,
        processed_dir: processed_dir.display().to_string(),
        items,
    }
}

fn inbox_format_drain_result(result: &InboxDrainResult) -> String {
    let verb = if result.dry_run {
        "would archive"
    } else {
        "archived"
    };
    let mut lines = vec![format!(
        "{}: {verb} {}/{} safe stale inbox message(s) (scanned {}, max {})",
        result.oracle, result.archived, result.matched, result.scanned, result.max
    )];
    if result.remaining_matches > 0 {
        lines.push(format!(
            "   → {} safe match(es) remain after max cap",
            result.remaining_matches
        ));
    }
    if result.items.is_empty() {
        lines.push("   → no messages matched the safe stale-ack filter".to_owned());
    }
    for item in result.items.iter().take(10) {
        lines.push(format!(
            "   - {} [{}, {}]",
            item.filename,
            item.reason,
            inbox_format_duration(Some(item.age_seconds))
        ));
    }
    lines.push(format!(
        "   → {}: {}",
        if result.dry_run {
            "preview"
        } else {
            "processed"
        },
        result.processed_dir
    ));
    format!("{}\n", lines.join("\n"))
}

fn inbox_load_pending_for_env(env: &InboxEnv, now_ms: u64) -> Result<Vec<InboxPendingMessage>, String> {
    let state_dir = inbox_state_pending_dir(env);
    inbox_reap_expired_pending(&state_dir, now_ms)?;
    let mut by_id = BTreeMap::<String, InboxPendingMessage>::new();
    for message in inbox_load_pending(&env.pending_dir, now_ms, false)? {
        by_id.entry(message.id.clone()).or_insert(message);
    }
    for message in inbox_load_pending(&state_dir, now_ms, true)? {
        by_id.insert(message.id.clone(), message);
    }
    let mut rows = by_id.into_values().collect::<Vec<_>>();
    rows.sort_by(|left, right| left.sent_at.cmp(&right.sent_at).then_with(|| left.id.cmp(&right.id)));
    Ok(rows)
}

fn inbox_load_pending(
    pending_dir: &std::path::Path,
    now_ms: u64,
    state_owned: bool,
) -> Result<Vec<InboxPendingMessage>, String> {
    let Ok(entries) = std::fs::read_dir(pending_dir) else {
        return Ok(Vec::new());
    };
    let mut rows = Vec::<InboxPendingMessage>::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let raw = std::fs::read_to_string(&path)
            .map_err(|error| format!("inbox: read pending {}: {error}", path.display()))?;
        if let Ok(message) = serde_json::from_str::<InboxPendingMessage>(&raw) {
            if inbox_pending_is_expired(&message, now_ms) {
                if state_owned {
                    let _ = std::fs::remove_file(&path);
                }
            } else if inbox_validate_pending_message(&message).is_ok() {
                rows.push(message);
            }
        }
    }
    rows.sort_by(|left, right| left.sent_at.cmp(&right.sent_at));
    Ok(rows)
}

fn inbox_resolve_pending_for_env(
    env: &InboxEnv,
    id: &str,
    now_ms: u64,
) -> Result<Option<InboxPendingMessage>, String> {
    inbox_validate_lookup_arg(id, "pending id")?;
    let rows = inbox_load_pending_for_env(env, now_ms)?;
    if let Some(exact) = rows.iter().find(|message| message.id == id) {
        return Ok(Some(exact.clone()));
    }
    let matches = rows
        .into_iter()
        .filter(|message| message.id.starts_with(id))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some(one.clone())),
        _ => Err(format!("pending id prefix is ambiguous: {id}")),
    }
}

fn inbox_write_pending(
    pending_dir: &std::path::Path,
    message: &InboxPendingMessage,
) -> Result<(), String> {
    inbox_validate_pending_message(message)?;
    std::fs::create_dir_all(pending_dir)
        .map_err(|error| format!("inbox: create pending dir: {error}"))?;
    let json = serde_json::to_string_pretty(message).map_err(|error| error.to_string())?;
    let path = pending_dir.join(format!("{}.json", message.id));
    inbox_write_0600_atomic(&path, &(json + "\n"))
        .map_err(|error| format!("inbox: write pending {}: {error}", message.id))?;
    let roundtrip = std::fs::read_to_string(&path)
        .map_err(|error| format!("inbox: validate pending {}: {error}", message.id))?;
    let parsed = serde_json::from_str::<InboxPendingMessage>(&roundtrip)
        .map_err(|error| format!("inbox: validate pending json {}: {error}", message.id))?;
    if parsed != *message {
        return Err(format!("inbox: validate pending mismatch {}", message.id));
    }
    Ok(())
}

fn inbox_delete_pending(pending_dir: &std::path::Path, id: &str) -> Result<(), String> {
    let path = pending_dir.join(format!("{id}.json"));
    if path.exists() {
        std::fs::remove_file(&path)
            .map_err(|error| format!("inbox: delete pending {}: {error}", path.display()))?;
    }
    Ok(())
}

fn inbox_reap_expired_pending(pending_dir: &std::path::Path, now_ms: u64) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(pending_dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(std::ffi::OsStr::to_str) != Some("json") {
            continue;
        }
        let Ok(raw) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(message) = serde_json::from_str::<InboxPendingMessage>(&raw) else {
            continue;
        };
        if inbox_pending_is_expired(&message, now_ms) {
            std::fs::remove_file(&path)
                .map_err(|error| format!("inbox: reap expired pending {}: {error}", path.display()))?;
        }
    }
    Ok(())
}

fn inbox_pending_is_expired(message: &InboxPendingMessage, now_ms: u64) -> bool {
    inbox_parse_iso_ms(&message.sent_at)
        .is_some_and(|sent_ms| inbox_age_seconds(sent_ms, now_ms) > INBOX_PENDING_TTL_SECONDS)
}

fn inbox_validate_pending_message(message: &InboxPendingMessage) -> Result<(), String> {
    inbox_validate_lookup_arg(&message.id, "pending id")?;
    inbox_validate_target_arg(&message.sender, "sender")?;
    inbox_validate_target_arg(&message.target, "target")?;
    if let Some(query) = &message.query {
        inbox_validate_target_arg(query, "query")?;
    }
    if !matches!(message.status.as_str(), "pending" | "approved" | "rejected") {
        return Err("inbox: invalid pending status".to_owned());
    }
    if message.sent_at.is_empty() || message.sent_at.chars().any(char::is_control) {
        return Err("inbox: invalid pending sentAt".to_owned());
    }
    Ok(())
}

fn inbox_write_0600_atomic(path: &std::path::Path, body: &str) -> Result<(), String> {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| format!("create parent failed: {error}"))?;
    let tmp = inbox_tmp_path(path);
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        }
        let mut file = options.open(&tmp).map_err(|error| format!("tmp create failed: {error}"))?;
        std::io::Write::write_all(&mut file, body.as_bytes())
            .map_err(|error| format!("tmp write failed: {error}"))?;
        file.sync_all().map_err(|error| format!("tmp sync failed: {error}"))?;
    }
    std::fs::read_to_string(&tmp).map_err(|error| format!("tmp validate read failed: {error}"))?;
    std::fs::rename(&tmp, path).map_err(|error| format!("atomic rename failed: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("chmod 0600 failed: {error}"))?;
    }
    Ok(())
}

fn inbox_tmp_path(path: &std::path::Path) -> std::path::PathBuf {
    let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let name = path.file_name().and_then(std::ffi::OsStr::to_str).unwrap_or("pending.json");
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(".{name}.{}-{nanos}.tmp", std::process::id()))
}

#[allow(dead_code)]
fn inbox_pending_id(now_ms: u64, random_hex: &str) -> Result<String, String> {
    if random_hex.len() != 6 || !random_hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return Err("inbox: pending id random suffix must be 6 hex chars".to_owned());
    }
    Ok(format!(
        "{}-{}",
        inbox_iso_label(now_ms).replace([':', '.'], "-"),
        random_hex.to_ascii_lowercase()
    ))
}

fn inbox_format_pending_list(rows: &[InboxPendingMessage]) -> String {
    if rows.is_empty() {
        return "no pending messages\n".to_owned();
    }
    let mut out = String::from("id  sender  target  sentAt  preview\n");
    out.push_str("--  ------  ------  ------  -------\n");
    for row in rows {
        let preview = inbox_pending_preview(&row.message);
        let _ = writeln!(
            out,
            "{}  {}  {}  {}  {preview}",
            row.id, row.sender, row.target, row.sent_at
        );
    }
    out
}

fn inbox_pending_preview(message: &str) -> String {
    let flattened = message.replace('\n', " ");
    let lower = flattened.to_ascii_lowercase();
    if lower.contains("token") || lower.contains("secret") || lower.contains("peer-key") {
        return "[redacted sensitive preview]".to_owned();
    }
    inbox_truncate(&flattened, 50)
}

fn inbox_format_pending_detail(message: &InboxPendingMessage) -> String {
    format!(
        "id:      {}\nsender:  {}\ntarget:  {}\nquery:   {}\nsentAt:  {}\nstatus:  {}\nmessage:\n{}\n",
        message.id,
        message.sender,
        message.target,
        message.query.as_deref().unwrap_or("-"),
        message.sent_at,
        message.status,
        message.message
    )
}

fn inbox_render_status(status: &InboxStatus, json: bool) -> Result<String, String> {
    if json {
        return inbox_json_pretty(status);
    }
    let symbol = if status.level == "red" {
        "🔴"
    } else {
        "🟢"
    };
    let oldest = status
        .oldest_age_seconds
        .map_or("none".to_owned(), |age| inbox_format_duration(Some(age)));
    let archive = status
        .last_archive_age_seconds
        .map_or("never".to_owned(), |age| {
            format!("{} ago", inbox_format_duration(Some(age)))
        });
    let mut line = format!(
        "{symbol} UNREAD {} (oldest {oldest}, last archive {archive}, Δ {} last cycle)\n",
        status.unread,
        inbox_format_delta(status.delta_since_last_check)
    );
    if status.level == "red" {
        line.push_str("   → not draining — consider escalation\n");
    }
    Ok(line)
}

fn inbox_render_status_list(statuses: &[InboxStatus], json: bool) -> Result<String, String> {
    if json {
        return inbox_json_pretty(statuses);
    }
    if statuses.is_empty() {
        return Ok("no local fleet inboxes found\n".to_owned());
    }
    let mut out = String::new();
    for status in statuses {
        let symbol = if status.level == "red" {
            "🔴"
        } else {
            "🟢"
        };
        let oldest = status
            .oldest_age_seconds
            .map_or("none".to_owned(), |age| inbox_format_duration(Some(age)));
        let reasons = if status.reasons.is_empty() {
            String::new()
        } else {
            format!(" [{}]", status.reasons.join(","))
        };
        let _ = writeln!(
            out,
            "{symbol} {}: unread {} (oldest {oldest}){reasons}",
            status.oracle, status.unread
        );
    }
    Ok(out)
}

fn inbox_json_pretty<T: serde::Serialize + ?Sized>(value: &T) -> Result<String, String> {
    serde_json::to_string_pretty(value)
        .map(|mut json| {
            json.push('\n');
            json
        })
        .map_err(|error| error.to_string())
}

fn inbox_required_value<'a>(
    argv: &'a [String],
    index: usize,
    flag: &str,
) -> Result<&'a str, String> {
    let Some(value) = argv.get(index + 1) else {
        return Err(format!("inbox: missing {flag} value"));
    };
    if value.starts_with('-') {
        return Err(format!("inbox: {flag} value must not start with '-'"));
    }
    Ok(value)
}

fn inbox_single_id_arg<'a>(argv: &'a [String], usage: &str) -> Result<&'a str, String> {
    if argv.len() != 1 {
        return Err(usage.to_owned());
    }
    inbox_validate_lookup_arg(&argv[0], "id")?;
    Ok(&argv[0])
}

fn inbox_validate_lookup_arg(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.starts_with('-') || value.contains('/') || value.contains("..") {
        return Err(format!("inbox: invalid {label}"));
    }
    if value
        .bytes()
        .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("inbox: invalid {label}"));
    }
    Ok(())
}

fn inbox_validate_target_arg(value: &str, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.trim() != value || value.starts_with('-') {
        return Err(format!("inbox: invalid {label}"));
    }
    if value.contains('/')
        || value
            .bytes()
            .any(|byte| byte == 0 || byte.is_ascii_control())
    {
        return Err(format!("inbox: invalid {label}"));
    }
    Ok(())
}

fn inbox_parse_usize(value: &str, flag: &str) -> Result<usize, String> {
    if value.is_empty() || value.starts_with('-') {
        return Err(format!("{flag} must be a non-negative integer"));
    }
    value
        .parse::<usize>()
        .map_err(|_| format!("{flag} must be a non-negative integer"))
}

fn inbox_parse_hours_seconds(value: &str) -> Result<u64, String> {
    if value.is_empty() || value.starts_with('-') {
        return Err("--older-than-hours must be a non-negative number".to_owned());
    }
    let (whole, frac) = value.split_once('.').unwrap_or((value, ""));
    let hours = whole
        .parse::<u64>()
        .map_err(|_| "--older-than-hours must be a non-negative number".to_owned())?;
    let mut seconds = hours
        .checked_mul(3600)
        .ok_or_else(|| "--older-than-hours is too large".to_owned())?;
    if !frac.is_empty() {
        if !frac.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err("--older-than-hours must be a non-negative number".to_owned());
        }
        let scale = 10_u64.pow(u32::try_from(frac.len().min(6)).unwrap_or(0));
        let trimmed = &frac[..frac.len().min(6)];
        let fraction = trimmed
            .parse::<u64>()
            .map_err(|_| "--older-than-hours must be a non-negative number".to_owned())?;
        seconds += fraction.saturating_mul(3600) / scale;
    }
    Ok(seconds)
}

fn inbox_now_ms() -> u64 {
    inbox_system_time_ms(SystemTime::now())
}

fn inbox_system_time_ms(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH).map_or(0, |duration| {
        u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
    })
}

fn inbox_age_seconds(timestamp_ms: u64, now_ms: u64) -> u64 {
    now_ms.saturating_sub(timestamp_ms) / 1000
}

fn inbox_relative_time(timestamp_ms: u64, now_ms: u64) -> String {
    if timestamp_ms == 0 {
        return "—".to_owned();
    }
    if timestamp_ms > now_ms {
        return "future".to_owned();
    }
    let mins = inbox_age_seconds(timestamp_ms, now_ms) / 60;
    if mins < 1 {
        "just now".to_owned()
    } else if mins < 60 {
        format!("{mins}m ago")
    } else if mins < 24 * 60 {
        format!("{}h ago", mins / 60)
    } else {
        format!("{}d ago", mins / (24 * 60))
    }
}

fn inbox_format_duration(seconds: Option<u64>) -> String {
    let Some(seconds) = seconds else {
        return "never".to_owned();
    };
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 48 * 3600 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86_400)
    }
}

fn inbox_format_delta(delta: i64) -> String {
    if delta > 0 {
        format!("+{delta}")
    } else {
        delta.to_string()
    }
}

fn inbox_archive_day(now_ms: u64) -> String {
    inbox_iso_label(now_ms)
        .get(0..10)
        .unwrap_or("1970-01-01")
        .to_owned()
}

fn inbox_iso_label(ms: u64) -> String {
    let seconds = ms / 1000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(0);
    let secs_of_day = seconds % 86_400;
    let (year, month, day) = inbox_civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:00.000Z",
        secs_of_day / 3600,
        (secs_of_day % 3600) / 60
    )
}

fn inbox_file_time_label(ms: u64) -> String {
    let iso = inbox_iso_label(ms);
    format!("{}_{}", &iso[0..10], &iso[11..16].replace(':', "-"))
}

fn inbox_civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    (
        i32::try_from(y + i64::from(m <= 2)).unwrap_or(1970),
        u32::try_from(m).unwrap_or(1),
        u32::try_from(d).unwrap_or(1),
    )
}

fn inbox_pad(value: &str, width: usize) -> String {
    let mut out = value.to_owned();
    while out.chars().count() < width {
        out.push(' ');
    }
    out
}

fn inbox_truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

#[cfg(test)]
mod inbox_tests {
    use super::*;

    #[derive(Default)]
    struct InboxFakeSender {
        sent: Vec<(String, String, bool)>,
        fail: bool,
    }

    impl InboxSender for InboxFakeSender {
        fn inbox_send<'a>(
            &'a mut self,
            query: &'a str,
            message: &'a str,
            acl_bypass: bool,
        ) -> Pin<Box<dyn Future<Output = Result<(), String>> + Send + 'a>> {
            Box::pin(async move {
                inbox_validate_target_arg(query, "query")?;
                if std::env::var("MAW_ACL_BYPASS").is_ok() {
                    return Err("test leak: MAW_ACL_BYPASS should not be global".to_owned());
                }
                if self.fail {
                    return Err("fake send failed".to_owned());
                }
                self.sent
                    .push((query.to_owned(), message.to_owned(), acl_bypass));
                Ok(())
            })
        }
    }

    fn inbox_run_test(
        argv: &[String],
        env: &InboxEnv,
        sender: &mut impl InboxSender,
    ) -> Result<String, String> {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime")
            .block_on(inbox_run(argv, env, sender))
    }

    fn inbox_temp_env(name: &str) -> InboxEnv {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        let root = std::env::temp_dir().join(format!(
            "maw-inbox-test-{name}-{}-{nanos}",
            std::process::id()
        ));
        InboxEnv {
            inbox_dir: root.join("psi").join("inbox"),
            pending_dir: root.join("config").join("pending"),
            state_dir: root.join("state"),
            oracle: "nova".to_owned(),
            node: "cli".to_owned(),
        }
    }

    fn inbox_write_fixture(env: &InboxEnv, filename: &str, from: &str, read: bool, body: &str) {
        inbox_write_fixture_at(
            env,
            filename,
            from,
            read,
            "2026-06-25T00:00:00.000Z",
            body,
        );
    }

    #[test]
    fn inbox_help_prints_usage_to_stdout_path() {
        let env = inbox_temp_env("help");
        let mut sender = InboxFakeSender::default();

        let output = inbox_run_test(&inbox_strings(&["--help"]), &env, &mut sender).unwrap();

        assert_eq!(output, format!("usage: {INBOX_USAGE}\n"));
    }

    fn inbox_write_fixture_at(
        env: &InboxEnv,
        filename: &str,
        from: &str,
        read: bool,
        timestamp: &str,
        body: &str,
    ) {
        std::fs::create_dir_all(&env.inbox_dir).unwrap();
        let text = format!(
            "---\nfrom: {from}\nto: nova\ntimestamp: {timestamp}\nread: {read}\n---\n\n{body}\n"
        );
        std::fs::write(env.inbox_dir.join(filename), text).unwrap();
    }

    fn inbox_pending_fixture(env: &InboxEnv, id: &str, status: &str) {
        inbox_pending_fixture_with_message(env, id, status, "hello fleet");
    }

    fn inbox_pending_fixture_with_message(env: &InboxEnv, id: &str, status: &str, body: &str) {
        let message = InboxPendingMessage {
            id: id.to_owned(),
            sender: "alice".to_owned(),
            target: "bob".to_owned(),
            query: Some("bob".to_owned()),
            sent_at: "2026-06-25T00:00:00.000Z".to_owned(),
            status: status.to_owned(),
            message: body.to_owned(),
        };
        inbox_write_pending(&inbox_state_pending_dir(env), &message).unwrap();
    }

    fn inbox_strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn inbox_list_show_read_and_write_are_hermetic() {
        let env = inbox_temp_env("list");
        inbox_write_fixture(
            &env,
            "2026-06-25_00-00_alice_ci.md",
            "alice",
            false,
            "[alice] ci green confirmed",
        );
        let mut sender = InboxFakeSender::default();
        let list = inbox_run_test(
            &inbox_strings(&["--unread", "--from", "alice", "--last", "1"]),
            &env,
            &mut sender,
        )
        .unwrap();
        assert!(list.contains("INBOX"));
        assert!(list.contains("alice"));
        let show = inbox_run_test(&inbox_strings(&["show", "ci"]), &env, &mut sender).unwrap();
        assert!(show.contains("ci green confirmed"));
        let read = inbox_run_test(&inbox_strings(&["read", "ci"]), &env, &mut sender).unwrap();
        assert!(read.contains("marked read"));
        let write =
            inbox_run_test(&inbox_strings(&["write", "new", "note"]), &env, &mut sender).unwrap();
        assert!(write.contains("wrote"));
    }

    #[test]
    fn inbox_list_prints_stable_row_ids() {
        let env = inbox_temp_env("list-ids");
        inbox_write_fixture_at(
            &env,
            "2026-06-25_00-00_alice_old.md",
            "alice",
            false,
            "2026-06-25T00:00:00.000Z",
            "older message",
        );
        inbox_write_fixture_at(
            &env,
            "2026-06-26_00-00_bob_new.md",
            "bob",
            true,
            "2026-06-26T00:00:00.000Z",
            "newer message",
        );
        let mut sender = InboxFakeSender::default();

        let list = inbox_run_test(&inbox_strings(&["list"]), &env, &mut sender).unwrap();

        assert!(list.contains("ID R FROM"), "{list}");
        assert!(list.contains("  1 \u{001b}[90m○\u{001b}[0m bob"), "{list}");
        assert!(list.contains("  2 \u{001b}[32m●\u{001b}[0m alice"), "{list}");
    }

    #[test]
    fn inbox_read_row_id_prints_message_and_marks_read() {
        let env = inbox_temp_env("read-row");
        inbox_write_fixture_at(
            &env,
            "2026-06-25_00-00_alice_old.md",
            "alice",
            false,
            "2026-06-25T00:00:00.000Z",
            "older message",
        );
        inbox_write_fixture_at(
            &env,
            "2026-06-26_00-00_bob_new.md",
            "bob",
            false,
            "2026-06-26T00:00:00.000Z",
            "first line\nsecond line with full body",
        );
        let mut sender = InboxFakeSender::default();

        let read = inbox_run_test(&inbox_strings(&["read", "1"]), &env, &mut sender).unwrap();

        assert!(read.contains("2026-06-26_00-00_bob_new.md"), "{read}");
        assert!(read.contains("from: bob"), "{read}");
        assert!(read.contains("when: 2026-06-26T00:00:00.000Z"), "{read}");
        assert!(read.contains("first line\nsecond line with full body"), "{read}");
        assert!(read.contains("marked read: 2026-06-26_00-00_bob_new.md"), "{read}");
        let stored = std::fs::read_to_string(env.inbox_dir.join("2026-06-26_00-00_bob_new.md")).unwrap();
        assert!(stored.contains("read: true"), "{stored}");
        assert!(stored.contains("readAt:"), "{stored}");
        let old = std::fs::read_to_string(env.inbox_dir.join("2026-06-25_00-00_alice_old.md")).unwrap();
        assert!(old.contains("read: false"), "{old}");
    }

    #[test]
    fn inbox_drain_safe_dry_run_matches_golden_shape() {
        let env = inbox_temp_env("drain");
        inbox_write_fixture(
            &env,
            "2026-06-24_00-00_alice_ci.md",
            "alice",
            false,
            "[alice] ci green confirmed",
        );
        let mut sender = InboxFakeSender::default();
        let out = inbox_run_test(
            &inbox_strings(&["drain", "--safe", "--dry-run", "--older-than-hours", "0"]),
            &env,
            &mut sender,
        )
        .unwrap();
        assert!(out.contains("nova: would archive 1/1 safe stale inbox message"));
        assert!(out.contains("ci-green"));
        assert!(env.inbox_dir.join("2026-06-24_00-00_alice_ci.md").exists());
    }

    #[test]
    fn inbox_status_json_writes_temp_cursor_only() {
        let env = inbox_temp_env("status");
        inbox_write_fixture(
            &env,
            "2026-06-25_00-00_alice_ci.md",
            "alice",
            false,
            "hello",
        );
        let status = inbox_build_status("nova", &env.inbox_dir, &env, 1_766_620_800_000).unwrap();
        assert_eq!(status.unread, 1);
        assert!(env.state_dir.join("inbox-cursor.json").exists());
        let json = inbox_render_status(&status, true).unwrap();
        assert!(json.contains("\"oldest_age_seconds\""));
    }

    #[test]
    fn inbox_pending_acl_surfaces_match_committed_goldens() {
        let env = inbox_temp_env("pending-golden");
        inbox_pending_fixture(&env, "abc123", "pending");
        inbox_pending_fixture(&env, "def456", "pending");
        let mut sender = InboxFakeSender::default();

        let pending = inbox_run_test(&inbox_strings(&["pending"]), &env, &mut sender).unwrap();
        assert_eq!(pending, include_str!("../../tests/fixtures/native-scope-acl/inbox-pending-list.stdout"));

        let detail = inbox_run_test(&inbox_strings(&["show-pending", "abc"]), &env, &mut sender).unwrap();
        assert_eq!(detail, include_str!("../../tests/fixtures/native-scope-acl/inbox-show-pending.stdout"));

        let approved = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();
        assert_eq!(approved, include_str!("../../tests/fixtures/native-scope-acl/inbox-approve.stdout"));
        assert_eq!(sender.sent, vec![("bob".to_owned(), "hello fleet".to_owned(), true)]);

        let rejected = inbox_run_test(&inbox_strings(&["reject", "def"]), &env, &mut sender).unwrap();
        assert_eq!(rejected, include_str!("../../tests/fixtures/native-scope-acl/inbox-reject.stdout"));
    }

    #[test]
    fn inbox_pending_show_approve_reject_are_hermetic() {
        let env = inbox_temp_env("pending");
        inbox_pending_fixture(&env, "abc123", "pending");
        inbox_pending_fixture(&env, "def456", "pending");
        let mut sender = InboxFakeSender::default();
        let pending = inbox_run_test(&inbox_strings(&["pending"]), &env, &mut sender).unwrap();
        assert!(pending.contains("abc123"));
        let detail =
            inbox_run_test(&inbox_strings(&["show-pending", "abc"]), &env, &mut sender).unwrap();
        assert!(detail.contains("message:"));
        let approved = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();
        assert!(approved.contains("approved: abc123"));
        assert_eq!(
            sender.sent,
            vec![("bob".to_owned(), "hello fleet".to_owned(), true)]
        );
        assert!(std::env::var("MAW_ACL_BYPASS").is_err());
        assert!(!inbox_state_pending_dir(&env).join("abc123.json").exists());
        let rejected = inbox_run_test(&inbox_strings(&["reject", "def"]), &env, &mut sender).unwrap();
        assert!(rejected.contains("rejected: def456"));
        assert!(!inbox_state_pending_dir(&env).join("def456.json").exists());
    }

    #[test]
    fn inbox_approve_sends_flag_like_messages_as_opaque_text() {
        let cases = [
            ("approve", "hello --approve"),
            ("from", "hello --from=mallory:edge"),
            ("trust", "hello --trust"),
            ("leading", "-leading payload"),
        ];

        for (name, body) in cases {
            let env = inbox_temp_env(name);
            inbox_pending_fixture_with_message(&env, "abc123", "pending", body);
            let mut sender = InboxFakeSender::default();

            let approved =
                inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).unwrap();

            assert!(approved.contains("approved: abc123"));
            assert_eq!(sender.sent, vec![("bob".to_owned(), body.to_owned(), true)]);
            assert!(std::env::var("MAW_ACL_BYPASS").is_err());
        }
    }

    #[test]
    fn inbox_pending_state_first_legacy_fallback_ttl_and_preview_only() {
        let env = inbox_temp_env("pending-state");
        let legacy = InboxPendingMessage {
            id: "same123".to_owned(),
            sender: "legacy".to_owned(),
            target: "bob".to_owned(),
            query: Some("bob".to_owned()),
            sent_at: "2026-06-25T00:00:00.000Z".to_owned(),
            status: "pending".to_owned(),
            message: "legacy full token SECRET_BODY".to_owned(),
        };
        inbox_write_pending(&env.pending_dir, &legacy).unwrap();
        let state = InboxPendingMessage {
            sender: "state".to_owned(),
            message: "state full token SECRET_BODY".to_owned(),
            ..legacy.clone()
        };
        inbox_write_pending(&inbox_state_pending_dir(&env), &state).unwrap();
        let expired = InboxPendingMessage {
            id: "old999".to_owned(),
            sent_at: "2026-05-01T00:00:00.000Z".to_owned(),
            ..state.clone()
        };
        inbox_write_pending(&inbox_state_pending_dir(&env), &expired).unwrap();

        let rows = inbox_load_pending_for_env(&env, inbox_parse_iso_ms("2026-06-26T00:00:00.000Z").unwrap()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sender, "state");
        assert!(!inbox_state_pending_dir(&env).join("old999.json").exists());

        let mut sender = InboxFakeSender::default();
        let list = inbox_run_test(&inbox_strings(&["queue"]), &env, &mut sender).unwrap();
        assert!(list.contains("same123"));
        assert!(list.contains("state"));
        assert!(!list.contains("SECRET_BODY"));
        let detail = inbox_run_test(&inbox_strings(&["show-pending", "same"]), &env, &mut sender).unwrap();
        assert!(detail.contains("SECRET_BODY"));
    }

    #[test]
    fn inbox_pending_approve_send_failure_keeps_file_for_retry() {
        let env = inbox_temp_env("pending-fail");
        inbox_pending_fixture(&env, "abc123", "pending");
        let mut sender = InboxFakeSender {
            fail: true,
            ..InboxFakeSender::default()
        };
        let err = inbox_run_test(&inbox_strings(&["approve", "abc"]), &env, &mut sender).expect_err("send failure");
        assert!(err.contains("fake send failed"));
        let path = inbox_state_pending_dir(&env).join("abc123.json");
        assert!(path.exists());
        let pending = inbox_load_pending_for_env(&env, inbox_now_ms()).unwrap();
        assert_eq!(pending[0].status, "pending");
    }

    #[test]
    fn inbox_pending_id_and_atomic_permissions_are_guarded() {
        let env = inbox_temp_env("pending-perms");
        inbox_pending_fixture(&env, "abc123", "pending");
        assert_eq!(
            inbox_pending_id(inbox_parse_iso_ms("2026-06-26T00:00:00.000Z").unwrap(), "A1B2c3").unwrap(),
            "2026-06-26T00-00-00-000Z-a1b2c3"
        );
        assert!(inbox_pending_id(0, "nope").is_err());
        let path = inbox_state_pending_dir(&env).join("abc123.json");
        assert!(path.exists());
        let siblings = std::fs::read_dir(inbox_state_pending_dir(&env))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert!(!siblings
            .iter()
            .any(|name| std::path::Path::new(name).extension().is_some_and(|ext| ext == "tmp")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn inbox_guards_reject_leading_dash_and_paths() {
        let env = inbox_temp_env("guards");
        let mut sender = InboxFakeSender::default();
        assert!(inbox_run_test(&inbox_strings(&["--from", "-bad"]), &env, &mut sender).is_err());
        assert!(inbox_run_test(&inbox_strings(&["read", "../secret"]), &env, &mut sender).is_err());
        assert!(inbox_run_test(&inbox_strings(&["write", "-bad"]), &env, &mut sender).is_err());
        assert!(inbox_run_test(&inbox_strings(&["write", "--", "-ok"]), &env, &mut sender).is_ok());
    }

    #[test]
    fn inbox_dispatch_is_native() {
        assert_eq!(DISPATCH_62.len(), 1);
        assert_eq!(DISPATCH_62[0].command, "inbox");
    }

    #[test]
    fn inbox_path_has_no_self_spawn_or_acl_env_channel() {
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let part62 = std::fs::read_to_string(manifest_dir.join("src/core_impl/inbox.rs"))
            .expect("read part62");
        let part62_prod = part62
            .split_once("#[cfg(test)]")
            .map_or(part62.as_str(), |(prod, _tests)| prod);
        assert!(!part62_prod.contains("Command::new"));
        assert!(!part62_prod.contains("current_exe"));
        assert!(!part62_prod.contains("MAW_ACL_BYPASS"));

        let part29 = std::fs::read_to_string(manifest_dir.join("src/core_impl/send_federation.rs"))
            .expect("read part29");
        let part29_prod = part29
            .split_once("#[cfg(test)]")
            .map_or(part29.as_str(), |(prod, _tests)| prod);
        assert!(!part29_prod.contains("std::env::var(\"MAW_ACL_BYPASS\")"));
        assert!(!part29_prod.contains("std::env::var_os(\"MAW_ACL_BYPASS\")"));
    }
}
