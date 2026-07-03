use std::time::Duration;

use maw_discord::{DiscordHttpResponse, DiscordRest, ReqwestDiscordRest};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::sleep;

use crate::error::{Error, Result};

#[derive(Debug, Clone, Deserialize)]
pub struct Guild {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Channel {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub kind: u8,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Message {
    pub id: String,
    pub channel_id: Option<String>,
    pub author: Option<MessageAuthor>,
    pub content: Option<String>,
    pub timestamp: Option<String>,
    pub edited_timestamp: Option<String>,
    pub attachments: Option<Vec<MessageAttachment>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageAuthor {
    pub id: String,
    pub username: Option<String>,
    pub bot: Option<bool>,
    pub global_name: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MessageAttachment {
    pub id: String,
    pub filename: String,
    pub size: u64,
    pub url: Option<String>,
}

pub fn new_rest() -> Result<ReqwestDiscordRest> {
    ReqwestDiscordRest::new().map_err(|e| Error::Api(e.to_string()))
}

async fn get_json(rest: &dyn DiscordRest, path: &str, token: &str) -> Result<DiscordHttpResponse> {
    for _attempt in 0..8 {
        let res = rest.get_json(path, token).await.map_err(Error::Api)?;
        if res.status == 429 {
            let wait = res.retry_after.unwrap_or(1.0) + 0.5;
            sleep(Duration::from_secs_f64(wait)).await;
            continue;
        }
        if (200..300).contains(&res.status) {
            return Ok(res);
        }
        return Err(Error::Api(format!("{} GET {}", res.status, path)));
    }
    Err(Error::Api(format!("rate limited: {path}")))
}

pub async fn whoami(rest: &dyn DiscordRest, token: &str) -> Result<Value> {
    Ok(get_json(rest, "/users/@me", token).await?.body)
}

pub async fn list_guilds(rest: &dyn DiscordRest, token: &str) -> Result<Vec<Guild>> {
    let body = get_json(rest, "/users/@me/guilds", token).await?.body;
    serde_json::from_value(body).map_err(Error::Json)
}

pub async fn guild_channels(
    rest: &dyn DiscordRest,
    token: &str,
    guild_id: &str,
) -> Result<Vec<Channel>> {
    let path = format!("/guilds/{guild_id}/channels");
    let body = get_json(rest, &path, token).await?.body;
    serde_json::from_value(body).map_err(Error::Json)
}

pub fn filter_text_channels(channels: &[Channel]) -> Vec<Channel> {
    channels.iter().filter(|c| c.kind == 0).cloned().collect()
}

/// Snowflake id ordering — numeric compare with string fallback.
pub fn snowflake_le(a: &str, b: &str) -> bool {
    match (a.parse::<u128>(), b.parse::<u128>()) {
        (Ok(a_id), Ok(b_id)) => a_id <= b_id,
        _ => a <= b,
    }
}

#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub messages: Vec<Message>,
    /// True when `limit` was reached but channel history may continue older.
    pub cap_hit: bool,
}

pub async fn fetch_messages(
    rest: &dyn DiscordRest,
    token: &str,
    channel_id: &str,
    limit: usize,
    stop_at_id: Option<&str>,
) -> Result<FetchOutcome> {
    let mut all = Vec::new();
    let mut before: Option<String> = None;
    let mut cap_hit = false;

    while all.len() < limit {
        let want = limit.saturating_sub(all.len()).min(100);
        let mut path = format!("/channels/{channel_id}/messages?limit={want}");
        if let Some(ref b) = before {
            path.push_str(&format!("&before={b}"));
        }
        let batch: Vec<Message> = serde_json::from_value(get_json(rest, &path, token).await?.body)
            .map_err(Error::Json)?;
        if batch.is_empty() {
            break;
        }
        let batch_len = batch.len();
        let mut hit_stop = false;
        for msg in batch {
            if let Some(stop) = stop_at_id {
                if snowflake_le(msg.id.as_str(), stop) {
                    hit_stop = true;
                    break;
                }
            }
            all.push(msg);
            if all.len() >= limit {
                cap_hit = !hit_stop;
                break;
            }
        }
        before = all.last().map(|m| m.id.clone());
        if hit_stop || batch_len < want {
            break;
        }
        sleep(Duration::from_millis(500)).await;
    }

    all.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(FetchOutcome {
        messages: all,
        cap_hit,
    })
}

pub fn slim_message(m: &Message) -> Message {
    Message {
        id: m.id.clone(),
        channel_id: m.channel_id.clone(),
        author: m.author.as_ref().map(|a| MessageAuthor {
            id: a.id.clone(),
            username: a.username.clone(),
            bot: Some(a.bot.unwrap_or(false)),
            global_name: a.global_name.clone(),
        }),
        content: Some(m.content.clone().unwrap_or_default()),
        timestamp: m.timestamp.clone(),
        edited_timestamp: m.edited_timestamp.clone(),
        attachments: Some(
            m.attachments
                .as_ref()
                .map(|items| {
                    items
                        .iter()
                        .map(|a| MessageAttachment {
                            id: a.id.clone(),
                            filename: a.filename.clone(),
                            size: a.size,
                            url: a.url.clone(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Mutex;

    use super::*;
    use serde_json::json;

    struct MockRest {
        responses: BTreeMap<String, DiscordHttpResponse>,
        calls: Mutex<Vec<String>>,
    }

    impl MockRest {
        fn new(responses: BTreeMap<String, DiscordHttpResponse>) -> Self {
            Self {
                responses,
                calls: Mutex::new(Vec::new()),
            }
        }
    }

    impl DiscordRest for MockRest {
        fn get_json<'a>(
            &'a self,
            path: &'a str,
            _token: &'a str,
        ) -> Pin<
            Box<dyn Future<Output = std::result::Result<DiscordHttpResponse, String>> + Send + 'a>,
        > {
            Box::pin(async move {
                self.calls.lock().expect("calls").push(path.to_owned());
                self.responses
                    .get(path)
                    .cloned()
                    .ok_or_else(|| format!("missing mock {path}"))
            })
        }
    }

    #[test]
    fn filter_text_channels_keeps_type_zero() {
        let channels = vec![
            Channel {
                id: "1".into(),
                name: "general".into(),
                kind: 0,
            },
            Channel {
                id: "2".into(),
                name: "voice".into(),
                kind: 2,
            },
        ];
        let text = filter_text_channels(&channels);
        assert_eq!(text.len(), 1);
        assert_eq!(text[0].name, "general");
    }

    #[tokio::test]
    async fn fetch_messages_sets_cap_hit_at_limit() {
        let channel = "cap-test";
        let mut responses = BTreeMap::new();
        responses.insert(
            format!("/channels/{channel}/messages?limit=5"),
            DiscordHttpResponse {
                status: 200,
                body: json!([
                    {"id": "1000000000000000005", "content": "e"},
                    {"id": "1000000000000000004", "content": "d"},
                    {"id": "1000000000000000003", "content": "c"},
                    {"id": "1000000000000000002", "content": "b"},
                    {"id": "1000000000000000001", "content": "a"},
                ]),
                retry_after: None,
            },
        );
        let rest = MockRest::new(responses);
        let out = fetch_messages(&rest, "tok", channel, 5, None)
            .await
            .expect("fetch");
        assert_eq!(out.messages.len(), 5);
        assert!(out.cap_hit);
    }

    #[tokio::test]
    async fn fetch_messages_stops_at_watermark_and_sorts() {
        let channel = "1500775333283237970";
        let mut responses = BTreeMap::new();
        responses.insert(
            format!("/channels/{channel}/messages?limit=5"),
            DiscordHttpResponse {
                status: 200,
                body: json!([
                    {"id": "1000000000000000100", "content": "new"},
                    {"id": "1000000000000000090", "content": "mid"},
                    {"id": "1000000000000000080", "content": "old"},
                ]),
                retry_after: None,
            },
        );
        let rest = MockRest::new(responses);
        let out = fetch_messages(&rest, "tok", channel, 5, Some("1000000000000000095"))
            .await
            .expect("fetch");
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0].id, "1000000000000000100");
        assert!(!out.cap_hit);
    }

    #[test]
    fn snowflake_le_orders_numeric_ids() {
        assert!(snowflake_le("1492501367409873087", "1521691823519567873"));
        assert!(!snowflake_le("1521691823519567873", "1492501367409873087"));
        assert!(snowflake_le("1000000000000000095", "1000000000000000100"));
    }

    #[test]
    fn slim_message_defaults_content_and_bot() {
        let slim = slim_message(&Message {
            id: "1".into(),
            channel_id: None,
            author: Some(MessageAuthor {
                id: "u1".into(),
                username: Some("bot".into()),
                bot: None,
                global_name: None,
            }),
            content: None,
            timestamp: None,
            edited_timestamp: None,
            attachments: None,
        });
        assert_eq!(slim.content.as_deref(), Some(""));
        assert_eq!(slim.author.as_ref().and_then(|a| a.bot), Some(false));
    }
}
