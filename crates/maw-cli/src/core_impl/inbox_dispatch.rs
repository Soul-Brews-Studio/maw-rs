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
    if argv
        .first()
        .is_some_and(|arg| matches!(arg.as_str(), "--help" | "-h"))
    {
        return Ok(format!("usage: {INBOX_USAGE}\n"));
    }
    match argv.first().map(String::as_str) {
        Some("pending" | "queue") => inbox_run_pending(env, inbox_now_ms()),
        Some("show-pending" | "pending-show") => inbox_run_show_pending(&argv[1..], env, inbox_now_ms()),
        Some("approve") => inbox_run_approve(&argv[1..], env, sender, inbox_now_ms()).await,
        Some("reject") => inbox_run_reject(&argv[1..], env, inbox_now_ms()),
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
    let config = inbox_read_config(&config_dir.join("maw.config.json"));
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

fn inbox_read_config(path: &std::path::Path) -> serde_json::Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(serde_json::Value::Null)
}

fn inbox_config_string(config: &serde_json::Value, key: &str, fallback: &str) -> String {
    config
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or(fallback)
        .to_owned()
}

