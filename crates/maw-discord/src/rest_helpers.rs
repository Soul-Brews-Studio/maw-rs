use super::*;

pub(super) fn resolve_bot_for_rest(
    env: &DiscordEnv,
    bot: &str,
    log: &mut Vec<String>,
) -> Option<(BotResolved, String, String)> {
    let pre = resolve_bot(env, bot, log)?;
    let token = decrypt_token(&pre.token_name)?;
    Some((pre, token, bot.to_owned()))
}

pub(super) async fn fetch_guilds(rest: &dyn DiscordRest, token: &str) -> Result<Vec<Guild>, String> {
    let res = rest.get_json("/users/@me/guilds", token).await?;
    if !(200..300).contains(&res.status) {
        return Err(format!("guilds REST {}", res.status));
    }
    serde_json::from_value(res.body).map_err(|e| e.to_string())
}

pub(super) async fn fetch_channels(
    rest: &dyn DiscordRest,
    token: &str,
    guild_id: &str,
) -> Result<Vec<Channel>, String> {
    if !is_numeric_snowflake(guild_id) {
        return Err(format!("invalid guild id '{guild_id}'"));
    }
    let res = rest
        .get_json(&format!("/guilds/{guild_id}/channels"), token)
        .await?;
    if !(200..300).contains(&res.status) {
        return Err(format!("channels REST {} for guild {guild_id}", res.status));
    }
    serde_json::from_value(res.body).map_err(|e| e.to_string())
}

pub(super) async fn resolve_user_list(rest: &dyn DiscordRest, token: &str, ids: &[String]) -> Vec<Value> {
    let mut out = Vec::new();
    for id in ids {
        if !is_numeric_snowflake(id) {
            out.push(json!({"id": id, "name": id, "invalid": true}));
            continue;
        }
        let path = format!("/users/{id}");
        let name = match rest.get_json(&path, token).await {
            Ok(res) if (200..300).contains(&res.status) => res
                .body
                .get("global_name")
                .or_else(|| res.body.get("username"))
                .and_then(Value::as_str)
                .map_or_else(|| id.clone(), ToOwned::to_owned),
            _ => id.clone(),
        };
        out.push(json!({"id": id, "name": name}));
    }
    out
}
