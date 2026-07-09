//maw:noauto

fn atlas_fake_discord() -> Option<AtlasFakeDiscord> {
    let raw = std::env::var(ATLAS_FAKE_DISCORD_ENV).ok()?;
    serde_json::from_str(&raw).ok()
}

async fn atlas_render_fake(parsed: &AtlasArgs, fake: &AtlasFakeDiscord) -> Result<String, String> {
    if fake.bot != parsed.bot {
        return Err(format!("discord-inv: fake discord has bot '{}', requested '{}'", fake.bot, parsed.bot));
    }
    let guilds = atlas_filter_guilds(parsed, &fake.guilds)?;
    let gateway_events = atlas_gateway_observed_count(&fake.gateway_events).await;
    if parsed.json {
        return Ok(atlas_render_json(&fake.bot, gateway_events, &guilds));
    }
    Ok(atlas_render_text(&fake.bot, gateway_events, &guilds))
}

fn atlas_filter_guilds(parsed: &AtlasArgs, guilds: &[AtlasGuild]) -> Result<Vec<AtlasGuild>, String> {
    let mut selected = if let Some(guild_id) = &parsed.guild {
        guilds.iter().filter(|guild| &guild.id == guild_id).cloned().collect::<Vec<_>>()
    } else if parsed.all_guilds {
        guilds.to_vec()
    } else {
        guilds.iter().take(1).cloned().collect()
    };
    if !parsed.with_threads {
        for guild in &mut selected {
            guild.channels.retain(|channel| !matches!(channel.kind, 10..=12));
        }
    }
    atlas_validate_fake_ids(&selected)?;
    Ok(selected)
}

fn atlas_validate_fake_ids(guilds: &[AtlasGuild]) -> Result<(), String> {
    for guild in guilds {
        atlas_validate_snowflake("guild", &guild.id)?;
        for channel in &guild.channels {
            atlas_validate_snowflake("channel", &channel.id)?;
            for user in &channel.allow_from {
                atlas_validate_snowflake("user", user)?;
            }
        }
    }
    Ok(())
}

async fn atlas_gateway_observed_count(events: &[String]) -> usize {
    maw_discord::gateway::observe_mock_gateway_events(events).await
}

fn atlas_render_json(bot: &str, gateway_events: usize, guilds: &[AtlasGuild]) -> String {
    let value = serde_json::json!({
        "bot": bot,
        "gatewayEvents": gateway_events,
        "guilds": guilds,
    });
    format!("{}\n", serde_json::to_string_pretty(&value).unwrap_or_default())
}

fn atlas_render_text(bot: &str, gateway_events: usize, guilds: &[AtlasGuild]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "🗺️ discord-inv — Discord oracle registry for {bot}");
    let _ = writeln!(out, "  gateway: {gateway_events} event(s) observed");
    let mut total_channels = 0usize;
    let mut total_enabled = 0usize;
    for guild in guilds {
        total_channels = total_channels.saturating_add(guild.channels.len());
        total_enabled = total_enabled.saturating_add(guild.channels.iter().filter(|channel| channel.enabled).count());
        let _ = writeln!(out, "  ▼ {} ({}) · {} channel(s)", guild.name, guild.id, guild.channels.len());
        let mut channels = guild.channels.clone();
        channels.sort_by(|left, right| left.name.cmp(&right.name));
        for channel in channels {
            let _ = writeln!(out, "{}", atlas_render_channel(&channel));
        }
    }
    let _ = writeln!(out, "summary: {} server(s) · {total_channels} channels visible · {total_enabled} enabled", guilds.len());
    out
}

fn atlas_render_channel(channel: &AtlasChannel) -> String {
    if channel.enabled {
        let mention = if channel.require_mention { "mention" } else { "all-msg" };
        let allow = if channel.allow_from.is_empty() { "EVERYONE".to_owned() } else { channel.allow_from.join(",") };
        format!("     ✓ #{:<36} {mention} {allow}", channel.name)
    } else {
        format!("     · #{:<36} (in guild, no access)", channel.name)
    }
}
