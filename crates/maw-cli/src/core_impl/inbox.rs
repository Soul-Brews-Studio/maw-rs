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
    inbox_run_at(argv, env, sender, inbox_now_ms()).await
}

async fn inbox_run_at(
    argv: &[String],
    env: &InboxEnv,
    sender: &mut impl InboxSender,
    now_ms: u64,
) -> Result<String, String> {
    if wants_help_before_positionals(argv, &["--from", "--last"]) {
        return Ok(format!("usage: {INBOX_USAGE}\n"));
    }
    match argv.first().map(String::as_str) {
        Some("pending" | "queue") => inbox_run_pending(env, now_ms),
        Some("show-pending" | "pending-show") => inbox_run_show_pending(&argv[1..], env, now_ms),
        Some("approve") => inbox_run_approve(&argv[1..], env, sender, now_ms).await,
        Some("reject") => inbox_run_reject(&argv[1..], env, now_ms),
        Some("list" | "ls") => inbox_run_list(&argv[1..], env, now_ms),
        Some("read") => inbox_run_mark_read(&argv[1..], env),
        Some("show") => inbox_run_show(&argv[1..], env),
        Some("write") => inbox_run_write(&argv[1..], env, now_ms),
        Some("status") => inbox_run_status(&argv[1..], env, now_ms),
        Some("drain") => inbox_run_drain(&argv[1..], env, now_ms),
        Some(value) if value.starts_with('-') => inbox_run_list(argv, env, now_ms),
        Some(value) => Err(format!("inbox: unknown subcommand {value}")),
        None => inbox_run_list(argv, env, now_ms),
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













