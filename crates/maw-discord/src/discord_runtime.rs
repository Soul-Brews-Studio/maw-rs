use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    env,
    future::Future,
    path::PathBuf,
    pin::Pin,
    process::Command,
    time::{Duration, SystemTime},
};

pub(super) const DISCORD_API_BASE: &str = "https://discord.com/api/v10";
pub(super) const VERSION: &str = "0.4.2";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscordOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DiscordHttpResponse {
    pub status: u16,
    pub body: Value,
    pub retry_after: Option<f64>,
}

pub trait DiscordRest: Send + Sync {
    fn get_json<'a>(
        &'a self,
        path: &'a str,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>>;

    fn post_json<'a>(
        &'a self,
        path: &'a str,
        token: &'a str,
        body: Value,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>> {
        let _ = (path, token, body);
        Box::pin(async { Err("Discord REST POST not implemented".to_owned()) })
    }
}

#[derive(Debug, Clone)]
pub struct ReqwestDiscordRest {
    client: reqwest::Client,
    base: &'static str,
}

impl ReqwestDiscordRest {
    /// Build a rustls-only Discord REST client pinned to `discord.com/api/v10`.
    ///
    /// The base URL is not configurable by callers; all paths are appended only
    /// after rejecting absolute URLs and non-leading-slash values.
    ///
    /// # Errors
    ///
    /// Returns the reqwest builder error if the TLS/client setup fails.
    pub fn new() -> Result<Self, reqwest::Error> {
        Ok(Self {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .redirect(reqwest::redirect::Policy::none())
                .build()?,
            base: DISCORD_API_BASE,
        })
    }

    pub(super) fn url_for(&self, path: &str) -> Result<String, String> {
        if !path.starts_with('/') || path.starts_with("//") || path.contains("://") {
            return Err("Discord REST path must be host-relative".to_owned());
        }
        Ok(format!("{}{}", self.base, path))
    }
}

impl DiscordRest for ReqwestDiscordRest {
    fn get_json<'a>(
        &'a self,
        path: &'a str,
        token: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>> {
        Box::pin(async move {
            let url = self.url_for(path)?;
            let res = self
                .client
                .get(url)
                .header(reqwest::header::AUTHORIZATION, format!("Bot {token}"))
                .send()
                .await
                .map_err(|_| "Discord REST request failed".to_owned())?;
            let status = res.status().as_u16();
            let retry_after = res
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<f64>().ok());
            let body = res.json::<Value>().await.unwrap_or(Value::Null);
            Ok(DiscordHttpResponse {
                status,
                body,
                retry_after,
            })
        })
    }

    fn post_json<'a>(
        &'a self,
        path: &'a str,
        token: &'a str,
        body: Value,
    ) -> Pin<Box<dyn Future<Output = Result<DiscordHttpResponse, String>> + Send + 'a>> {
        Box::pin(async move {
            let url = self.url_for(path)?;
            let res = self
                .client
                .post(url)
                .header(reqwest::header::AUTHORIZATION, format!("Bot {token}"))
                .json(&body)
                .send()
                .await
                .map_err(|_| "Discord REST request failed".to_owned())?;
            let status = res.status().as_u16();
            let retry_after = res
                .headers()
                .get("retry-after")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<f64>().ok());
            let body = res.json::<Value>().await.unwrap_or(Value::Null);
            Ok(DiscordHttpResponse {
                status,
                body,
                retry_after,
            })
        })
    }
}

#[derive(Debug, Clone)]
pub struct DiscordEnv {
    pub home: PathBuf,
    pub ghq_root: PathBuf,
    pub hostname: String,
}

/// `ghq`'s own canonical root config — `git config --get ghq.root`, the
/// standard way `ghq` itself is configured. `$GHQ_ROOT` is an alternate,
/// less common override; when it's unset this must NOT silently fall back
/// to `$HOME`, or hybrid `.discord/` resolution (`find_hybrid_discord`,
/// which scans under `ghq_root` for a directory named `<bot>`) breaks for
/// every host that configured ghq the standard way instead of exporting
/// the env var.
fn git_config_ghq_root() -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["config", "--get", "ghq.root"])
        .output()
        .ok()?;
    parse_git_config_ghq_root_output(output.status.success(), &output.stdout)
}

/// Split out from `git_config_ghq_root` so the parsing/trimming logic is
/// testable without shelling out — `git config` behavior itself depends on
/// the environment's config, which a unit test shouldn't assume.
fn parse_git_config_ghq_root_output(success: bool, stdout: &[u8]) -> Option<PathBuf> {
    if !success {
        return None;
    }
    let root = String::from_utf8_lossy(stdout).trim().to_owned();
    (!root.is_empty()).then(|| PathBuf::from(root))
}

impl DiscordEnv {
    #[must_use]
    pub fn from_process() -> Self {
        let home = env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
        let ghq_root = env::var_os("GHQ_ROOT")
            .map(PathBuf::from)
            .or_else(git_config_ghq_root)
            .unwrap_or_else(|| home.clone());
        let hostname = env::var("HOSTNAME")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| {
                Command::new("hostname")
                    .output()
                    .ok()
                    .and_then(|out| String::from_utf8(out.stdout).ok())
                    .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned())
            });
        Self {
            home,
            ghq_root,
            hostname,
        }
    }

    pub(super) fn pass_dir(&self) -> PathBuf {
        self.home.join(".password-store/discord")
    }

    pub(super) fn legacy_state_root(&self) -> PathBuf {
        self.home.join(".claude/channels")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenEntry {
    pub name: String,
    pub bot: String,
    pub file: PathBuf,
    pub size_bytes: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccessFile {
    #[serde(default = "default_dm_policy")]
    pub(super) dm_policy: String,
    #[serde(default)]
    pub(super) allow_from: Vec<String>,
    #[serde(default)]
    pub(super) groups: BTreeMap<String, AccessGroup>,
    #[serde(default)]
    pub(super) pending: BTreeMap<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub(super) struct AccessGroup {
    #[serde(default = "default_true")]
    pub(super) require_mention: bool,
    #[serde(default)]
    pub(super) allow_from: Vec<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct Guild {
    pub(super) id: String,
    pub(super) name: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(super) struct Channel {
    pub(super) id: String,
    pub(super) name: String,
    #[serde(rename = "type")]
    pub(super) kind: u8,
    #[serde(default)]
    pub(super) parent_id: Option<String>,
    #[serde(default)]
    pub(super) guild_id: Option<String>,
}

pub(super) fn default_true() -> bool {
    true
}

pub(super) fn default_dm_policy() -> String {
    "allowlist".to_owned()
}

#[cfg(test)]
mod ghq_root_fallback_tests {
    use super::parse_git_config_ghq_root_output;
    use std::path::PathBuf;

    #[test]
    fn success_with_value_returns_trimmed_path() {
        assert_eq!(
            parse_git_config_ghq_root_output(true, b"/opt/Code\n"),
            Some(PathBuf::from("/opt/Code"))
        );
    }

    #[test]
    fn success_with_empty_output_returns_none() {
        // `git config --get` can exit 0 with empty stdout in some edge cases
        // (e.g. a key set to an empty string) — must not become PathBuf::from("").
        assert_eq!(parse_git_config_ghq_root_output(true, b"\n"), None);
    }

    #[test]
    fn nonzero_exit_returns_none() {
        // git exits 1 when the key isn't set at all — this is the common case
        // (no ghq.root configured), must fall through to the next fallback,
        // not error out.
        assert_eq!(parse_git_config_ghq_root_output(false, b""), None);
    }
}
