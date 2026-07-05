fn servecore_validate_engine_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if value
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace() || ch == '\0')
    {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    Ok(())
}

fn servecore_validate_command_token(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.trim() != value || value.starts_with('-') || value == "--" {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
    {
        return Err(format!("serve-orchestration {label} must be safe"));
    }
    Ok(())
}

fn servecore_validate_prompt_text(value: &str) -> Result<(), String> {
    if value.is_empty() || value.chars().any(|ch| ch.is_control() || ch == '\0') {
        return Err("serve-orchestration prompt must be safe".to_owned());
    }
    Ok(())
}

#[derive(Clone)]
pub struct ServecoreThreadStore {
    root: Arc<PathBuf>,
    lock: Arc<Mutex<()>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreThreadRecord {
    pub thread: ServecoreThreadInfo,
    pub messages: Vec<ServecoreThreadMessage>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreThreadInfo {
    pub id: u64,
    pub title: String,
    pub status: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServecoreThreadMessage {
    pub id: u64,
    pub role: String,
    pub content: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ServecoreThreadPostResult {
    pub thread_id: u64,
    pub message_id: u64,
    pub status: String,
}

const SERVECORE_THREAD_MAX_PARTICIPANTS: usize = 32;
const SERVECORE_THREAD_MAX_TEXT_BYTES: usize = 64 * 1024;
const SERVECORE_THREAD_MAX_THREADS: usize = 10_000;
const SERVECORE_THREAD_FILE_BYTES: u64 = 8 * 1024 * 1024;

impl Default for ServecoreThreadStore {
    fn default() -> Self {
        Self::servecore_default()
    }
}

fn servecore_ed25519_tofu_path() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    let vars = [
        "MAW_HOME",
        "MAW_DATA_DIR",
        "MAW_XDG",
        "XDG_DATA_HOME",
        "XDG_STATE_HOME",
    ]
    .into_iter()
    .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_owned(), value)));
    let env = maw_xdg::MawXdgEnv::with_vars(home, vars);
    maw_xdg::maw_data_path(&env, &["auth", "ed25519-tofu-pins.json"])
}

